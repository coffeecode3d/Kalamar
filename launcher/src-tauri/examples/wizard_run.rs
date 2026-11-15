//! Runs the install wizard's pipeline without the window on one folder (the same `installer::Installer` the page
//! drives) and prints what the plan announced, then each step with its duration. Developer tool for measuring installs
//! and updates, not part of the app.
//!
//!   cargo run --release --example wizard_run -- <install dir> <bundle dir> [--jobs N] [--force] [--free-build-files]
//!       [--base <base .nsp> --update <update .nsp> --keys <prod.keys>] [--plan-only | --check | --rollback]
//!       [--probe <data dir>] [--hold <s> --hold-at start|compile]
//!
//! `--plan-only` prints the plan screen's content and stops; `--check` prints what the Play page asks (is an update
//! available, what it would redo) and how long that took; `--rollback` does the Installation page's "Revert to the
//! previous version".
//!
//! `<bundle dir>` holds a `runtime-src.tar.gz` and its `runtime-src.json` (`examples/bundle_runtime.rs --out`), and may
//! hold `kalamar-tools.*` (`examples/bundle_tools.rs`; else the copy compiled in is used). A first install takes the
//! player's three files by their paths (`--base`, `--update`, `--keys`: only read, by the extraction tools); without
//! them the folder must already hold the extracted ExeFS and the merged RomFS. The launcher's own data folder and
//! configuration are neither read nor written. Every snapshot change of a step (status, phase, steps running beside)
//! is printed with its time, so the output is a timeline of the run.
//!
//! What a player does while the install runs, checked with the launcher's own code (developer checks):
//!
//! * `--probe <data dir>` (a launcher data folder of its own, never the real one): the Locker's catalogue is built from
//!   the install's folder the moment its merged game data is complete (`Snapshot::game_data`), and the estimate the
//!   Locker's card shows before is printed; once the compile compiles, game settings (`build\kalamar.ini`) and a profile
//!   made from that catalogue (`profile.json` of that data folder) are written as the Settings page and the Locker
//!   write them; at the end both are read back, the profile is turned into the file a first launch hands the game, and
//!   the self-check's log says whether the game read the settings.
//! * `--hold <s>` with `--hold-at start|compile`: the game of this install held open for that long (`build\splat3.exe
//!   --selftest` started suspended: the executable is in use as by a running game, nothing of it runs), from before the
//!   run or from the moment the compile compiles; then let go (the self-check runs and ends). The update must wait for
//!   it and finish.

use kalamar_lib::installer::{self, InstallRequest, Installer, Snapshot, StepId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The game of an install held open: `splat3.exe --selftest` created suspended (its image mapped, nothing run).
#[cfg(windows)]
struct Held {
    process: windows_sys::Win32::Foundation::HANDLE,
    thread: windows_sys::Win32::Foundation::HANDLE,
    pid: u32,
}

#[cfg(windows)]
impl Held {
    fn start(exe: &Path) -> Option<Held> {
        use windows_sys::Win32::System::Threading::*;
        let cmd: Vec<u16> = format!("\"{}\" --selftest", exe.display()).encode_utf16().chain(std::iter::once(0)).collect();
        let mut cmd = cmd;
        let dir: Vec<u16> = exe.parent()?.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: zeroed structures are the documented empty values; the strings outlive the call.
        unsafe {
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let ok = CreateProcessW(std::ptr::null(), cmd.as_mut_ptr(), std::ptr::null(), std::ptr::null(), 0, CREATE_SUSPENDED | CREATE_NO_WINDOW, std::ptr::null(), dir.as_ptr(), &si, &mut pi);
            (ok != 0).then_some(Held { process: pi.hProcess, thread: pi.hThread, pid: pi.dwProcessId })
        }
    }
    /// Lets it run: the self-check runs and the process ends by itself. Returns its exit code.
    fn release(self) -> u32 {
        use windows_sys::Win32::System::Threading::*;
        // SAFETY: our own handles, closed once.
        unsafe {
            ResumeThread(self.thread);
            WaitForSingleObject(self.process, 60_000);
            let mut code = 0u32;
            windows_sys::Win32::System::Threading::GetExitCodeProcess(self.process, &mut code);
            windows_sys::Win32::Foundation::CloseHandle(self.thread);
            windows_sys::Win32::Foundation::CloseHandle(self.process);
            code
        }
    }
}

/// A complete profile from the catalogue: the first selectable look of the first style, its species' hair and
/// eyebrows, the first legwear, gear and weapon the game keeps.
fn probe_profile(cat: &kalamar_lib::game::catalogue::Catalogue) -> kalamar_lib::profile::Profile {
    let first = |c: &str, f: &dyn Fn(&kalamar_lib::game::catalogue::Entry) -> bool| cat.categories.get(c).and_then(|v| v.iter().find(|e| e.selectable && f(e)).map(|e| e.id));
    let style = first("style", &|_| true);
    let species = style.and_then(|s| cat.categories.get("style")?.iter().find(|e| e.id == s)?.species.clone());
    let same = |e: &kalamar_lib::game::catalogue::Entry| species.is_none() || e.species.is_none() || e.species == species;
    let kept = |slot: &'static str| move |e: &kalamar_lib::game::catalogue::Entry| !kalamar_lib::game::catalogue::DROPPED_GEAR.contains(&(slot, e.id));
    kalamar_lib::profile::Profile {
        name: "Attente".into(),
        style,
        skin: first("skin", &|_| true),
        eyes: first("eyes", &|_| true),
        hair: first("hair", &same),
        eyebrows: first("eyebrows", &same),
        legwear: first("legwear", &|_| true),
        legwear_variation: Some(0),
        head: first("head", &kept("head")),
        clothes: first("clothes", &kept("clothes")),
        shoes: first("shoes", &kept("shoes")),
        weapon: first("weapon", &|_| true),
        ..Default::default()
    }
}

const PROBE_SETTINGS: [(&str, &str); 4] = [("mouse_gyro_cm360", "34.5"), ("mouse_gyro_dpi", "1600"), ("fullscreen", "1"), ("window_size", "1600x900")];

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 2 {
        eprintln!("usage: wizard_run <install dir> <bundle dir> [--jobs N] [--force] [--free-build-files] [--plan-only | --check | --rollback] [--probe <data dir>] [--hold <s> --hold-at start|compile]");
        std::process::exit(2);
    }
    let flag = |n: &str| a.iter().any(|x| x == n);
    let opt = |n: &str| a.iter().position(|x| x == n).and_then(|i| a.get(i + 1)).cloned().unwrap_or_default();
    let jobs = opt("--jobs").parse().unwrap_or(0);
    let bundle = PathBuf::from(&a[1]);
    let req = InstallRequest {
        install_dir: a[0].clone(),
        base_nsp: opt("--base"),
        update_nsp: opt("--update"),
        prod_keys: opt("--keys"),
        jobs,
        force_rebuild: flag("--force"),
        free_build_files: flag("--free-build-files"),
        ..Default::default()
    };

    if flag("--rollback") {
        // "Revert to the previous version" of the Installation page.
        let t = Instant::now();
        let r = installer::rollback(std::path::Path::new(&a[0]));
        println!("rollback ({:.2} s): {}", t.elapsed().as_secs_f64(), serde_json::to_string(&r.map_err(|e| e.to_string())).unwrap_or_default());
        return;
    }
    if flag("--check") {
        // What the Play page asks (no tool detection): is an update available, what would it redo.
        let t = Instant::now();
        let c = installer::update_check(std::path::Path::new(&a[0]), &bundle);
        println!("update check ({:.2} s): {}", t.elapsed().as_secs_f64(), serde_json::to_string_pretty(&c).unwrap_or_default());
        return;
    }
    let t = Instant::now();
    let plan = installer::plan(&req, Some(&bundle));
    println!("plan ({:.1} s): source {} -> {}, {} jobs, estimate {:.0} s (at most {:.0} s)", t.elapsed().as_secs_f64(), plan.installed_source.as_deref().unwrap_or("-"), plan.source_commit.as_deref().unwrap_or("-"), plan.jobs, plan.eta, plan.eta_max);
    println!(
        "plan: tools {:?} in {}, downloads {:.1} MB, disk needed {:.1} GB (fullest at {:?}), {:.1} GB at the end, {:.1} GB free",
        plan.toolchain,
        plan.tools_dir,
        plan.download_bytes as f64 / 1e6,
        plan.disk_needed as f64 / 1e9,
        plan.disk_peak_step.map(|s| s.name()),
        plan.footprint as f64 / 1e9,
        plan.disk_free.unwrap_or(0) as f64 / 1e9
    );
    for d in plan.downloads.iter().filter(|d| d.needed) {
        println!("  download {:8} {:>8.1} MB  {}", d.id, d.bytes as f64 / 1e6, d.source);
    }
    if let Some(u) = &plan.update {
        println!("update: {}", serde_json::to_string(u).unwrap_or_default());
    }
    for s in &plan.steps {
        println!("  {:9} {}", s.id.name(), if s.done { "kept".to_string() } else { format!("~{:.0} s", s.reference) });
    }
    if flag("--plan-only") {
        return;
    }

    let seen: Arc<Mutex<HashMap<String, String>>> = Arc::default();
    let started = Instant::now();
    let emit: installer::Emit = Arc::new(move |s: &Snapshot| {
        let mut seen = seen.lock().unwrap();
        let now = started.elapsed().as_secs_f64();
        for v in &s.steps {
            let key = format!("status {}", v.id.name());
            if seen.get(&key).map(String::as_str) != Some(v.status) {
                seen.insert(key, v.status.to_string());
                if v.status != "pending" {
                    let secs = v.secs.map(|x| format!(" {x:.1} s")).unwrap_or_default();
                    println!("[{now:7.1}] {:9} {}{secs}", v.id.name(), v.status);
                }
            }
        }
        // The phase of the current step and of the steps beside it (once per phase, with the ETA shown then).
        let mut phases: Vec<(StepId, Option<String>)> = s.current.map(|c| vec![(c, s.stage.clone())]).unwrap_or_default();
        phases.extend(s.background.iter().map(|b| (b.id, b.stage.clone())));
        for (id, stage) in phases {
            let key = format!("stage {}", id.name());
            let val = stage.unwrap_or_default();
            if !val.is_empty() && seen.get(&key) != Some(&val) {
                seen.insert(key, val.clone());
                let beside = if Some(id) == s.current { "" } else { " (beside)" };
                println!("[{now:7.1}] {:9} phase {val}{beside}; overall {:.0} %, ETA {:.0} s", id.name(), s.overall * 100.0, s.eta.unwrap_or(0.0));
            }
        }
    });
    let probe = Some(opt("--probe")).filter(|p| !p.is_empty()).map(PathBuf::from);
    let hold_secs: u64 = opt("--hold").parse().unwrap_or(0);
    let hold_at = opt("--hold-at");
    let lay = installer::Layout::new(&a[0]);
    let game_root = lay.game();
    let selftest_log = lay.logs().join("verif-selftest.log");
    let selftest_from = std::fs::metadata(&selftest_log).map(|m| m.len()).unwrap_or(0);
    #[cfg(windows)]
    let mut held: Option<(Held, Instant)> = None;
    #[cfg(windows)]
    if hold_secs > 0 && hold_at == "start" {
        held = Held::start(&lay.game_exe()).map(|h| (h, Instant::now()));
        println!("[{:7.1}] probe: the game of this install is held open (pid {:?}, splat3.exe --selftest suspended) for {hold_secs} s", started.elapsed().as_secs_f64(), held.as_ref().map(|h| h.0.pid));
    }

    let inst = Installer::default();
    if let Err(e) = inst.start(req, Some(bundle), emit, Arc::new(|_| {})) {
        eprintln!("not started: {e}");
        std::process::exit(1);
    }
    // The page's side of the run: what the Locker and the Settings page would do, and a game opened and closed.
    let catalogue: Arc<Mutex<Option<Arc<kalamar_lib::game::catalogue::Catalogue>>>> = Arc::default();
    let mut locker_started = false;
    let mut written: Option<kalamar_lib::profile::Profile> = None;
    let mut last_eta_print = Instant::now() - Duration::from_secs(60);
    let mut was_waiting = false;
    while inst.active() {
        std::thread::sleep(Duration::from_millis(250));
        let s = inst.snapshot();
        let now = started.elapsed().as_secs_f64();
        if s.waiting.is_some() != was_waiting {
            was_waiting = s.waiting.is_some();
            println!("[{now:7.1}] probe: {}", if was_waiting { format!("the run waits: {:?} (step {:?})", s.waiting, s.current.map(|c| c.name())) } else { "the run goes on".into() });
        }
        if let Some(dir) = &probe {
            if !s.game_data && last_eta_print.elapsed() >= Duration::from_secs(20) {
                last_eta_print = Instant::now();
                println!("[{now:7.1}] probe: Locker card: game data in about {:.0} s (current step {:?})", s.game_data_eta.unwrap_or(-1.0), s.current.map(|c| c.name()));
            }
            if s.game_data && !locker_started {
                // The Locker opened now: it reads the install's folder (the launcher's `data_root`), however far the compile is.
                locker_started = true;
                let (root, dir, cat, cur) = (game_root.clone(), dir.clone(), catalogue.clone(), s.current);
                println!("[{now:7.1}] probe: game data complete (current step {:?}, compile {}): opening the Locker", cur.map(|c| c.name()), s.steps.iter().find(|v| v.id == StepId::Compile).map(|v| v.status).unwrap_or("?"));
                std::thread::spawn(move || {
                    let t = Instant::now();
                    let svc = kalamar_lib::game::service::GameService::default();
                    match svc.load(&root, &dir, false) {
                        Ok(l) => {
                            let n = |c: &str| l.catalogue.categories.get(c).map_or(0, |v| v.iter().filter(|e| e.selectable).count());
                            println!(
                                "[{:7.1}] probe: Locker catalogue ready in {:.1} s (from cache: {}): {} styles, {} hair, {} weapons, {} head gear, {} stages",
                                started.elapsed().as_secs_f64(),
                                t.elapsed().as_secs_f64(),
                                l.from_cache,
                                n("style"),
                                n("hair"),
                                n("weapon"),
                                n("head"),
                                l.catalogue.stages.len()
                            );
                            *cat.lock().unwrap() = Some(l.catalogue.clone());
                        }
                        Err(e) => println!("[{:7.1}] probe: Locker catalogue FAILED: {e}", started.elapsed().as_secs_f64()),
                    }
                });
            }
            let cat_now = catalogue.lock().unwrap().clone();
            if written.is_none() && s.current == Some(StepId::Compile) && s.stage.as_deref() == Some("compile") && let Some(cat) = cat_now {
                // The Settings page's save and the Locker's save, while the compile compiles.
                let schema = kalamar_lib::settings::Schema::embedded();
                let ini = kalamar_lib::install::ini_path(&game_root);
                let changes: Vec<kalamar_lib::settings::Change> = PROBE_SETTINGS.iter().map(|(k, v)| kalamar_lib::settings::Change { key: k.to_string(), value: Some(v.to_string()) }).collect();
                let r = kalamar_lib::settings::save(&ini, &schema, &changes);
                let p = kalamar_lib::profile::validate_profile(&probe_profile(&cat), &cat, None);
                match (&r, &p) {
                    (Ok(_), Ok(p)) => {
                        kalamar_lib::profile::save_profile(dir, p).expect("profile saved");
                        println!("[{now:7.1}] probe: during the compile ({}): settings written to {} and profile '{}' saved", s.detail.clone().unwrap_or_default(), ini.display(), p.name);
                        written = Some(p.clone());
                    }
                    _ => {
                        println!("[{now:7.1}] probe: writing during the compile FAILED: settings {:?} profile {:?}", r.err(), p.err());
                        written = Some(kalamar_lib::profile::Profile::default());
                    }
                }
            }
        }
        #[cfg(windows)]
        {
            if hold_secs > 0 && hold_at == "compile" && held.is_none() && s.current == Some(StepId::Compile) && s.stage.as_deref() == Some("compile") {
                held = Held::start(&lay.game_exe()).map(|h| (h, Instant::now()));
                println!("[{now:7.1}] probe: the game of this install is opened during the compile (pid {:?}, splat3.exe --selftest suspended) for {hold_secs} s", held.as_ref().map(|h| h.0.pid));
            }
            if held.as_ref().is_some_and(|(_, t)| t.elapsed() >= Duration::from_secs(hold_secs)) {
                let (h, _) = held.take().unwrap();
                let pid = h.pid;
                let code = h.release();
                println!("[{:7.1}] probe: the game (pid {pid}) is let go: its self-check ran and it ended (exit code {code})", started.elapsed().as_secs_f64());
            }
        }
    }
    #[cfg(windows)]
    if let Some((h, _)) = held.take() {
        let code = h.release();
        println!("probe: the run ended while the game was still held (exit code {code})");
    }
    let s = inst.snapshot();
    if let (Some(dir), Some(p)) = (&probe, &written) {
        // What is left at the end: the settings file and the profile as written, the profile as a first launch hands it
        // to the game, and the settings as the game read them in its self-check.
        let st = kalamar_lib::settings::load(&kalamar_lib::install::ini_path(&game_root), &kalamar_lib::settings::Schema::embedded());
        let intact = st.as_ref().is_ok_and(|st| PROBE_SETTINGS.iter().all(|(k, v)| st.values.get(*k).map(String::as_str) == Some(*v)));
        println!("probe: settings after the install: {} ({:?})", if intact { "intact" } else { "CHANGED OR LOST" }, st.map(|s| s.values));
        let back = kalamar_lib::profile::load_profile(dir);
        println!("probe: profile after the install: {}", if &back == p { "intact" } else { "CHANGED OR LOST" });
        let userdata = game_root.join("userdata");
        let save = kalamar_lib::profile::save_exists(&userdata.join("profile"));
        match kalamar_lib::profile::prepare_launch(&back, kalamar_lib::profile::ApplyOptions::default(), &userdata, save) {
            Ok(env) => println!(
                "probe: first launch: {} written, environment {:?}\n{}",
                kalamar_lib::profile::runtime_profile_path(&userdata).display(),
                env,
                std::fs::read_to_string(kalamar_lib::profile::runtime_profile_path(&userdata)).unwrap_or_default()
            ),
            Err(e) => println!("probe: first launch: profile NOT passed: {e}"),
        }
        let log = std::fs::read(&selftest_log).map(|b| String::from_utf8_lossy(&b[(selftest_from as usize).min(b.len())..]).into_owned()).unwrap_or_default();
        for l in log.lines().filter(|l| l.contains("cfg: ")) {
            println!("probe: the game's self-check: {}", l.trim());
        }
    }
    println!("outcome: {} in {:.1} s{}", s.outcome.unwrap_or("?"), s.elapsed, s.footprint.map(|f| format!(", install folder {:.2} GB", f as f64 / 1e9)).unwrap_or_default());
    for v in &s.steps {
        println!("  {:9} {:8} {}", v.id.name(), v.status, v.secs.map(|x| format!("{x:.1} s")).unwrap_or_default());
    }
    if let Some(e) = &s.error {
        println!("error: step {} code {} {:?} log {:?}", e.step.name(), e.code, e.params, e.log);
        for l in &e.tail {
            println!("  | {l}");
        }
    }
    std::process::exit(if s.outcome == Some("done") { 0 } else { 1 });
}
