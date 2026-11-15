//! The launcher updates itself from its GitHub releases, in a build that names a repository (`channel.rs`); without one
//! this module is inert: no interface, no request.
//!
//! - **Check** (`check.rs`): 20 seconds after every start and then every 6 hours while the launcher stays open, if the
//!   player keeps "check automatically" (on by default), and on "Check now" (Settings, About). GitHub's public release
//!   API, no token, an ETag (an unchanged answer is a bodiless 304), a closed list of hosts (`net.rs`); a failure is
//!   silent.
//! - **Trust** (`manifest.rs`): a release is offered only when its manifest is signed by a key compiled into the
//!   launcher, names this repository, the release's own version and a version above the running one.
//! - **Download**: only on the player's click, in the background, into `<data>\update\download\<tag>\`, resumed after a
//!   cut, never past the signed size, then checked against the signed SHA-256.
//! - **Apply** (`apply.rs`): only on the player's click and when no game, installation or relay runs: the swap of the
//!   program file with a trial and an automatic roll-back, or the release's installer for an installed copy, or a copy
//!   saved where the player says for a program that cannot replace itself.

pub mod apply;
pub mod channel;
pub mod check;
pub mod manifest;
pub mod net;
pub mod version;

#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
pub mod testkit;

use check::{CheckState, Found, Outcome};
use manifest::{Channel, Expect, FileEntry, FileKind};
use net::Source;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use version::Version;

/// The player's choices (`config.json`, `update`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UpdatePrefs {
    /// Check at every start and every 6 hours while open. On by default (the feature only exists in a build that names a repository).
    pub auto_check: bool,
    pub channel: Channel,
}

impl Default for UpdatePrefs {
    fn default() -> Self {
        UpdatePrefs { auto_check: true, channel: Channel::Stable }
    }
}

/// While the launcher stays open, the automatic check repeats this often (the first one is `AUTO_DELAY` after start).
pub const AUTO_EVERY: Duration = Duration::from_secs(6 * 3600);
/// "Check now" pressed again within this many seconds gives the last answer.
const MANUAL_GAP: u64 = 30;
/// The automatic check waits this long after start: never on the first screen's path.
pub const AUTO_DELAY: Duration = Duration::from_secs(20);

/// How long to wait before the next automatic check: `AUTO_DELAY` for the first one after start, `AUTO_EVERY` after.
pub fn auto_wait(first: bool) -> Duration {
    if first { AUTO_DELAY } else { AUTO_EVERY }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Replace the program file, restart.
    Swap,
    /// Start the release's installer (a copy installed with it).
    Installer,
    /// Save the new program in a folder the player picks (temporary or read-only folder).
    Elsewhere,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AvailableView {
    pub version: String,
    pub channel: Channel,
    /// Plain text from the signed manifest.
    pub notes: String,
    pub published: Option<String>,
    pub size: u64,
    pub mode: Mode,
    /// Why `elsewhere`: `temp` or `readonly`.
    pub why: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NoticeView {
    /// `updated`, `rolled_back`, `installer_unfinished`.
    pub kind: &'static str,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

/// Everything the page shows.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateView {
    /// This build has the feature (a repository is configured).
    pub enabled: bool,
    pub current: String,
    pub auto_check: bool,
    pub channel: Channel,
    pub last_check: Option<u64>,
    pub checking: bool,
    /// The last check's failure: `network`, `rate_limited`, `untrusted`.
    pub check_error: Option<String>,
    pub available: Option<AvailableView>,
    /// "Later" was pressed for this version during this run of the launcher: the bar is quiet and the start dialog is
    /// not shown again (nothing is remembered across starts).
    pub later: bool,
    /// `idle`, `downloading`, `ready`, `applying`, `restarting`, `saved`.
    pub phase: &'static str,
    pub progress: Option<Progress>,
    /// The last download or apply failure: `network`, `integrity`, `disk`, `locked`, `not_writable`, `spawn`,
    /// `other_instance`, `installer`, `rollback_failed`, `busy`.
    pub error: Option<String>,
    /// Why "Restart" waits: `game`, `install`, `relay`.
    pub blocked: Option<&'static str>,
    pub saved_to: Option<String>,
    pub notice: Option<NoticeView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Idle,
    Downloading(Progress),
    Ready(PathBuf),
    Applying,
    Restarting,
    Saved(PathBuf),
}

struct Inner {
    state: CheckState,
    checking: bool,
    last_attempt: u64,
    check_error: Option<String>,
    found: Option<Found>,
    phase: Phase,
    error: Option<String>,
    notice: Option<NoticeView>,
    lock: Option<apply::Lock>,
    /// The version "Later" was pressed for, in this run only.
    later: Option<String>,
}

pub struct Updater {
    source: Option<Source>,
    /// `<data>\update`.
    dir: PathBuf,
    current: Version,
    current_text: String,
    exe: Option<PathBuf>,
    temp: Vec<PathBuf>,
    /// Where the program runs from, looked at once (it probes its folder with a file).
    place: std::sync::OnceLock<apply::Place>,
    inner: Mutex<Inner>,
    cancel: AtomicBool,
}

/// What a download needs, taken under the lock.
pub struct DownloadJob {
    pub url: String,
    pub part: PathBuf,
    pub file: PathBuf,
    pub entry: FileEntry,
}

impl Updater {
    /// `data_dir` is the launcher's data folder (`KALAMAR_DATA` respected by the caller). Reads only small files
    /// of `<data>\update\`; no network.
    pub fn new(source: Option<Source>, data_dir: &Path, current: &str, exe: Option<PathBuf>, temp: Vec<PathBuf>, channel: Channel) -> Updater {
        let dir = data_dir.join("update");
        // A running version that does not parse could not refuse an older one: no updates then.
        let (source, current_v) = match Version::parse(current) {
            Some(v) => (source, v),
            None => (None, Version::parse("0.0.0").expect("0.0.0")),
        };
        let state = crate::util::read_text_limited(&dir.join("state.json"), 1 << 20).ok().flatten().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let notice = if source.is_some() { apply::take_notice(&dir, current).map(|(kind, from, to)| NoticeView { kind, from, to }) } else { None };
        let u = Updater {
            source,
            dir,
            current: current_v,
            current_text: current.to_string(),
            exe,
            temp,
            place: std::sync::OnceLock::new(),
            inner: Mutex::new(Inner { state, checking: false, last_attempt: 0, check_error: None, found: None, phase: Phase::Idle, error: None, notice, lock: None, later: None }),
            cancel: AtomicBool::new(false),
        };
        if u.source.is_some() {
            u.load_stored(channel);
        }
        u
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn enabled(&self) -> bool {
        self.source.is_some()
    }

    pub fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn exe(&self) -> Option<&Path> {
        self.exe.as_deref()
    }

    pub fn current(&self) -> &str {
        &self.current_text
    }

    fn available_dir(&self) -> PathBuf {
        self.dir.join("available")
    }

    fn download_root(&self) -> PathBuf {
        self.dir.join("download")
    }

    /// A manifest kept from an earlier check: checked again (signature, still newer); its download, when complete,
    /// makes the update ready. Anything that no longer holds (an older one after an update) is removed.
    fn load_stored(&self, channel: Channel) {
        let Some(src) = &self.source else { return };
        let a = self.available_dir();
        let read = |n: &str| std::fs::read(a.join(n)).ok();
        let (Some(m), Some(s), Some(tag)) = (read(manifest::MANIFEST_NAME), read(manifest::SIGNATURE_NAME), read("tag")) else {
            self.forget_stored();
            return;
        };
        let tag = String::from_utf8_lossy(&tag).trim().to_string();
        let manifest = match (check::valid_tag(&tag), Version::parse_tag(&tag)) {
            (true, Some(tv)) => manifest::verify(&m, &s, &src.keys, &Expect { repo: &src.repo, tag_version: Some(&tv), current: &self.current, channel }).ok(),
            _ => None,
        };
        let Some(manifest) = manifest else {
            self.forget_stored();
            return;
        };
        let found = Found { tag, manifest, manifest_bytes: m, signature: s };
        let mut g = self.lock();
        if let Some(entry) = self.entry_for(&found)
            && let Ok(meta) = std::fs::metadata(self.file_path(&found.tag, &entry.name))
            && meta.len() == entry.size
        {
            // its SHA-256 is checked again right before it is used
            g.phase = Phase::Ready(self.file_path(&found.tag, &entry.name));
        }
        g.found = Some(found);
    }

    fn forget_stored(&self) {
        let _ = std::fs::remove_dir_all(self.available_dir());
        let _ = std::fs::remove_dir_all(self.download_root());
    }

    fn store_found(&self, f: &Found) {
        let a = self.available_dir();
        let _ = crate::util::atomic_write(&a.join(manifest::MANIFEST_NAME), &f.manifest_bytes);
        let _ = crate::util::atomic_write(&a.join(manifest::SIGNATURE_NAME), &f.signature);
        let _ = crate::util::atomic_write(&a.join("tag"), f.tag.as_bytes());
        // downloads of other releases are dropped
        if let Ok(rd) = std::fs::read_dir(self.download_root()) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy() != f.tag {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }

    fn save_state(&self, s: &CheckState) {
        if let Ok(t) = serde_json::to_vec_pretty(s) {
            let _ = crate::util::atomic_write(&self.dir.join("state.json"), &t);
        }
    }

    fn file_path(&self, tag: &str, name: &str) -> PathBuf {
        self.download_root().join(tag).join(name)
    }

    fn place(&self) -> apply::Place {
        *self.place.get_or_init(|| match &self.exe {
            Some(e) => apply::place(e, &self.temp),
            None => apply::Place { installed: false, temp: false, writable: false },
        })
    }

    /// How this copy gets the release: its installer when it was installed with one, a swap when its folder takes it,
    /// else a copy saved where the player says.
    pub fn mode_for(&self, m: &manifest::Manifest) -> (Mode, Option<&'static str>) {
        let p = self.place();
        if p.temp {
            (Mode::Elsewhere, Some("temp"))
        } else if p.installed && m.file(FileKind::Installer).is_some() {
            (Mode::Installer, None)
        } else if p.writable && m.file(FileKind::Portable).is_some() {
            (Mode::Swap, None)
        } else {
            (Mode::Elsewhere, Some("readonly"))
        }
    }

    fn entry_for(&self, f: &Found) -> Option<FileEntry> {
        let kind = match self.mode_for(&f.manifest).0 {
            Mode::Installer => FileKind::Installer,
            Mode::Swap | Mode::Elsewhere => FileKind::Portable,
        };
        f.manifest.file(kind).cloned()
    }

    pub fn view(&self, prefs: &UpdatePrefs, blocked: Option<&'static str>) -> UpdateView {
        let g = self.lock();
        let available = g.found.as_ref().map(|f| {
            let (mode, why) = self.mode_for(&f.manifest);
            AvailableView {
                version: f.manifest.version.to_string(),
                channel: f.manifest.channel,
                notes: f.manifest.notes.clone(),
                published: f.manifest.published.clone(),
                size: self.entry_for(f).map(|e| e.size).unwrap_or(0),
                mode,
                why,
            }
        });
        let later = match (&g.found, &g.later) {
            (Some(f), Some(v)) => *v == f.manifest.version.to_string(),
            _ => false,
        };
        UpdateView {
            enabled: self.source.is_some(),
            current: self.current_text.clone(),
            auto_check: prefs.auto_check,
            channel: prefs.channel,
            last_check: (g.state.last_check > 0).then_some(g.state.last_check),
            checking: g.checking,
            check_error: g.check_error.clone(),
            available,
            later,
            phase: match g.phase {
                Phase::Idle => "idle",
                Phase::Downloading(_) => "downloading",
                Phase::Ready(_) => "ready",
                Phase::Applying => "applying",
                Phase::Restarting => "restarting",
                Phase::Saved(_) => "saved",
            },
            progress: match g.phase {
                Phase::Downloading(p) => Some(p),
                _ => None,
            },
            error: g.error.clone(),
            blocked,
            saved_to: match &g.phase {
                Phase::Saved(p) => Some(p.display().to_string()),
                _ => None,
            },
            notice: g.notice.clone(),
        }
    }

    /// One check. `manual`: "Check now" (a second press within 30 s gives the last answer). `on_start` is called once
    /// the check is under way (the page then says "checking").
    pub async fn check(&self, channel: Channel, manual: bool, on_start: &(dyn Fn() + Send + Sync)) {
        let Some(src) = &self.source else { return };
        let now = crate::util::now_secs();
        let (mut state, known) = {
            let mut g = self.lock();
            if g.checking || (manual && now < g.last_attempt + MANUAL_GAP) {
                return;
            }
            if matches!(g.phase, Phase::Downloading(_) | Phase::Applying | Phase::Restarting) {
                return;
            }
            g.checking = true;
            g.last_attempt = now;
            (g.state.clone(), g.found.clone())
        };
        on_start();
        let outcome = check::run(src, &self.current, channel, &mut state, known.as_ref(), now).await;
        crate::trace::mark(&format!("update:check {}", match &outcome {
            Outcome::UpToDate => "up_to_date".to_string(),
            Outcome::Available(f) => format!("available {}", f.tag),
            Outcome::Failed(code, why) => format!("{code} {why}"),
        }));
        let mut g = self.lock();
        g.checking = false;
        match outcome {
            Outcome::UpToDate => {
                state.last_result = "up_to_date".into();
                g.check_error = None;
                if g.found.take().is_some() {
                    g.phase = Phase::Idle;
                    self.forget_stored();
                }
            }
            Outcome::Available(f) => {
                state.last_result = "available".into();
                g.check_error = None;
                let same = g.found.as_ref().is_some_and(|k| k.tag == f.tag);
                if !same {
                    self.store_found(&f);
                    if !matches!(g.phase, Phase::Downloading(_)) {
                        g.phase = Phase::Idle;
                    }
                    g.error = None;
                }
                g.found = Some(*f);
            }
            Outcome::Failed(code, _) => {
                state.last_result = code.to_string();
                g.check_error = Some(code.to_string());
            }
        }
        g.state = state.clone();
        drop(g);
        self.save_state(&state);
    }

    /// The player went back to the stable channel: a beta release on offer (or downloaded) is dropped at once.
    pub fn channel_changed(&self, channel: Channel) {
        let mut g = self.lock();
        let beta = g.found.as_ref().is_some_and(|f| f.manifest.channel == Channel::Beta);
        if channel == Channel::Stable && beta && !matches!(g.phase, Phase::Downloading(_) | Phase::Applying | Phase::Restarting) {
            g.found = None;
            g.phase = Phase::Idle;
            g.error = None;
            drop(g);
            self.forget_stored();
        }
    }

    /// "Later" (the bar, the start dialog): quiet until the launcher is started again; not saved.
    pub fn later(&self) {
        let mut g = self.lock();
        if let Some(v) = g.found.as_ref().map(|f| f.manifest.version.to_string()) {
            g.later = Some(v);
        }
    }

    pub fn dismiss(&self) {
        let mut g = self.lock();
        g.notice = None;
        g.error = None;
        if matches!(g.phase, Phase::Saved(_)) {
            g.phase = Phase::Idle;
        }
    }

    pub fn set_notice(&self, n: Option<NoticeView>) {
        self.lock().notice = n;
    }

    /// Starts a download: `None` when there is nothing to download, one runs already, or another launcher holds the
    /// update lock.
    pub fn begin_download(&self) -> Option<DownloadJob> {
        let src = self.source.as_ref()?;
        let mut g = self.lock();
        if matches!(g.phase, Phase::Downloading(_) | Phase::Ready(_) | Phase::Applying | Phase::Restarting) {
            return None;
        }
        let f = g.found.clone()?;
        let entry = self.entry_for(&f)?;
        if !manifest::valid_file_name(&entry.name) || !check::valid_tag(&f.tag) {
            return None;
        }
        if g.lock.is_none() {
            match apply::try_lock(&self.dir) {
                Some(l) => g.lock = Some(l),
                None => {
                    g.error = Some("other_instance".into());
                    return None;
                }
            }
        }
        if let Some(free) = crate::installer::sys::free_bytes(&self.dir)
            && free < entry.size * 2 + 64 * 1024 * 1024
        {
            g.error = Some("disk".into());
            g.lock = None;
            return None;
        }
        self.cancel.store(false, Ordering::SeqCst);
        g.error = None;
        g.phase = Phase::Downloading(Progress { done: 0, total: entry.size });
        let file = self.file_path(&f.tag, &entry.name);
        let mut part = file.clone().into_os_string();
        part.push(".part");
        Some(DownloadJob { url: src.asset_url(&f.tag, &entry.name), part: PathBuf::from(part), file, entry })
    }

    /// Runs a download to its end: resumes up to 3 times after a cut, checks the signed size and SHA-256, and makes the
    /// update ready. `notify` is called on progress (at most 5 times a second) and at the end.
    pub async fn run_download(&self, job: DownloadJob, notify: &(dyn Fn() + Send + Sync)) {
        let Some(src) = &self.source else { return };
        let last = Mutex::new(std::time::Instant::now() - Duration::from_secs(1));
        let progress = |done: u64, total: u64| {
            self.lock().phase = Phase::Downloading(Progress { done, total });
            let mut l = last.lock().unwrap_or_else(|p| p.into_inner());
            if l.elapsed() >= Duration::from_millis(200) {
                *l = std::time::Instant::now();
                notify();
            }
        };
        let mut result = Err(net::NetError::Transport("not started".into()));
        for attempt in 0..4 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(2 * attempt)).await;
            }
            result = net::download(src, &job.url, &job.part, job.entry.size, &progress, &self.cancel).await;
            match &result {
                Err(net::NetError::Transport(_)) if !self.cancel.load(Ordering::SeqCst) => continue,
                _ => break,
            }
        }
        let outcome = match result {
            Ok(()) => match apply::verify_file(&job.part, &job.entry.sha256, job.entry.size) {
                Ok(()) => std::fs::rename(&job.part, &job.file).map_err(|_| "disk".to_string()),
                Err(_) => {
                    // the bytes are not the signed ones: never kept, never resumed
                    let _ = std::fs::remove_file(&job.part);
                    Err("integrity".to_string())
                }
            },
            Err(net::NetError::Cancelled) => Err(String::new()),
            Err(e) => Err(e.code().to_string()),
        };
        crate::trace::mark(&format!("update:download {}", outcome.as_ref().err().map(String::as_str).unwrap_or("ok")));
        {
            let mut g = self.lock();
            g.lock = None;
            match outcome {
                Ok(()) => {
                    g.phase = Phase::Ready(job.file);
                    g.error = None;
                }
                Err(code) => {
                    g.phase = Phase::Idle;
                    g.error = (!code.is_empty()).then_some(code);
                }
            }
        }
        notify();
    }

    pub fn cancel_download(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// The verified download and its entry, when the update is ready.
    pub fn ready(&self) -> Option<(PathBuf, FileEntry, String, Mode)> {
        let g = self.lock();
        let Phase::Ready(file) = &g.phase else { return None };
        let f = g.found.as_ref()?;
        let entry = self.entry_for(f)?;
        Some((file.clone(), entry, f.manifest.version.to_string(), self.mode_for(&f.manifest).0))
    }

    fn set_phase(&self, p: Phase, error: Option<&str>) {
        let mut g = self.lock();
        g.phase = p;
        g.error = error.map(str::to_string);
    }

    /// Takes the update lock for applying (`false`: another launcher holds it).
    fn take_lock(&self) -> bool {
        let mut g = self.lock();
        if g.lock.is_none() {
            g.lock = apply::try_lock(&self.dir);
        }
        g.lock.is_some()
    }

    /// The ready update of mode `want`, moved to "applying" in the same step: a second click finds nothing to do.
    fn take_ready(&self, want: Mode) -> Option<(PathBuf, FileEntry, String)> {
        let mut g = self.lock();
        let Phase::Ready(file) = &g.phase else { return None };
        let file = file.clone();
        let f = g.found.as_ref()?;
        if self.mode_for(&f.manifest).0 != want {
            return None;
        }
        let entry = self.entry_for(f)?;
        let to = f.manifest.version.to_string();
        g.phase = Phase::Applying;
        g.error = None;
        Some((file, entry, to))
    }

    /// Applying failed before anything changed: ready again, with the reason.
    fn back_to_ready(&self, file: PathBuf, e: apply::ApplyError) -> apply::ApplyError {
        let mut g = self.lock();
        g.phase = Phase::Ready(file);
        g.error = Some(e.code().into());
        g.lock = None;
        e
    }

    /// Swap mode, the part before the restart: checks, copy, the two renames, the new program started on trial
    /// (`spawn`: [`apply::spawn_trial`]). The caller hides its window and watches the returned process.
    pub fn apply_swap(&self, spawn: &dyn Fn(&Path) -> std::io::Result<std::process::Child>) -> Result<(std::process::Child, apply::Paths, String), apply::ApplyError> {
        let (file, entry, to) = self.take_ready(Mode::Swap).ok_or_else(|| apply::ApplyError::Io("nothing ready to swap".into()))?;
        let Some(exe) = self.exe.clone() else { return Err(self.back_to_ready(file, apply::ApplyError::Io("program path unknown".into()))) };
        if !apply::other_instances(&exe).is_empty() {
            return Err(self.back_to_ready(file, apply::ApplyError::Busy("another Kalamar runs from this file".into())));
        }
        if !self.take_lock() {
            return Err(self.back_to_ready(file, apply::ApplyError::Busy("another launcher holds the update lock".into())));
        }
        let p = apply::Paths::of(&exe);
        let run = || -> Result<std::process::Child, apply::ApplyError> {
            // The checked file is held from its check until the new program has started: nothing can write it, and the
            // file started is that one (`apply::lock_in_place`).
            let checked = apply::prepare(&file, &entry.sha256, entry.size, &p)?;
            apply::swap(&p, &self.dir, &self.current_text, &to)?;
            let _held = match apply::lock_in_place(&p, &checked) {
                Ok(h) => h,
                Err(e) => {
                    drop(checked);
                    let _ = apply::roll_back(&p, &self.dir, &self.current_text, &to, "the new program changed after its check");
                    return Err(e);
                }
            };
            drop(checked);
            let _ = std::fs::remove_file(apply::witness_path(&self.dir));
            match spawn(&p.exe) {
                Ok(c) => Ok(c),
                Err(e) => {
                    let _ = apply::roll_back(&p, &self.dir, &self.current_text, &to, "could not start");
                    Err(apply::ApplyError::Spawn(e.to_string()))
                }
            }
        };
        match run() {
            Ok(child) => {
                self.set_phase(Phase::Restarting, None);
                Ok((child, p, to))
            }
            Err(apply::ApplyError::Integrity) => {
                // the download changed on disk since it was checked: dropped, never used
                let _ = std::fs::remove_file(&file);
                let mut g = self.lock();
                g.phase = Phase::Idle;
                g.error = Some("integrity".into());
                g.lock = None;
                Err(apply::ApplyError::Integrity)
            }
            Err(e) => Err(self.back_to_ready(file, e)),
        }
    }

    /// The trial failed (the old process, after [`apply::roll_back`]): ready again, with the notice.
    pub fn after_rollback(&self, to: &str, restored: bool) {
        let mut g = self.lock();
        g.lock = None;
        if let Some(r) = self.ready_file(&g) {
            g.phase = Phase::Ready(r);
        } else {
            g.phase = Phase::Idle;
        }
        if restored {
            g.notice = Some(NoticeView { kind: "rolled_back", from: self.current_text.clone(), to: to.to_string() });
            g.error = None;
        } else {
            g.error = Some("rollback_failed".into());
        }
    }

    fn ready_file(&self, g: &Inner) -> Option<PathBuf> {
        let f = g.found.as_ref()?;
        let e = self.entry_for(f)?;
        let p = self.file_path(&f.tag, &e.name);
        p.is_file().then_some(p)
    }

    /// Installer mode: checks the installer again and starts it (it asks for elevation itself when it needs it). The
    /// caller quits right after.
    pub fn apply_installer(&self) -> Result<(), apply::ApplyError> {
        let (file, entry, to) = self.take_ready(Mode::Installer).ok_or_else(|| apply::ApplyError::Io("no installer ready".into()))?;
        if entry.kind != FileKind::Installer {
            return Err(self.back_to_ready(file, apply::ApplyError::Io("not an installer".into())));
        }
        // Checked through a handle that keeps the installer from being written, renamed or deleted until it has started:
        // what runs is what was checked.
        let r = apply::verify_locked(&file, &entry.sha256, entry.size, false).and_then(|held| {
            apply::save_journal(&self.dir, &apply::Journal::InstallerStarted { from: self.current_text.clone(), to: to.clone() })?;
            let started = apply::open_installer(&file).map_err(|e| {
                apply::clear_journal(&self.dir);
                apply::ApplyError::Spawn(e)
            });
            drop(held);
            started
        });
        match &r {
            Ok(()) => self.set_phase(Phase::Restarting, None),
            Err(apply::ApplyError::Integrity) => {
                let _ = std::fs::remove_file(&file);
                self.set_phase(Phase::Idle, Some("integrity"));
            }
            Err(e) => self.set_phase(Phase::Ready(file), Some(if matches!(e, apply::ApplyError::Spawn(_)) { "installer" } else { e.code() })),
        }
        self.lock().lock = None;
        r
    }

    /// Elsewhere mode: saves the new program in `dir`.
    pub fn save_to(&self, dir: &Path) -> Result<PathBuf, apply::ApplyError> {
        let (file, entry, to, _) = self.ready().ok_or_else(|| apply::ApplyError::Io("nothing ready".into()))?;
        match apply::save_elsewhere(&file, &entry.sha256, entry.size, dir, &to, &self.temp) {
            Ok(p) => {
                self.set_phase(Phase::Saved(p.clone()), None);
                Ok(p)
            }
            Err(e) => {
                if e == apply::ApplyError::Integrity {
                    let _ = std::fs::remove_file(&file);
                    self.set_phase(Phase::Idle, Some("integrity"));
                } else {
                    self.lock().error = Some(e.code().into());
                }
                Err(e)
            }
        }
    }

    pub fn saved_file(&self) -> Option<PathBuf> {
        match &self.lock().phase {
            Phase::Saved(p) => Some(p.clone()),
            _ => None,
        }
    }
}

// ---- the launcher's side: start-up, commands, events -----------------------------------------------------------------

use crate::commands::AppState;
use tauri::{AppHandle, Emitter, Manager, State};

/// The data folder before the window exists, the one `lib::data_dir` gives once Tauri runs ([`crate::datadir`]:
/// `KALAMAR_DATA`, else `%APPDATA%\<identifier>`). While an update from a launcher of the earlier identifier is on
/// trial, it is that launcher's folder: the journal and the witness are there, where the old process waits.
pub fn early_data_dir() -> Option<PathBuf> {
    Some(crate::datadir::current().dir.clone())
}

/// First thing at start: finishes or undoes an interrupted update. `false`: the previous version was put back and
/// started, this process must quit now.
pub fn early_start() -> bool {
    if channel::source().is_none() {
        return true;
    }
    let (Some(data), Ok(exe)) = (early_data_dir(), std::env::current_exe()) else { return true };
    let trial = std::env::args().skip(1).any(|a| a == apply::TRIAL_ARG);
    let relaunch = |p: &Path| std::process::Command::new(p).current_dir(p.parent().unwrap_or(Path::new("."))).spawn().map(|_| ());
    apply::early_start(&data.join("update"), &exe, env!("CARGO_PKG_VERSION"), trial, &relaunch) == apply::Early::Continue
}

fn prefs(app: &AppHandle) -> UpdatePrefs {
    app.state::<AppState>().config.lock().unwrap_or_else(|p| p.into_inner()).update.clone()
}

/// Why applying must wait: a game, an installation or a relay this launcher runs.
fn blocked(app: &AppHandle) -> Option<&'static str> {
    use crate::launch::RunState;
    let st = app.state::<AppState>();
    let (g1, g2) = (st.game.status(), st.game2.status());
    if g1.state == RunState::Running || g1.restarted || g2.state == RunState::Running {
        Some("game")
    } else if st.installer.active() {
        Some("install")
    } else if st.relay.status(None, Path::new("")).running {
        Some("relay")
    } else {
        None
    }
}

fn view(app: &AppHandle) -> UpdateView {
    let upd = app.state::<Updater>();
    upd.view(&prefs(app), blocked(app))
}

fn emit(app: &AppHandle) {
    let _ = app.emit("update-state", view(app));
}

/// The player changed the automatic check or the channel (Settings > About).
pub fn prefs_changed(app: &AppHandle, prefs: &UpdatePrefs) {
    app.state::<Updater>().channel_changed(prefs.channel);
    emit(app);
}

/// Built in `setup`: reads `<data>\update\`, no network.
pub fn updater(data_dir: &Path, prefs: &UpdatePrefs) -> Updater {
    Updater::new(channel::source(), data_dir, env!("CARGO_PKG_VERSION"), std::env::current_exe().ok(), apply::temp_roots(), prefs.channel)
}

/// After `setup`: removes leftovers of earlier updates and schedules the automatic checks (20 s after start, then every
/// 6 hours while the launcher stays open). Each round reads the player's switch again; a check at every start costs one
/// conditional request, answered 304 when nothing changed.
pub fn start(app: &AppHandle) {
    let upd = app.state::<Updater>();
    if !upd.enabled() {
        return;
    }
    if let Some(exe) = upd.exe().map(Path::to_path_buf) {
        let dir = upd.dir().to_path_buf();
        std::thread::spawn(move || apply::cleanup_leftovers(&dir, &exe));
    }
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut first = true;
        loop {
            tokio::time::sleep(auto_wait(first)).await;
            first = false;
            let p = prefs(&app2);
            if p.auto_check {
                let upd = app2.state::<Updater>();
                let app3 = app2.clone();
                upd.check(p.channel, false, &move || emit(&app3)).await;
                emit(&app2);
            }
        }
    });
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> UpdateView {
    view(&app)
}

/// Called by the page once it is up: the witness of a new version's first start (the old process then quits).
#[tauri::command]
pub fn update_confirm_start(app: AppHandle, upd: State<'_, Updater>) -> UpdateView {
    if upd.enabled()
        && let Some(exe) = upd.exe().map(Path::to_path_buf)
        && let Some((from, to)) = apply::confirm(upd.dir(), &exe, upd.current(), std::process::id())
    {
        crate::trace::mark("update:confirmed");
        upd.set_notice(Some(NoticeView { kind: "updated", from, to }));
        let dir = upd.dir().to_path_buf();
        let version = upd.current().to_string();
        std::thread::spawn(move || apply::finish_confirmed(&dir, &exe, &version, Duration::from_secs(120)));
        emit(&app);
    }
    view(&app)
}

/// "Check now".
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<UpdateView, String> {
    let p = prefs(&app);
    let upd = app.state::<Updater>();
    let app2 = app.clone();
    upd.check(p.channel, true, &move || emit(&app2)).await;
    emit(&app);
    Ok(view(&app))
}

#[tauri::command]
pub fn update_download(app: AppHandle, upd: State<'_, Updater>) -> UpdateView {
    if let Some(job) = upd.begin_download() {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let upd = app2.state::<Updater>();
            let app3 = app2.clone();
            upd.run_download(job, &move || emit(&app3)).await;
        });
    }
    view(&app)
}

#[tauri::command]
pub fn update_cancel(app: AppHandle, upd: State<'_, Updater>) -> UpdateView {
    upd.cancel_download();
    view(&app)
}

#[tauri::command]
pub fn update_later(app: AppHandle, upd: State<'_, Updater>) -> UpdateView {
    upd.later();
    view(&app)
}

#[tauri::command]
pub fn update_dismiss(app: AppHandle, upd: State<'_, Updater>) -> UpdateView {
    upd.dismiss();
    view(&app)
}

/// "Restart to finish" (swap), "Start the installation" (installer). Refused while a game, an installation or a relay
/// runs.
#[tauri::command(async)]
pub fn update_apply(app: AppHandle) -> UpdateView {
    let upd = app.state::<Updater>();
    if !upd.enabled() {
        return view(&app);
    }
    if let Some(b) = blocked(&app) {
        upd.lock().error = Some(format!("blocked_{b}"));
        return view(&app);
    }
    let Some((_, _, _, mode)) = upd.ready() else { return view(&app) };
    match mode {
        // `apply_swap` / `apply_installer` take the ready update atomically: a double click starts nothing twice.
        Mode::Swap => match upd.apply_swap(&apply::spawn_trial) {
            Ok((mut child, paths, to)) => {
                emit(&app);
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
                let app2 = app.clone();
                let from = upd.current().to_string();
                let dir = upd.dir().to_path_buf();
                std::thread::spawn(move || {
                    let outcome = apply::watch(&mut child, &dir, &to, apply::TRIAL_TIMEOUT, Duration::from_millis(250));
                    crate::trace::mark(&format!("update:trial {}", outcome.reason()));
                    if outcome == apply::TrialOutcome::Started {
                        app2.exit(0);
                        return;
                    }
                    let restored = apply::roll_back(&paths, &dir, &from, &to, &outcome.reason()).is_ok();
                    app2.state::<Updater>().after_rollback(&to, restored);
                    crate::shell::bring_back(&app2);
                    emit(&app2);
                });
            }
            Err(e) => crate::trace::mark(&format!("update:apply {e}")),
        },
        Mode::Installer => {
            if upd.apply_installer().is_ok() {
                emit(&app);
                let app2 = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(400));
                    app2.exit(0);
                });
            }
        }
        Mode::Elsewhere => {}
    }
    view(&app)
}

/// Elsewhere mode: the folder the player picked in the open-folder dialog.
#[tauri::command(async)]
pub fn update_save_to(app: AppHandle, dir: String) -> UpdateView {
    let upd = app.state::<Updater>();
    let d = dir.trim().trim_matches('"');
    if upd.enabled() && !d.is_empty() && d.len() <= 1024 && !d.contains('\0') {
        let _ = upd.save_to(Path::new(d));
    }
    view(&app)
}

#[tauri::command]
pub fn update_show_saved(upd: State<'_, Updater>) {
    if let Some(p) = upd.saved_file() {
        apply::show_in_folder(&p);
    }
}
