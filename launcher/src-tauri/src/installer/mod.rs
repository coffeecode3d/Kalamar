//! The install wizard's back end: the whole install pipeline as a resumable state machine.
//!
//! Layout of an install folder `<I>` (the one pre-release installs already use, so the launcher and the runtime find everything):
//!
//! ```text
//! <I>\kalamar-install.json        state: which steps finished, with which runtime source; marks a wizard install
//! <I>\logs\<step>.log             one log per command (stdout + stderr), plus install-<time>.log
//! <I>\src\                        the runtime source shipped with the launcher (+ kit-tools\)
//! <I>\tools\                      Python, llvm-mingw, Ninja, the libraries, hactool (`portable`, `prebuilt`)
//! <I>\work\xlate\totk_xlate.exe   the shader translator (shipped with the launcher)
//! <I>\work\splatoon3-11.3.0\      the game root the launcher uses: exefs\, romfs\, build\splat3.exe, userdata\
//! ```
//!
//! A first install puts nothing outside that folder: no system tool, no administrator prompt, no winget. Installs made
//! before (a pre-release installer's, or this wizard's with MSYS2 and a Python virtual environment in `work\venv`) keep the tools
//! they were built with while those are complete, so an update does not recompile everything for a new compiler.
//!
//! Every step checks its own outputs first and is skipped when they are there (and, for the steps that depend on the
//! runtime source, when they were made from the same source commit), so running the wizard again resumes where it
//! stopped, after a cancel, a failure, a closed launcher or a reboot. Two steps run beside the others: the toolchain
//! download from the start until the compile needs it, and the RomFS merge during the lift (which reads only the
//! ExeFS). The player's three files are only read by the extraction tools; the merge tool's temporary folder (a key
//! file derived from prod.keys, and the NCA copies when hactool cannot read them in place) is deleted by the wizard
//! whenever that step stops, normally or not.

pub mod bundle;
pub mod gamever;
pub mod incremental;
pub mod nsp;
pub mod portable;
pub mod prebuilt;
pub mod prereq;
pub mod progress;
pub mod space;
pub mod sys;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const GAME_DIR: &str = gamever::CURRENT.game_dir;
pub const TARGET: &str = gamever::CURRENT.target;
/// Executable suffix: `.exe` on Windows, nothing on Linux (`build_game.py` names the target's executable
/// `splat3` + `.exe` only on Windows).
pub const EXE_SUFFIX: &str = if cfg!(windows) { ".exe" } else { "" };
pub const GAME_STEM: &str = "splat3";
pub const XLATE_STEM: &str = "totk_xlate";
pub const HACTOOL_STEM: &str = "hactool";

/// A program's file name with the platform's suffix (`splat3` → `splat3.exe` on Windows, `splat3` on Linux).
pub fn exe_name(stem: &str) -> String {
    format!("{stem}{EXE_SUFFIX}")
}
pub const STATE_FILE: &str = "kalamar-install.json";
pub const MNG_URL: &str = "https://github.com/ly05010419/MacNativeGames.git";
pub const MNG_PIN: &str = "d1509b4b62211265fad5449cf53cc9f13484ecc5";
/// hactool's source and commit are pinned by the runtime's `tools/build_hactool.py`; repeated here for the screen
/// that lists the downloads.
pub const HACTOOL_URL: &str = "https://github.com/SciresM/hactool.git";
pub const HACTOOL_PIN: &str = "1d64a83450e025622f3468c28fc4164dad2c5ef6";
/// The packages of the old Python virtual environment (installs made before the portable Python keep it).
pub const PIP_PACKAGES: [&str; 5] = ["cryptography==50.0.2", "numpy==2.5.4", "lz4==4.4.5", "capstone==5.0.9", "unicorn==2.1.4"];
/// What deleting the generated C and the objects after the build saves (the next update then rebuilds everything).
pub const BUILD_FILES_ESTIMATE: u64 = 2_000_000_000;
const NUGET_CONFIG: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!-- Only nuget.org, for the restore of the shader translator. -->\n<configuration>\n  <packageSources>\n    <clear />\n    <add key=\"nuget.org\" value=\"https://api.nuget.org/v3/index.json\" protocolVersion=\"3\" />\n  </packageSources>\n</configuration>\n";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StepId {
    Checks,
    Prereqs,
    Sources,
    Python,
    Exefs,
    Hactool,
    Romfs,
    Lift,
    Compile,
    Xlate,
    Keys,
    Layout,
    Selftest,
    Cleanup,
}

impl StepId {
    pub const ALL: [StepId; 14] = [
        StepId::Checks,
        StepId::Prereqs,
        StepId::Sources,
        StepId::Python,
        StepId::Exefs,
        StepId::Hactool,
        StepId::Romfs,
        StepId::Lift,
        StepId::Compile,
        StepId::Xlate,
        StepId::Keys,
        StepId::Layout,
        StepId::Selftest,
        StepId::Cleanup,
    ];
    pub fn name(self) -> &'static str {
        match self {
            StepId::Checks => "checks",
            StepId::Prereqs => "prereqs",
            StepId::Sources => "sources",
            StepId::Python => "python",
            StepId::Exefs => "exefs",
            StepId::Hactool => "hactool",
            StepId::Romfs => "romfs",
            StepId::Lift => "lift",
            StepId::Compile => "compile",
            StepId::Xlate => "xlate",
            StepId::Keys => "keys",
            StepId::Layout => "layout",
            StepId::Selftest => "selftest",
            StepId::Cleanup => "cleanup",
        }
    }
    /// Steps whose result depends on the runtime source (redone when the launcher ships another one).
    pub fn source_bound(self) -> bool {
        matches!(self, StepId::Lift | StepId::Compile | StepId::Xlate | StepId::Keys)
    }
    /// Steps that run every time (quick checks and verifications), never shown as "already done".
    pub fn always_runs(self) -> bool {
        matches!(self, StepId::Checks | StepId::Layout | StepId::Selftest | StepId::Cleanup)
    }
}

/// The compiler an install builds with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Toolchain {
    /// llvm-mingw, Ninja and the libraries in `<I>\tools` (`portable`): what a first install uses.
    Portable,
    /// MSYS2 UCRT64 in `C:\msys64`: kept by an install whose objects were compiled with it, while it is complete.
    Msys2,
    /// The system's clang, lld, Ninja and libraries (`clang`, `ld.lld`, `ninja`, SDL3, Opus, libvpx, zstd): Linux.
    System,
}

impl Toolchain {
    /// What the state file records for a compile made with it.
    pub fn id(self) -> String {
        match self {
            Toolchain::Portable => portable::toolchain_id(),
            Toolchain::Msys2 => "msys2".into(),
            Toolchain::System => "system".into(),
        }
    }
}

/// Paths of an install folder.
#[derive(Debug, Clone)]
pub struct Layout {
    pub root: PathBuf,
}

impl Layout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Layout { root: root.into() }
    }
    pub fn src(&self) -> PathBuf {
        self.root.join("src")
    }
    pub fn work(&self) -> PathBuf {
        self.root.join("work")
    }
    pub fn game(&self) -> PathBuf {
        self.work().join(GAME_DIR)
    }
    /// The game folder of the version before this one (`gamever::PREVIOUS`), when the install still has it.
    pub fn previous_game(&self) -> Option<PathBuf> {
        gamever::previous_game(&self.root)
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn state(&self) -> PathBuf {
        self.root.join(STATE_FILE)
    }
    pub fn venv(&self) -> PathBuf {
        self.work().join("venv")
    }
    /// The Python of an older install's virtual environment (`work\venv\Scripts\python.exe`, `work/venv/bin/python3`).
    pub fn venv_py(&self) -> PathBuf {
        #[cfg(windows)]
        {
            self.venv().join("Scripts").join("python.exe")
        }
        #[cfg(not(windows))]
        {
            self.venv().join("bin").join("python3")
        }
    }
    /// The Python the steps run: the portable one in `tools\python` when it is there, else an older install's virtual
    /// environment.
    pub fn python(&self) -> PathBuf {
        let p = portable::python_exe(&self.root);
        if p.is_file() || !self.venv_py().is_file() { p } else { self.venv_py() }
    }
    pub fn tools(&self) -> PathBuf {
        portable::tools(&self.root)
    }
    /// hactool, built (`build_hactool.py`) or shipped with the launcher (`prebuilt`), in `tools/bin`.
    pub fn hactool(&self) -> PathBuf {
        portable::bin_dir(&self.root).join(exe_name(HACTOOL_STEM))
    }
    /// hactool as installs before the shipped one built it (`tools/build_hactool.py`).
    pub fn hactool_built(&self) -> PathBuf {
        self.src().join("toolchain").join("hactool").join(exe_name(HACTOOL_STEM))
    }
    pub fn kit_tools(&self) -> PathBuf {
        self.src().join("kit-tools")
    }
    pub fn vendor(&self) -> PathBuf {
        self.src().join("tools").join("vendor").join("macnativegames")
    }
    pub fn exefs(&self) -> PathBuf {
        self.game().join("exefs")
    }
    pub fn romfs(&self) -> PathBuf {
        self.game().join("romfs")
    }
    pub fn lift(&self) -> PathBuf {
        self.game().join("lift")
    }
    pub fn build(&self) -> PathBuf {
        self.game().join("build")
    }
    pub fn game_exe(&self) -> PathBuf {
        self.build().join(exe_name(GAME_STEM))
    }
    pub fn userdata(&self) -> PathBuf {
        self.game().join("userdata")
    }
    pub fn xlate(&self) -> PathBuf {
        self.work().join("xlate")
    }
    pub fn xlate_exe(&self) -> PathBuf {
        self.xlate().join(exe_name(XLATE_STEM))
    }
    pub fn nuget(&self) -> PathBuf {
        self.work().join("nuget")
    }
    pub fn mng(&self) -> PathBuf {
        self.src().join("refs").join("MacNativeGames")
    }
    pub fn keys_json(&self) -> PathBuf {
        self.kit_tools().join("profile").join("s3save.keys.json")
    }
    pub fn romfs_done(&self) -> bool {
        self.romfs().join("romfs.bin").is_file() && self.romfs().join("index.json").is_file()
    }
    /// The generated C is complete: the layout, the target, and the manifest of every module the target lifts (the
    /// generator writes each manifest last, so a lift that stopped half-way has none).
    pub fn lift_outputs(&self) -> bool {
        let l = self.lift();
        if !l.join("layout.json").is_file() {
            return false;
        }
        let Some(target) = std::fs::read_to_string(l.join("target.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else {
            return false;
        };
        let mods: Vec<&str> = target.get("modules").and_then(|m| m.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
        !mods.is_empty() && mods.iter().all(|m| l.join(m).join("manifest.json").is_file())
    }
    /// The objects of the last build and Ninja's records of them are there (an update recompiles only what changed).
    pub fn objects_kept(&self) -> bool {
        let b = self.build();
        b.join("obj").is_dir() && b.join(".ninja_log").is_file() && b.join(".ninja_deps").is_file()
    }
    /// Identity of the RomFS index the compile reads (size and date of `romfs\index.json`): a merge made again rebuilds.
    pub fn romfs_identity(&self) -> String {
        std::fs::metadata(self.romfs().join("index.json"))
            .map(|m| format!("{}:{}", m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())))
            .unwrap_or_default()
    }
    /// Temporary folders of the merge tool (`update-merge-*` next to the RomFS folder).
    pub fn merge_temps(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.game())
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("update-merge-"))).collect())
            .unwrap_or_default()
    }
    /// Intermediates that are not needed once the game is built (deleted as soon as possible).
    pub fn intermediates(&self) -> Vec<PathBuf> {
        vec![
            self.lift(),
            self.build().join("obj"),
            self.mng(),
            self.nuget(),
            self.xlate().join("ryujinx"),
            self.xlate().join("xlate"),
            self.xlate().join("publish"),
            self.src().join("toolchain").join("src"),
            self.userdata().join("selfcheck"),
            portable::downloads(&self.root),
        ]
    }
    /// The compiler the objects in `build\` were compiled with, from the `cc` line of Ninja's file (`None` without one).
    pub fn built_with(&self) -> Option<Toolchain> {
        let text = std::fs::read_to_string(self.build().join("build.ninja")).ok()?;
        let cc = text.lines().find(|l| l.starts_with("cc = "))?.to_ascii_lowercase().replace('\\', "/");
        let mine = portable::llvm_dir(&self.root).display().to_string().to_ascii_lowercase().replace('\\', "/");
        if cc.contains(&mine) {
            Some(Toolchain::Portable)
        } else if cc.contains("msys64") {
            Some(Toolchain::Msys2)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct InstallRequest {
    pub install_dir: String,
    pub base_nsp: String,
    pub update_nsp: String,
    pub prod_keys: String,
    /// Parallel compile jobs (0 = the wizard's default for this PC).
    pub jobs: u32,
    /// The player confirmed the list of tools to install (asked only when some are missing).
    pub allow_prereqs: bool,
    /// Rebuild the game from scratch (generated C and every object) even when it was built from the same source.
    pub force_rebuild: bool,
    /// Delete the generated C and the objects after the build (about 2 GB). They are kept otherwise, so a later update
    /// only recompiles what changed; without them the next update rebuilds everything.
    pub free_build_files: bool,
    /// Run the build tools below normal priority (off by default: normal priority, faster).
    pub low_priority: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct StepRecord {
    /// `done`, `failed` or `cancelled`.
    pub status: String,
    pub at: u64,
    pub secs: f64,
    /// Runtime source commit the step ran with.
    pub commit: String,
    /// Seconds of the last run that did the step's work (kept while it is skipped): this PC's duration for estimates.
    pub worked_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct StateFile {
    pub schema: u32,
    pub tool: String,
    /// The engine target (`TARGET`) the game steps of this state were done for; empty in a state written before the
    /// game version was recorded (those installs are for the previous version: `gamever::PREVIOUS`).
    pub target: String,
    pub created: u64,
    pub updated: u64,
    pub source_commit: String,
    /// Paths of the player's files (paths only; the files themselves stay where they are).
    pub base_nsp: String,
    pub update_nsp: String,
    pub prod_keys: String,
    pub jobs: u32,
    pub steps: BTreeMap<String, StepRecord>,
    pub finished: bool,
    pub needs_reboot: bool,
    pub last_error: Option<InstallError>,
    pub footprint: u64,
    /// Digest of the code-generator sources the lifted C was made with (`bundle::LIFT_INPUTS`).
    pub lift_inputs: String,
    /// Digest of the sources the shader translator was built with (`bundle::XLATE_INPUTS`).
    pub xlate_inputs: String,
    /// Digest of everything the game executable in place was compiled from (`incremental::compile_key`); empty while
    /// a compile runs, after a failed one and after a rollback.
    pub compile_inputs: String,
    /// Digest of the script the save keys were read with (`bundle::KEYS_INPUTS`).
    pub keys_inputs: String,
    /// Runtime source of the game executable kept in `build-previous` for `Revert to the previous version`.
    pub previous_commit: String,
    /// The compiler of the last compile (`Toolchain::id`): `msys2`, or the portable toolchain's pinned identity.
    pub toolchain: String,
    /// The folder was made by the pre-release installer and taken over (its old launcher's data folder is `launcher-data`).
    pub adopted_from_kit: bool,
    /// The step that was running (saved when it starts; cleared when the run finishes): after a closed launcher, a
    /// crash or a reboot, the next start says where the installation stopped.
    pub current: Option<String>,
}

impl StateFile {
    pub fn load(path: &Path) -> Option<StateFile> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        crate::util::atomic_write(path, text.as_bytes()).map_err(|e| std::io::Error::other(e.to_string()))
    }
    /// Puts a state made for another game version right: the steps that read or build the game (ExeFS, RomFS,
    /// generated C, executable, save keys) are forgotten, with their input digests, so none is skipped on the strength
    /// of the old version's results; the tools (downloads, Python, hactool, the shader translator) stay. Returns
    /// whether anything changed. A state with no steps (a first install) only gets the target.
    pub fn retarget(&mut self, target: &str) -> bool {
        if self.target == target {
            return false;
        }
        let had_game = !self.steps.is_empty();
        self.target = target.to_string();
        if !had_game {
            return false;
        }
        for id in [StepId::Exefs, StepId::Romfs, StepId::Lift, StepId::Compile, StepId::Keys, StepId::Layout, StepId::Selftest, StepId::Cleanup] {
            self.steps.remove(id.name());
        }
        self.lift_inputs.clear();
        self.compile_inputs.clear();
        self.keys_inputs.clear();
        self.previous_commit.clear();
        self.finished = false;
        self.last_error = None;
        self.current = None;
        true
    }
    pub fn done_with(&self, step: StepId, commit: &str) -> bool {
        self.steps.get(step.name()).is_some_and(|r| r.status == "done" && (!step.source_bound() || r.commit == commit))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstallError {
    pub step: StepId,
    /// Stable code the interface turns into a sentence and advice.
    pub code: String,
    pub params: Vec<(String, String)>,
    pub log: Option<String>,
    pub tail: Vec<String>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StepView {
    pub id: StepId,
    /// `pending`, `running`, `done`, `skipped`, `failed`, `cancelled`.
    pub status: &'static str,
    pub secs: Option<f64>,
    pub reference: f64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Snapshot {
    pub active: bool,
    /// `done`, `failed`, `cancelled`, `reboot` once the run ended.
    pub outcome: Option<&'static str>,
    pub install_dir: Option<String>,
    pub game_root: Option<String>,
    pub steps: Vec<StepView>,
    pub current: Option<StepId>,
    pub overall: f64,
    pub step_fraction: Option<f64>,
    pub elapsed: f64,
    pub eta: Option<f64>,
    pub step_elapsed: f64,
    pub step_eta: Option<f64>,
    /// Phase of the current step (`copy`, `hactool`, `emit`, `link`...), translated by the interface.
    pub stage: Option<String>,
    /// Count shown next to the phase (`312/733`, `12.3/22.0 GB`).
    pub detail: Option<String>,
    /// Why the run waits (`game_running`).
    pub waiting: Option<String>,
    pub tail: Vec<String>,
    pub log: Option<String>,
    pub logs_dir: Option<String>,
    pub error: Option<InstallError>,
    pub footprint: Option<u64>,
    pub cancel_requested: bool,
    pub warnings: Vec<String>,
    pub source_commit: Option<String>,
    /// Steps running beside the current one (the toolchain download, the RomFS merge).
    pub background: Vec<BgView>,
    /// The run began with a game executable in place (an update): that version stays playable until the link replaces
    /// it, and the link waits for it to be closed.
    pub update: bool,
    /// The merged game data (the RomFS image and its index) is complete: the Locker reads its names, icons and colors
    /// from it, long before the compile ends.
    pub game_data: bool,
    /// Seconds until then, while it is not (`None` once it is, or when no estimate is known).
    pub game_data_eta: Option<f64>,
    /// The game's executable or its translator is being replaced right now: a launch of that game waits a few seconds.
    pub replacing: bool,
}

/// A step running beside the current one, for the progress screen.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BgView {
    pub id: StepId,
    pub fraction: Option<f64>,
    pub stage: Option<String>,
    pub detail: Option<String>,
    pub elapsed: f64,
}

impl Snapshot {
    pub fn idle() -> Self {
        Snapshot {
            active: false,
            outcome: None,
            install_dir: None,
            game_root: None,
            steps: StepId::ALL.iter().map(|&id| StepView { id, status: "pending", secs: None, reference: progress::reference_secs(id, 8), note: None }).collect(),
            current: None,
            overall: 0.0,
            step_fraction: None,
            elapsed: 0.0,
            eta: None,
            step_elapsed: 0.0,
            step_eta: None,
            stage: None,
            detail: None,
            waiting: None,
            tail: Vec::new(),
            log: None,
            logs_dir: None,
            error: None,
            footprint: None,
            cancel_requested: false,
            warnings: Vec::new(),
            source_commit: None,
            background: Vec::new(),
            update: false,
            game_data: false,
            game_data_eta: None,
            replacing: false,
        }
    }
}

pub type Emit = Arc<dyn Fn(&Snapshot) + Send + Sync>;
/// Called once the game root is in place (the launcher then points its configuration at it).
pub type OnRoot = Arc<dyn Fn(&Path) + Send + Sync>;

struct Inner {
    snap: Snapshot,
    job: Option<Arc<sys::Job>>,
    /// The process trees of the steps running beside the current one (killed by a cancel too).
    bg_jobs: Vec<Arc<sys::Job>>,
    halts: Vec<Arc<AtomicBool>>,
    last_emit: Option<Instant>,
}

/// The wizard's runner, shared by the IPC commands. One install at a time.
#[derive(Clone)]
pub struct Installer {
    inner: Arc<Mutex<Inner>>,
    cancel: Arc<AtomicBool>,
}

impl Default for Installer {
    fn default() -> Self {
        Installer { inner: Arc::new(Mutex::new(Inner { snap: Snapshot::idle(), job: None, bg_jobs: Vec::new(), halts: Vec::new(), last_emit: None })), cancel: Arc::new(AtomicBool::new(false)) }
    }
}

/// How a step ended without error.
#[derive(Debug, PartialEq, Eq)]
enum Flow {
    Done,
    Skipped,
    /// The step goes on in a thread beside the next ones ([`Background`]); its result is taken before the step that
    /// needs it.
    Background,
}

/// The work of a step run in a thread beside the others: it gets the stop flag and the step's progress.
#[cfg(windows)]
type BgWork = Box<dyn FnOnce(&AtomicBool, &Mutex<Prog>) -> StepResult + Send>;

/// A step running beside the others.
struct Background {
    id: StepId,
    /// The step that needs its result: it waits for it before starting.
    before: StepId,
    started: Instant,
    reference: f64,
    /// The step's result and how long it worked (not how long until it was waited for).
    handle: Option<std::thread::JoinHandle<(StepResult, f64)>>,
    prog: Arc<Mutex<Prog>>,
    /// Set to stop it (a cancel, another step's failure).
    halt: Arc<AtomicBool>,
    /// Its result, once taken from the thread and not yet recorded.
    done: Option<(StepResult, f64)>,
}

#[derive(Debug)]
struct Fail {
    code: String,
    params: Vec<(String, String)>,
    exit_code: Option<i32>,
    log: Option<PathBuf>,
    cancelled: bool,
    reboot: bool,
}

impl Fail {
    fn new(code: &str) -> Fail {
        Fail { code: code.into(), params: Vec::new(), exit_code: None, log: None, cancelled: false, reboot: false }
    }
    fn with(mut self, k: &str, v: impl ToString) -> Fail {
        self.params.push((k.into(), v.to_string()));
        self
    }
    fn cancelled() -> Fail {
        Fail { cancelled: true, ..Fail::new("cancelled") }
    }
    fn io(what: &str, e: impl std::fmt::Display) -> Fail {
        Fail::new("io").with("what", what).with("message", e)
    }
}

type StepResult = Result<Flow, Fail>;

impl Installer {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn snapshot(&self) -> Snapshot {
        self.lock().snap.clone()
    }

    pub fn active(&self) -> bool {
        self.lock().snap.active
    }

    /// The game folder of the run started in this session (running or ended): the folder an installation builds into,
    /// which becomes the launcher's game root at its layout step.
    pub fn target_root(&self) -> Option<PathBuf> {
        let g = self.lock();
        g.snap.install_dir.as_ref().and(g.snap.game_root.as_ref()).map(PathBuf::from)
    }

    /// That folder, once its merged game data is complete (`Snapshot::game_data`): from then on the Locker reads names,
    /// icons and colors from it, whatever the compile is doing.
    pub fn game_data_root(&self) -> Option<PathBuf> {
        let g = self.lock();
        if !g.snap.game_data {
            return None;
        }
        g.snap.install_dir.as_ref().and(g.snap.game_root.as_ref()).map(PathBuf::from)
    }

    /// The executable (or the translator) of the game in `root` is being replaced at this moment.
    pub fn replacing(&self, root: &Path) -> bool {
        let g = self.lock();
        g.snap.active
            && g.snap.replacing
            && g.snap.game_root.as_deref().is_some_and(|r| r.trim_end_matches(['\\', '/']).eq_ignore_ascii_case(root.display().to_string().trim_end_matches(['\\', '/'])))
    }

    /// Asks the run to stop: the current command's process tree is killed (except a prerequisite installer, which is
    /// let finish), and the run ends with outcome `cancelled`. Everything finished stays, so a new run resumes.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        let mut g = self.lock();
        g.snap.cancel_requested = true;
        if let Some(j) = &g.job {
            j.terminate();
        }
        for j in &g.bg_jobs {
            j.terminate();
        }
        for h in &g.halts {
            h.store(true, Ordering::SeqCst);
        }
    }

    /// Starts a run in a background thread. Refused while one is active.
    pub fn start(&self, req: InstallRequest, bundle_dir: Option<PathBuf>, emit: Emit, on_root: OnRoot) -> Result<Snapshot, String> {
        {
            let mut g = self.lock();
            if g.snap.active {
                return Err("an installation is already running".into());
            }
            let mut s = Snapshot::idle();
            s.active = true;
            s.install_dir = Some(req.install_dir.clone());
            let lay = Layout::new(&req.install_dir);
            s.game_root = Some(lay.game().display().to_string());
            s.logs_dir = Some(lay.logs().display().to_string());
            g.snap = s;
            g.job = None;
            g.bg_jobs.clear();
            g.halts.clear();
        }
        self.cancel.store(false, Ordering::SeqCst);
        let me = self.clone();
        let snap = self.snapshot();
        std::thread::spawn(move || {
            let mut run = Run::new(me.clone(), req, bundle_dir, emit, on_root);
            run.execute();
        });
        Ok(snap)
    }
}

/// One run of the pipeline.
struct Run {
    inst: Installer,
    req: InstallRequest,
    lay: Layout,
    bundle_dir: Option<PathBuf>,
    manifest: Option<bundle::Manifest>,
    emit: Emit,
    on_root: OnRoot,
    state: StateFile,
    tools: Option<prereq::Detection>,
    jobs: u32,
    started: Instant,
    /// Reference weight of every step this run expects to execute (0 for the ones predicted to be skipped).
    weights: BTreeMap<StepId, f64>,
    /// (actual, reference) of the steps finished in this run.
    finished: Vec<(f64, f64)>,
    step_started: Instant,
    main_log: Option<std::fs::File>,
    tail: VecDeque<String>,
    nsp_sizes: (u64, u64),
    update_known: bool,
    /// Digests of the lift, translator and keys inputs in the bundled source (`bundle::LIFT_INPUTS`, `XLATE_INPUTS`,
    /// `KEYS_INPUTS`).
    lift_hash: String,
    xlate_hash: String,
    keys_hash: String,
    /// SHA-256 of every file of the bundled source, by path (empty without a bundle).
    archive: BTreeMap<String, String>,
    /// The bundled `tools/build_game.py` (its compile flags tell whether every object recompiles).
    build_game_py: Option<String>,
    /// What this run does to the generated C and the objects, decided before the first step.
    preview: Option<incremental::UpdatePreview>,
    /// Keep the generated C and objects after the build.
    keep_build: bool,
    /// This run replaced the game executable (the previous one is in `build-previous`) or moved the translator aside:
    /// put back if it fails.
    swapped: bool,
    xlate_swapped: bool,
    /// The compiler this run builds with (decided by the prerequisites step).
    toolchain: Toolchain,
    /// hactool and the translator as the launcher ships them.
    shipped: Option<prebuilt::Tools>,
    /// Steps running beside the current one.
    bg: Vec<Background>,
    /// A step running beside the current one failed: the current one stops and the failure is reported.
    bg_failed: Arc<AtomicBool>,
    /// How the run stops after a step running beside the others failed while a step waited for it in its own work.
    pending_stop: Option<(&'static str, Option<InstallError>)>,
    /// The build tools of the current command run below normal priority.
    low_priority_now: bool,
    /// The merged game data is complete (`Snapshot::game_data`).
    game_data: bool,
}

/// Default parallel compile jobs: every logical CPU (the extra jobs run on the second thread of each core; the compile
/// then runs below normal priority, so the PC stays usable), at most one per 1.25 GiB of RAM beyond 4 GiB (a compile
/// takes up to 0.7 GiB, measured).
pub fn default_jobs() -> u32 {
    jobs_for(sys::cpus(), sys::total_ram().unwrap_or(16 << 30))
}

pub fn jobs_for(cpus: usize, ram: u64) -> u32 {
    let by_ram = ram.saturating_sub(4 << 30) / (5 << 28);
    (cpus.max(1) as u64).min(by_ram.max(1)).min(64) as u32
}

fn now_stamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

/// The SHA-256 of `main` that `recomp/targets.py` expects for the target (the script's version check).
pub fn expected_main_sha(targets_py: &str, target: &str) -> Option<String> {
    let at = targets_py.find(&format!("'{target}'"))?;
    let rest = &targets_py[at..];
    let m = rest.find("'main':")? + "'main':".len();
    let rest = rest[m..].trim_start();
    let rest = rest.strip_prefix('\'')?;
    let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    (hex.len() == 64).then(|| hex.to_ascii_lowercase())
}

pub fn file_sha256(path: &Path) -> std::io::Result<String> {
    use sha2::Digest;
    let mut f = std::fs::File::open(path)?;
    let mut h = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Lines of a log containing the case-sensitive words the script treats as a failed self-check.
pub fn selfcheck_failed(text: &str) -> bool {
    text.contains("FAILED") || text.contains("FATAL")
}

impl Run {
    fn new(inst: Installer, req: InstallRequest, bundle_dir: Option<PathBuf>, emit: Emit, on_root: OnRoot) -> Run {
        let lay = Layout::new(&req.install_dir);
        let manifest = bundle_dir.as_deref().and_then(bundle::load_manifest);
        let jobs = if req.jobs == 0 { default_jobs() } else { req.jobs.clamp(1, 64) };
        let mut state = StateFile::load(&lay.state()).unwrap_or_default();
        state.retarget(TARGET);
        Run {
            inst,
            state,
            lay,
            req,
            bundle_dir,
            manifest,
            emit,
            on_root,
            tools: None,
            jobs,
            started: Instant::now(),
            weights: BTreeMap::new(),
            finished: Vec::new(),
            step_started: Instant::now(),
            main_log: None,
            tail: VecDeque::new(),
            nsp_sizes: (0, 0),
            update_known: false,
            lift_hash: String::new(),
            xlate_hash: String::new(),
            keys_hash: String::new(),
            archive: BTreeMap::new(),
            build_game_py: None,
            preview: None,
            keep_build: true,
            swapped: false,
            xlate_swapped: false,
            toolchain: Toolchain::Portable,
            shipped: None,
            bg: Vec::new(),
            bg_failed: Arc::new(AtomicBool::new(false)),
            pending_stop: None,
            low_priority_now: false,
            game_data: false,
        }
        .with_input_hashes()
        .with_toolchain()
    }

    /// The shipped tools, and the compiler: an install whose objects were compiled with MSYS2 keeps it while MSYS2 has
    /// what the build needs (another compiler would recompile every object); anything else uses the portable toolchain.
    fn with_toolchain(mut self) -> Run {
        self.shipped = prebuilt::load(self.bundle_dir.as_deref());
        #[cfg(windows)]
        {
            let msys = self.state.toolchain == "msys2" || (self.state.toolchain.is_empty() && self.lay.built_with() == Some(Toolchain::Msys2));
            self.toolchain = if msys && self.lay.objects_kept() && prereq::msys2_builds() { Toolchain::Msys2 } else { Toolchain::Portable };
        }
        #[cfg(not(windows))]
        {
            // Linux builds with the system's clang/lld/ninja and libraries (engine/docs/building.md, "Linux").
            self.toolchain = Toolchain::System;
        }
        self
    }

    fn portable(&self) -> bool {
        self.toolchain == Toolchain::Portable
    }

    /// The RomFS merge reads the NCAs inside the NSPs: the hactool in place is the shipped one, which can.
    fn merge_in_place(&self) -> bool {
        self.shipped.as_ref().is_some_and(|t| t.manifest.hactool_ranges && t.has(prebuilt::HACTOOL))
    }

    fn commit(&self) -> String {
        self.manifest.as_ref().map(|m| m.commit.clone()).unwrap_or_default()
    }

    fn cancelled(&self) -> bool {
        self.inst.cancel.load(Ordering::SeqCst)
    }

    fn log(&mut self, line: &str) {
        if let Some(f) = self.main_log.as_mut() {
            let _ = writeln!(f, "[{}] {line}", now_stamp());
        }
    }

    fn update(&self, force: bool, f: impl FnOnce(&mut Snapshot)) {
        let snap = {
            let mut g = self.inst.lock();
            f(&mut g.snap);
            let due = g.last_emit.is_none_or(|t| t.elapsed() >= Duration::from_millis(400));
            if !force && !due {
                return;
            }
            g.last_emit = Some(Instant::now());
            g.snap.clone()
        };
        (self.emit)(&snap);
    }

    fn set_step_status(&self, id: StepId, status: &'static str, secs: Option<f64>, note: Option<String>) {
        self.update(true, |s| {
            if let Some(v) = s.steps.iter_mut().find(|v| v.id == id) {
                v.status = status;
                if secs.is_some() {
                    v.secs = secs;
                }
                if note.is_some() {
                    v.note = note;
                }
            }
        });
    }

    fn save_state(&mut self) {
        self.state.updated = crate::util::now_secs();
        let _ = std::fs::create_dir_all(&self.lay.root);
        let _ = self.state.save(&self.lay.state());
    }

    /// The step a step running beside the others must have finished before: the toolchain download before the
    /// compile; the RomFS merge before the translator step, so before the save keys (the lift reads only the ExeFS, and
    /// the compile compiles every object beside the merge and waits for it only before the RomFS index and the link:
    /// `Run::step_compile`).
    fn bg_before(&self, id: StepId) -> Option<StepId> {
        match id {
            StepId::Prereqs => Some(StepId::Compile),
            StepId::Romfs => Some(StepId::Xlate),
            _ => None,
        }
    }

    /// Seconds left: the foreground steps after `id` (whose own remaining time is `current`), with the steps that run
    /// beside them counted once ([`progress::lanes_eta`]).
    fn eta_after(&self, id: Option<StepId>, current: f64, speed: f64) -> f64 {
        let running: Vec<StepId> = self.bg.iter().map(|b| b.id).collect();
        let mut fg = Vec::new();
        let mut order = Vec::new();
        if let Some(id) = id {
            fg.push(current);
            order.push(id);
        }
        let mut planned = Vec::new();
        for (&k, &w) in self.weights.iter().filter(|(k, w)| id.is_none_or(|i| **k > i) && **w > 0.0 && !running.contains(k)) {
            match self.bg_before(k) {
                Some(before) => planned.push((w * speed, before)),
                None => {
                    fg.push(w * speed);
                    order.push(k);
                }
            }
        }
        let join = |before: StepId| order.iter().position(|k| *k == before).unwrap_or(order.len());
        let mut lanes: Vec<(f64, usize)> = planned.into_iter().map(|(rem, before)| (rem, join(before))).collect();
        for b in &self.bg {
            let p = b.prog.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let rem = progress::step_eta(b.started.elapsed().as_secs_f64(), p.fraction, b.reference, speed);
            lanes.push((rem, join(b.before)));
        }
        progress::lanes_eta(&fg, &lanes)
    }

    /// The steps running beside the current one, for the screen (`except`: the one shown as the current step).
    fn bg_views(&self, except: Option<StepId>) -> Vec<BgView> {
        self.bg
            .iter()
            .filter(|b| Some(b.id) != except)
            .map(|b| {
                let p = b.prog.lock().unwrap_or_else(|e| e.into_inner()).clone();
                BgView { id: b.id, fraction: p.fraction, stage: p.stage, detail: p.detail, elapsed: b.started.elapsed().as_secs_f64() }
            })
            .collect()
    }

    /// Seconds until the merged game data is complete (the Locker can read it then): the merge's own estimate while it
    /// runs beside the others; before it starts, the foreground steps before it (`current` with `current_rem` seconds
    /// left), then its reference. `None` once it is complete, or when nothing can be said.
    fn game_data_eta(&self, current: Option<StepId>, current_rem: f64, speed: f64) -> Option<f64> {
        if self.game_data {
            return None;
        }
        if let Some(b) = self.bg.iter().find(|b| b.id == StepId::Romfs) {
            let p = b.prog.lock().unwrap_or_else(|e| e.into_inner()).clone();
            return Some(progress::step_eta(b.started.elapsed().as_secs_f64(), p.fraction, b.reference, speed));
        }
        let merge = self.weights.get(&StepId::Romfs).copied().unwrap_or(0.0) * speed;
        match current {
            Some(StepId::Romfs) => Some(merge),
            Some(c) if c < StepId::Romfs => {
                let before: f64 = self
                    .weights
                    .iter()
                    .filter(|(k, _)| **k > c && **k < StepId::Romfs && self.bg_before(**k).is_none())
                    .map(|(_, w)| w * speed)
                    .sum();
                Some(current_rem + before + merge)
            }
            None if self.weights.is_empty() => None,
            None => Some(self.weights.iter().filter(|(k, _)| **k < StepId::Romfs && self.bg_before(**k).is_none()).map(|(_, w)| w * speed).sum::<f64>() + merge),
            _ => None,
        }
    }

    /// Recomputes progress numbers of the snapshot from the current step's fraction.
    fn progress(&self, id: StepId, fraction: Option<f64>, stage: Option<String>, detail: Option<String>) {
        let speed = progress::speed_factor(&self.finished);
        let reference = self.reference(id, &self.commit());
        let el = self.step_started.elapsed().as_secs_f64();
        let step_eta = progress::step_eta(el, fraction, reference, speed);
        let eta = self.eta_after(Some(id), step_eta, speed);
        let tail: Vec<String> = self.tail.iter().cloned().collect();
        let background = self.bg_views(None);
        let elapsed = self.started.elapsed().as_secs_f64();
        let data_eta = self.game_data_eta(Some(id), step_eta, speed);
        // The link writes the new executable and renames it over the old one: a game started now would hold it.
        let replacing = id == StepId::Compile && stage.as_deref() == Some("link");
        self.update(false, |s| {
            s.game_data_eta = data_eta;
            s.replacing = replacing;
            s.current = Some(id);
            s.step_fraction = fraction;
            s.stage = stage;
            s.detail = detail;
            s.step_elapsed = el;
            s.step_eta = Some(step_eta);
            s.elapsed = elapsed;
            // Time done over time expected: never backwards.
            s.overall = s.overall.max(progress::overall(elapsed, 0.0, 0.0, elapsed + eta).min(0.99));
            s.eta = Some(eta);
            s.tail = tail;
            s.background = background;
        });
    }

    fn execute(&mut self) {
        let _ = std::fs::create_dir_all(self.lay.logs());
        let main_log = self.lay.logs().join(format!("install-{}.log", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        self.main_log = std::fs::OpenOptions::new().create(true).append(true).open(&main_log).ok();
        self.log(&format!("Kalamar install wizard, folder {}", self.lay.root.display()));
        let commit = self.commit();
        self.log(&format!("runtime source {commit}"));
        self.log(&format!(
            "    tools: {}{}; {} compile jobs",
            match self.toolchain {
                Toolchain::Portable => format!("portable ({}) in {}", portable::toolchain_id(), self.lay.tools().display()),
                Toolchain::Msys2 => "MSYS2 UCRT64 (the objects in place were compiled with it)".into(),
                Toolchain::System => "the system's clang, lld, Ninja and libraries (nothing downloaded)".into(),
            },
            match &self.shipped {
                Some(t) => format!(", shipped hactool and translator (engine {})", &t.manifest.engine_commit[..t.manifest.engine_commit.len().min(12)]),
                None => ", no shipped tools".into(),
            },
            self.jobs
        ));
        if self.state.created == 0 {
            self.state.created = crate::util::now_secs();
        }
        self.state.schema = 1;
        self.state.tool = "Kalamar".into();
        self.state.base_nsp = self.req.base_nsp.clone();
        self.state.update_nsp = self.req.update_nsp.clone();
        self.state.prod_keys = self.req.prod_keys.clone();
        self.state.jobs = self.jobs;
        self.state.finished = false;
        self.state.needs_reboot = false;
        self.state.last_error = None;
        // Kept unless the player asked to free the space: the next update then recompiles only what changed.
        self.keep_build = !self.req.free_build_files;
        steps::adopt_records(&mut self.state, &self.lay);
        // Decided before anything changes on disk: what the bundled source changes against the installed one.
        self.preview = self.assess();
        if let Some(p) = &self.preview {
            self.log(&format!(
                "    update: {} ({} files differ, {} in runtime, {} in recomp); generated C {}; compile {}: about {} objects of {}, {:.0} s (at most {:.0} s){}",
                p.kind,
                p.changed_files,
                p.changed_runtime,
                p.changed_generator,
                if p.lift { "made again" } else { "kept" },
                if p.compile { "runs" } else { "skipped" },
                p.objects,
                p.objects_total,
                p.compile_secs,
                p.compile_secs_max,
                if p.reasons.is_empty() { String::new() } else { format!(", every generated object: {}", p.reasons.join(", ")) },
            ));
        }
        if self.req.force_rebuild {
            // From scratch: the generated C, the objects and Ninja's records go, so nothing of the last build is reused
            // (the compile record stays: it names the build kept as the previous version).
            self.state.compile_inputs.clear();
            self.state.lift_inputs.clear();
            for p in [self.lay.lift(), self.lay.build().join("obj"), self.lay.build().join(".ninja_log"), self.lay.build().join(".ninja_deps")] {
                let _ = sys::remove_all(&p);
            }
        }
        self.save_state();
        self.plan_weights();
        // An executable in place: an update, whose game stays playable until the link. Game data already merged (the
        // merge step will be skipped): the Locker can read it from the start.
        let update = self.lay.game_exe().is_file();
        self.game_data = self.lay.romfs_done();
        let game_data = self.game_data;
        let data_eta = self.game_data_eta(None, 0.0, 1.0);
        self.update(true, |s| {
            s.source_commit = Some(commit.clone());
            s.update = update;
            s.game_data = game_data;
            s.game_data_eta = data_eta;
        });

        let mut stop: Option<(&'static str, Option<InstallError>)> = None;
        for id in StepId::ALL {
            // The steps running beside the others: the ones that ended are recorded, the ones this step needs waited for (all
            // of them once one has failed).
            self.reap_background();
            let wait_for = if self.bg_failed.load(Ordering::SeqCst) { None } else { Some(id) };
            if let Some(s) = self.join_background(wait_for) {
                stop = Some(s);
                break;
            }
            if self.cancelled() {
                self.set_step_status(id, "cancelled", None, None);
                stop = Some(("cancelled", None));
                break;
            }
            self.step_started = Instant::now();
            self.tail.clear();
            self.low_priority_now = false;
            self.set_step_status(id, "running", None, None);
            self.update(true, |s| {
                s.current = Some(id);
                s.stage = None;
                s.detail = None;
                s.step_fraction = None;
                s.waiting = None;
                s.replacing = false;
                s.tail.clear();
            });
            self.log(&format!("=== {} ===", id.name()));
            self.state.current = Some(id.name().to_string());
            self.save_state();
            let r = self.run_step(id);
            let secs = self.step_started.elapsed().as_secs_f64();
            if r.as_ref().is_ok_and(|f| *f == Flow::Background) {
                self.log("    goes on beside the next steps");
                continue;
            }
            if let Err(f) = &r
                && f.code == "bg_failed"
            {
                // Stopped because a step running beside it failed: this one is redone next time; the failure reported is
                // that step's.
                self.log("    stopped: a step running beside it failed");
                self.set_step_status(id, "pending", None, None);
                // A step that waited for one beside it inside its own work (the compile) left the failure here.
                stop = self.pending_stop.take().or_else(|| self.join_background(None)).or(Some(("failed", None)));
                break;
            }
            if let Some(s) = self.record(id, r, secs, false) {
                stop = Some(s);
                break;
            }
        }
        if stop.is_some() {
            // Whatever still runs beside stops too (the next run redoes it); its own failure is not the one reported.
            self.halt_background();
            let _ = self.join_background(None);
        } else {
            // Nothing is left beside the last step (the compile waits for both), but a run never ends with one running.
            stop = self.join_background(None);
        }
        let (outcome, error) = stop.unwrap_or(("done", None));
        if outcome == "done" {
            self.state.current = None;
            self.state.finished = true;
            self.state.footprint = sys::dir_size(&self.lay.root);
            self.save_state();
        }
        let footprint = (outcome == "done").then_some(self.state.footprint);
        self.log(&format!("end: {outcome}"));
        let elapsed = self.started.elapsed().as_secs_f64();
        {
            let mut g = self.inst.lock();
            g.job = None;
            g.bg_jobs.clear();
        }
        self.update(true, |s| {
            s.active = false;
            s.outcome = Some(outcome);
            s.error = error;
            s.footprint = footprint;
            s.waiting = None;
            s.replacing = false;
            s.game_data_eta = None;
            s.elapsed = elapsed;
            s.eta = None;
            s.step_eta = None;
            s.background.clear();
            if outcome == "done" {
                s.overall = 1.0;
                s.current = None;
            }
        });
    }

    /// Records how a step ended. `Some((outcome, error))` when the run stops there.
    fn record(&mut self, id: StepId, r: StepResult, secs: f64, beside: bool) -> Option<(&'static str, Option<InstallError>)> {
        let commit = self.commit();
        match r {
            Ok(flow) => {
                let status = if flow == Flow::Skipped { "skipped" } else { "done" };
                self.log(&format!("    {}: {status} in {secs:.0} s", id.name()));
                if flow == Flow::Done {
                    let reference = self.weights.get(&id).copied().unwrap_or(0.0);
                    if reference > 0.0 {
                        self.finished.push((secs, self.reference(id, &commit)));
                    }
                }
                let worked_secs = if flow == Flow::Done { secs } else { self.state.steps.get(id.name()).map_or(0.0, |r| r.worked_secs) };
                self.state.steps.insert(id.name().into(), StepRecord { status: "done".into(), at: crate::util::now_secs(), secs, commit, worked_secs });
                self.save_state();
                // A skipped step's weight leaves the total so the estimate does not count it.
                if flow == Flow::Skipped {
                    self.weights.insert(id, 0.0);
                }
                // The merge is over and its outputs were checked (`step_romfs`): the Locker may read them now.
                if id == StepId::Romfs && self.lay.romfs_done() && !self.game_data {
                    self.game_data = true;
                    self.log("    the merged game data is complete: the Locker can read it");
                    self.update(true, |s| {
                        s.game_data = true;
                        s.game_data_eta = None;
                    });
                }
                self.set_step_status(id, status, Some(secs), None);
                // A step that ran beside the others ends without taking the place of the current one.
                if !beside {
                    self.progress(id, Some(1.0), None, None);
                }
                None
            }
            Err(f) => {
                let status = if f.cancelled { "cancelled" } else { "failed" };
                self.log(&format!("    {}: {status}: {} {:?}", id.name(), f.code, f.params));
                if id == StepId::Romfs {
                    self.remove_merge_temps();
                }
                // An update that does not finish leaves the previous game (and translator) in place.
                if self.swapped || self.xlate_swapped {
                    if self.swapped {
                        // The executable in place is the previous one again, not what the objects were built from.
                        self.state.compile_inputs.clear();
                    }
                    self.restore_previous();
                    self.state.steps.remove(StepId::Compile.name());
                }
                let worked_secs = self.state.steps.get(id.name()).map_or(0.0, |r| r.worked_secs);
                self.state.steps.insert(id.name().into(), StepRecord { status: status.into(), at: crate::util::now_secs(), secs, commit, worked_secs });
                let outcome = if f.cancelled {
                    "cancelled"
                } else if f.reboot {
                    "reboot"
                } else {
                    "failed"
                };
                let mut error = None;
                if !f.cancelled {
                    let log = f.log.clone();
                    let tail = log.as_deref().and_then(|p| crate::util::tail_lines(p, 30).ok().flatten()).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default();
                    let e = InstallError { step: id, code: f.code.clone(), params: f.params.clone(), log: log.map(|p| p.display().to_string()), tail, exit_code: f.exit_code };
                    self.state.last_error = Some(e.clone());
                    error = Some(e);
                }
                self.state.needs_reboot = f.reboot;
                self.save_state();
                self.set_step_status(id, status, Some(secs), None);
                Some((outcome, error))
            }
        }
    }

    /// Waits for the steps running beside the others that must finish before `before` (all of them with `None`), shows
    /// the one waited for as the current step, and records how each ended. `Some` when the run stops (the first failure;
    /// a cancel).
    fn join_background(&mut self, before: Option<StepId>) -> Option<(&'static str, Option<InstallError>)> {
        self.join_background_where(&|b: &Background| before.is_none_or(|s| b.before <= s))
    }

    /// The same for the steps running beside the others that `pick` names.
    fn join_background_where(&mut self, pick: &dyn Fn(&Background) -> bool) -> Option<(&'static str, Option<InstallError>)> {
        let mut stop = None;
        loop {
            // A finished one first: after a failure, the failed step is the one recorded, and the others are stopped.
            let due = |b: &Background| pick(b);
            let finished = |b: &Background| b.done.is_some() || b.handle.as_ref().is_none_or(|h| h.is_finished());
            let Some(i) = self.bg.iter().position(|b| due(b) && finished(b)).or_else(|| self.bg.iter().position(due)) else { break };
            let id = self.bg[i].id;
            let mut waited = false;
            while self.bg[i].done.is_none() && !self.bg[i].handle.as_ref().is_none_or(|h| h.is_finished()) {
                if !waited {
                    self.log(&format!("    waiting for {} (it runs beside the other steps)", id.name()));
                    waited = true;
                }
                let speed = progress::speed_factor(&self.finished);
                let b = &self.bg[i];
                let p = b.prog.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let el = b.started.elapsed().as_secs_f64();
                let rem = progress::step_eta(el, p.fraction, b.reference, speed);
                let later = self.eta_after(None, 0.0, speed);
                let elapsed = self.started.elapsed().as_secs_f64();
                let others = self.bg_views(Some(id));
                let data_eta = self.game_data_eta(None, 0.0, speed);
                self.update(false, |s| {
                    s.current = Some(id);
                    s.step_fraction = p.fraction;
                    s.stage = p.stage;
                    s.detail = p.detail;
                    s.step_elapsed = el;
                    s.step_eta = Some(rem);
                    s.elapsed = elapsed;
                    s.game_data_eta = data_eta;
                    s.eta = Some(later.max(rem));
                    s.overall = s.overall.max(progress::overall(elapsed, 0.0, 0.0, elapsed + later.max(rem)).min(0.99));
                    s.background = others;
                });
                std::thread::sleep(Duration::from_millis(250));
            }
            let mut b = self.bg.remove(i);
            let panicked = || (Err(Fail::new("io").with("what", "thread").with("message", "panicked")), 0.0);
            let (r, secs) = match b.done.take() {
                Some(d) => d,
                None => b.handle.take().map(|h| h.join().unwrap_or_else(|_| panicked())).unwrap_or((Ok(Flow::Done), 0.0)),
            };
            if stop.is_some() {
                // Already stopping: a later failure is only recorded as stopped.
                let r = r.map_err(|f| Fail { cancelled: true, ..f });
                let _ = self.record(id, r, secs, true);
                continue;
            }
            if let Some(s) = self.record(id, r, secs, true) {
                stop = Some(s);
                self.halt_background();
            }
        }
        if stop.is_none() {
            self.bg_failed.store(false, Ordering::SeqCst);
        }
        let background = self.bg_views(None);
        self.update(true, |s| s.background = background);
        stop
    }

    /// Records the steps running beside the others that have ended successfully, as soon as they have (their status and
    /// duration show then, not when the step that needs them starts). A failure stays for [`Run::join_background`],
    /// which reports it.
    fn reap_background(&mut self) {
        let mut i = 0;
        while i < self.bg.len() {
            let b = &mut self.bg[i];
            if b.done.is_none() && b.handle.as_ref().is_some_and(|h| h.is_finished()) {
                b.done = b.handle.take().map(|h| h.join().unwrap_or_else(|_| (Err(Fail::new("io").with("what", "thread").with("message", "panicked")), 0.0)));
            }
            if b.done.as_ref().is_some_and(|(r, _)| r.is_ok()) {
                let b = self.bg.remove(i);
                let (r, secs) = b.done.expect("done");
                let _ = self.record(b.id, r, secs, true);
            } else {
                i += 1;
            }
        }
    }

    /// Stops the steps running beside the others (their process trees are killed, a download stops at its next chunk).
    fn halt_background(&self) {
        let g = self.inst.lock();
        for j in &g.bg_jobs {
            j.terminate();
        }
        for b in &self.bg {
            b.halt.store(true, Ordering::SeqCst);
        }
    }

    /// Predicts which steps will be skipped, so the estimate counts only the work this run does.
    fn plan_weights(&mut self) {
        let commit = self.commit();
        for id in StepId::ALL {
            let skip = !id.always_runs() && self.is_done(id, &commit);
            let w = if skip { 0.0 } else { self.reference(id, &commit) };
            self.weights.insert(id, w);
        }
        let refs: BTreeMap<StepId, f64> = StepId::ALL.iter().map(|&id| (id, self.reference(id, &commit))).collect();
        self.update(true, |s| {
            for v in s.steps.iter_mut() {
                v.reference = refs.get(&v.id).copied().unwrap_or(v.reference);
            }
        });
    }

    /// Reference duration of a step for this install: the lift and the compile from what this update changes
    /// ([`incremental::assess`]: a few minutes when only the runtime changed), the others from the reference run.
    fn reference(&self, id: StepId, _commit: &str) -> f64 {
        match (id, &self.preview) {
            (StepId::Lift, Some(p)) if p.lift => p.lift_secs,
            (StepId::Compile, Some(p)) if p.compile => p.compile_secs,
            _ => self.reference_secs(id),
        }
    }

    /// The reference run's duration of a step with this install's tools.
    fn reference_secs(&self, id: StepId) -> f64 {
        if self.portable() { progress::reference_secs_portable(id, self.jobs) } else { progress::reference_secs(id, self.jobs) }
    }

    /// Fills the bundled source's file hashes and input digests (used to tell which outputs are still valid).
    fn with_input_hashes(mut self) -> Run {
        if let Some(dir) = &self.bundle_dir
            && let Ok(gz) = std::fs::read(dir.join(bundle::ARCHIVE_NAME))
            && let Ok(files) = bundle::read_archive(&gz)
        {
            self.archive = incremental::archive_hashes(&files);
            self.lift_hash = bundle::inputs_hash_map(&self.archive, bundle::LIFT_INPUTS);
            self.xlate_hash = bundle::inputs_hash_map(&self.archive, bundle::XLATE_INPUTS);
            self.keys_hash = bundle::inputs_hash_map(&self.archive, bundle::KEYS_INPUTS);
            self.build_game_py = files.iter().find(|(p, _)| p == "tools/build_game.py").map(|(_, d)| String::from_utf8_lossy(d).into_owned());
        }
        self
    }

    /// Digest of everything the compile reads from the bundled source, the target and the RomFS index in place.
    fn compile_hash(&self) -> String {
        if self.archive.is_empty() {
            return String::new();
        }
        let key = incremental::compile_key(&self.archive, TARGET, &self.lay.romfs_identity());
        // The compiler is an input too; MSYS2 builds keep the digest they were recorded with before it was.
        match self.toolchain {
            Toolchain::Msys2 | Toolchain::System => key,
            Toolchain::Portable => bundle::sha256_hex(format!("{key}\0{}", portable::toolchain_id()).as_bytes()),
        }
    }

    /// The executable in place was compiled from the bundled source. Installs made before the digest was recorded
    /// fall back on the source commit.
    fn compile_valid(&self, commit: &str) -> bool {
        if !self.lay.game_exe().is_file() || self.req.force_rebuild {
            return false;
        }
        if self.state.compile_inputs.is_empty() {
            return self.state.done_with(StepId::Compile, commit);
        }
        let h = self.compile_hash();
        !h.is_empty() && self.state.compile_inputs == h
    }

    /// The generated C is complete and was made by the bundled generator.
    fn lift_valid(&self) -> bool {
        !self.req.force_rebuild && self.lay.lift_outputs() && !self.lift_hash.is_empty() && self.state.lift_inputs == self.lift_hash
    }

    /// What the update does to the generated C and the objects (`None` without a bundle).
    fn assess(&self) -> Option<incremental::UpdatePreview> {
        if self.archive.is_empty() {
            return None;
        }
        let commit = self.commit();
        let src = self.lay.src();
        let ninja = incremental::load_ninja(&self.lay.build());
        let installed_bg = std::fs::read_to_string(src.join("tools").join("build_game.py")).ok();
        let flags_changed = |name: &str| match (&installed_bg, &self.build_game_py) {
            (Some(a), Some(b)) => incremental::flag_block(a, name) != incremental::flag_block(b, name),
            _ => false,
        };
        let facts = incremental::Facts {
            changed: incremental::source_changes(&self.archive, &src),
            stale: ninja.as_ref().map(|n| incremental::stale_objects(n, &src)).unwrap_or_default(),
            lift_valid: self.lift_valid(),
            lift_complete: self.lay.lift_outputs(),
            compile_valid: self.compile_valid(&commit),
            objects: self.lay.objects_kept(),
            force: self.req.force_rebuild,
            gen_flags_changed: flags_changed("GEN_FLAGS"),
            rt_flags_changed: flags_changed("RT_FLAGS"),
            toolchain_changed: self.lay.objects_kept() && self.lay.built_with().is_some_and(|t| t != self.toolchain),
            jobs: self.jobs,
        };
        let prefix = |p: &Path| format!("{}/", incremental::norm(&p.display().to_string()).trim_end_matches('/'));
        // A lift always does the same work: this PC's last one says how long it takes. The compile's own records give
        // its durations object by object.
        let lift_secs = self
            .state
            .steps
            .get(StepId::Lift.name())
            .filter(|r| r.status == "done")
            .map(|r| if r.worked_secs > 0.0 { r.worked_secs } else { r.secs })
            .filter(|&s| s >= 30.0)
            .unwrap_or_else(|| self.reference_secs(StepId::Lift));
        Some(incremental::assess(
            &facts,
            ninja.as_ref(),
            &prefix(&src),
            &prefix(&self.lay.build().join("obj")),
            lift_secs,
            self.reference_secs(StepId::Compile),
        ))
    }

    /// Whether a step's outputs are there and still valid. The steps that depend on the runtime source compare digests
    /// of their inputs in the bundled source with the ones recorded when they last ran, never commits or dates, so a new
    /// launcher whose source changed only elsewhere redoes nothing. Intermediates count as done when what they feed is.
    fn is_done(&self, id: StepId, commit: &str) -> bool {
        let l = &self.lay;
        let compile_done = self.compile_valid(commit);
        match id {
            StepId::Prereqs => match self.toolchain {
                Toolchain::Portable => portable::toolchain_ready(&l.root),
                Toolchain::Msys2 => true,
                Toolchain::System => prereq::system_ready(),
            },
            StepId::Sources => {
                bundle::load_marker(&l.src()).is_some_and(|m| m == commit)
                    && self.manifest.as_ref().is_none_or(|m| bundle::load_marker_archive(&l.src()).is_none_or(|a| a == m.sha256))
                    && l.src().join("recomp").join("gen.py").is_file()
            }
            StepId::Python => portable::python_ready(&l.root) || l.venv_py().is_file(),
            StepId::Exefs => l.exefs().join("main").is_file(),
            StepId::Hactool => l.romfs_done() || self.hactool_current(),
            StepId::Romfs => l.romfs_done(),
            StepId::Lift => compile_done || self.lift_valid(),
            StepId::Compile => compile_done,
            StepId::Xlate => l.xlate_exe().is_file() && !self.xlate_hash.is_empty() && self.state.xlate_inputs == self.xlate_hash,
            StepId::Keys => {
                l.keys_json().is_file()
                    && if self.state.keys_inputs.is_empty() { self.state.done_with(StepId::Keys, commit) } else { self.state.keys_inputs == self.keys_hash }
            }
            _ => false,
        }
    }

    fn run_step(&mut self, id: StepId) -> StepResult {
        match id {
            StepId::Checks => self.step_checks(),
            StepId::Prereqs => self.step_prereqs(),
            StepId::Sources => self.step_sources(),
            StepId::Python => self.step_python(),
            StepId::Exefs => self.step_exefs(),
            StepId::Hactool => self.step_hactool(),
            StepId::Romfs => self.step_romfs(),
            StepId::Lift => self.step_lift(),
            StepId::Compile => self.step_compile(),
            StepId::Xlate => self.step_xlate(),
            StepId::Keys => self.step_keys(),
            StepId::Layout => self.step_layout(),
            StepId::Selftest => self.step_selftest(),
            StepId::Cleanup => self.step_cleanup(),
        }
    }

    // ---- command execution ------------------------------------------------------------------------------------

    fn tool_path(&self, id: &str) -> Option<PathBuf> {
        self.tools.as_ref().and_then(|t| t.path_of(id))
    }

    /// Environment of the build commands; no system or user variable is changed. Portable: the install folder's own
    /// tools first on PATH, and Python kept from the user's packages and variables (as a virtual environment would).
    /// MSYS2: the script's (MSYS2 UCRT64 and usr first, then git and dotnet).
    fn child_env(&self, cmd: &mut Command) {
        let mut dirs = Vec::new();
        if self.portable() {
            dirs.push(portable::bin_dir(&self.lay.root));
            dirs.push(portable::python_dir(&self.lay.root));
        } else if cfg!(windows) {
            let msys = Path::new(prereq::MSYS_ROOT);
            dirs.extend([msys.join("ucrt64").join("bin"), msys.join("usr").join("bin")]);
        } else {
            // Linux: the install's own Python environment, then the system's tools.
            dirs.push(self.lay.venv().join("bin"));
        }
        for t in ["git", "dotnet"] {
            if let Some(d) = self.tool_path(t).and_then(|p| p.parent().map(Path::to_path_buf)) {
                dirs.push(d);
            }
        }
        if let Some(p) = std::env::var_os("PATH") {
            dirs.extend(std::env::split_paths(&p));
        }
        if let Ok(p) = std::env::join_paths(dirs) {
            cmd.env("PATH", p);
        }
        sys::clean_child_env(cmd);
        cmd.env_remove("PYTHONPATH").env_remove("PYTHONHOME").env("PYTHONNOUSERSITE", "1");
        cmd.env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .env("DOTNET_NOLOGO", "1")
            .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
            .env("NUGET_PACKAGES", self.lay.nuget().join("packages"))
            .env("PYTHONUTF8", "1")
            .env("PYTHONUNBUFFERED", "1")
            // No MSBuild or compiler servers left behind by the translator's build (they would hold the NuGet cache).
            .env("MSBUILDDISABLENODEREUSE", "1")
            .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
            .env("UseSharedCompilation", "false");
    }

    /// Starts a step's command beside the next steps ([`Background`]): its output goes to `logs\<log>.log`, its process
    /// tree into a job object a cancel kills. `tick` runs twice a second with the step's progress; `after` turns the exit
    /// code into the step's result (in the step's thread).
    #[allow(clippy::too_many_arguments)]
    fn spawn_background(
        &mut self,
        id: StepId,
        exe: &Path,
        args: &[String],
        cwd: &Path,
        log: &str,
        mut tick: Box<dyn FnMut(&mut Prog) + Send>,
        after: Box<dyn FnOnce(i32, PathBuf) -> StepResult + Send>,
    ) -> StepResult {
        let before = self.bg_before(id).unwrap_or(StepId::Layout);
        let log_path = self.lay.logs().join(format!("{log}.log"));
        let _ = std::fs::create_dir_all(self.lay.logs());
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| Fail::io("log", e))?;
        let shown: Vec<String> = args.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect();
        let header = format!("> {} {}", exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), shown.join(" "));
        let _ = writeln!(f, "[{}] {header}", now_stamp());
        self.log(&format!("    {header}"));
        let out = f.try_clone().map_err(|e| Fail::io("log", e))?;
        let mut cmd = Command::new(exe);
        cmd.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(out).stderr(f);
        self.child_env(&mut cmd);
        sys::build_command(&mut cmd, self.req.low_priority);
        let job = Arc::new(sys::Job::new(self.req.low_priority).map_err(|e| Fail::io("job", e))?);
        let mut child = cmd.spawn().map_err(|e| Fail::new("spawn_failed").with("program", exe.display()).with("message", e))?;
        let _ = job.assign(&child);
        let halt = Arc::new(AtomicBool::new(self.cancelled()));
        {
            let mut g = self.inst.lock();
            g.bg_jobs.push(job.clone());
            g.halts.push(halt.clone());
        }
        let prog = Arc::new(Mutex::new(Prog::default()));
        let (p2, h2, failed) = (prog.clone(), halt.clone(), self.bg_failed.clone());
        let handle = std::thread::spawn(move || {
            let t0 = Instant::now();
            let mut last_tick = Instant::now() - Duration::from_secs(1);
            let code = loop {
                let st = child.try_wait();
                if h2.load(Ordering::SeqCst) {
                    job.terminate();
                }
                if last_tick.elapsed() >= Duration::from_millis(500) {
                    let mut p = p2.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    tick(&mut p);
                    *p2.lock().unwrap_or_else(|e| e.into_inner()) = p;
                    last_tick = Instant::now();
                }
                match st {
                    Ok(Some(s)) => break s.code().unwrap_or(-1),
                    Ok(None) => std::thread::sleep(Duration::from_millis(250)),
                    Err(_) => {
                        job.terminate();
                        break -1;
                    }
                }
            };
            let r = if h2.load(Ordering::SeqCst) { Err(Fail { log: Some(log_path.clone()), ..Fail::cancelled() }) } else { after(code, log_path) };
            if r.as_ref().is_err_and(|f| !f.cancelled) {
                failed.store(true, Ordering::SeqCst);
            }
            (r, t0.elapsed().as_secs_f64())
        });
        let reference = self.reference(id, &self.commit());
        self.bg.push(Background { id, before, started: Instant::now(), reference, handle: Some(handle), prog, halt, done: None });
        Ok(Flow::Background)
    }

    /// Runs `work` in a thread beside the next steps (the toolchain download). `work` gets the stop flag and the step's
    /// progress.
    #[cfg(windows)]
    fn spawn_background_work(&mut self, id: StepId, work: BgWork) -> StepResult {
        let before = self.bg_before(id).unwrap_or(StepId::Layout);
        let halt = Arc::new(AtomicBool::new(self.cancelled()));
        self.inst.lock().halts.push(halt.clone());
        let prog = Arc::new(Mutex::new(Prog::default()));
        let (p2, h2, failed) = (prog.clone(), halt.clone(), self.bg_failed.clone());
        let handle = std::thread::spawn(move || {
            let t0 = Instant::now();
            let r = work(&h2, &p2);
            if r.as_ref().is_err_and(|f| !f.cancelled) {
                failed.store(true, Ordering::SeqCst);
            }
            (r, t0.elapsed().as_secs_f64())
        });
        let reference = self.reference(id, &self.commit());
        self.bg.push(Background { id, before, started: Instant::now(), reference, handle: Some(handle), prog, halt, done: None });
        Ok(Flow::Background)
    }

    /// Runs one command, its stdout and stderr appended to `logs\<log>.log`, inside a job object. `parse` sees every
    /// output line; `tick` runs twice a second. Returns the exit code; a cancel kills the tree (unless `no_kill`: the
    /// prerequisite installers are let finish) and returns the cancelled failure.
    #[allow(clippy::too_many_arguments)]
    fn exec(
        &mut self,
        id: StepId,
        exe: &Path,
        args: &[String],
        cwd: &Path,
        log: &str,
        no_kill: bool,
        parse: &mut dyn FnMut(&str, &mut Prog),
        tick: &mut dyn FnMut(&mut Prog),
    ) -> Result<i32, Fail> {
        let log_path = self.lay.logs().join(format!("{log}.log"));
        let _ = std::fs::create_dir_all(self.lay.logs());
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| Fail::io("log", e))?;
        let shown: Vec<String> = args.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect();
        let header = format!("> {} {}", exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), shown.join(" "));
        let _ = writeln!(f, "[{}] {header}", now_stamp());
        self.log(&format!("    {header}"));
        let start_len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let out = f.try_clone().map_err(|e| Fail::io("log", e))?;
        let mut cmd = Command::new(exe);
        cmd.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(out).stderr(f);
        self.child_env(&mut cmd);
        let low = self.req.low_priority || self.low_priority_now;
        sys::build_command(&mut cmd, low);
        let job = Arc::new(sys::Job::new(low).map_err(|e| Fail::io("job", e))?);
        let mut child = cmd.spawn().map_err(|e| Fail::new("spawn_failed").with("program", exe.display()).with("message", e))?;
        let _ = job.assign(&child);
        if !no_kill {
            self.inst.lock().job = Some(job.clone());
            if self.cancelled() {
                job.terminate();
            }
        }
        // Start from what the step already shows (its phase set before the command), so a quiet command keeps it.
        let shown = self.inst.snapshot();
        let mut prog = Prog { fraction: shown.step_fraction, stage: shown.stage, detail: shown.detail };
        let mut offset = start_len;
        let mut partial = String::new();
        let mut last_tick = Instant::now() - Duration::from_secs(1);
        let code = loop {
            if !no_kill && !self.bg.is_empty() && last_tick.elapsed() >= Duration::from_millis(500) {
                self.reap_background();
            }
            if !no_kill && self.bg_failed.load(Ordering::SeqCst) {
                // A step running beside this one failed: stop here, its failure is the one reported.
                job.terminate();
                let _ = child.wait();
                self.inst.lock().job = None;
                return Err(Fail { log: Some(log_path), ..Fail::new("bg_failed") });
            }
            let status = child.try_wait();
            self.read_new(&log_path, &mut offset, &mut partial, &mut prog, parse);
            if last_tick.elapsed() >= Duration::from_millis(500) {
                tick(&mut prog);
                last_tick = Instant::now();
            }
            self.progress(id, prog.fraction, prog.stage.clone(), prog.detail.clone());
            match status {
                Ok(Some(st)) => break st.code().unwrap_or(-1),
                Ok(None) => {}
                Err(e) => {
                    job.terminate();
                    self.inst.lock().job = None;
                    return Err(Fail::io("wait", e));
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        };
        self.read_new(&log_path, &mut offset, &mut partial, &mut prog, parse);
        if !partial.is_empty() {
            let line = std::mem::take(&mut partial);
            parse(&line, &mut prog);
            self.push_tail(&line);
        }
        self.inst.lock().job = None;
        drop(job);
        if self.cancelled() && !no_kill {
            return Err(Fail { log: Some(log_path), ..Fail::cancelled() });
        }
        Ok(code)
    }

    fn push_tail(&mut self, line: &str) {
        let line: String = line.chars().take(400).collect();
        self.tail.push_back(line);
        while self.tail.len() > 200 {
            self.tail.pop_front();
        }
    }

    fn read_new(&mut self, path: &Path, offset: &mut u64, partial: &mut String, prog: &mut Prog, parse: &mut dyn FnMut(&str, &mut Prog)) {
        let Ok(mut f) = std::fs::File::open(path) else { return };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len <= *offset || f.seek(SeekFrom::Start(*offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        let _ = (&mut f).take(4 << 20).read_to_end(&mut buf);
        *offset += buf.len() as u64;
        partial.push_str(&String::from_utf8_lossy(&buf));
        while let Some(i) = partial.find('\n') {
            let line: String = partial[..i].trim_end_matches('\r').to_string();
            partial.drain(..=i);
            for piece in line.split('\r') {
                if !piece.trim().is_empty() {
                    parse(piece, prog);
                    self.push_tail(piece);
                }
            }
        }
    }

    /// Runs and requires exit code 0.
    fn exec_ok(&mut self, id: StepId, exe: &Path, args: &[String], cwd: &Path, log: &str, parse: &mut dyn FnMut(&str, &mut Prog)) -> Result<(), Fail> {
        let code = self.exec(id, exe, args, cwd, log, false, parse, &mut |_| {})?;
        if code != 0 {
            let mut f = Fail::new("command_failed").with("program", exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            f.exit_code = Some(code);
            f.log = Some(self.lay.logs().join(format!("{log}.log")));
            return Err(f);
        }
        Ok(())
    }

    /// The Python the steps run ([`Layout::python`]).
    fn venv_py(&self) -> PathBuf {
        self.lay.python()
    }

    /// The hactool in place is the one the launcher ships (or, without one, the one built the old way).
    fn hactool_current(&self) -> bool {
        match &self.shipped {
            Some(t) => t.files.get(prebuilt::HACTOOL).is_some_and(|d| std::fs::read(self.lay.hactool()).is_ok_and(|have| have == *d)),
            None => self.lay.hactool_built().is_file(),
        }
    }

    /// Bytes of the two Program NCAs (known after the checks; the update's alone before).
    fn nca_bytes(&self) -> u64 {
        self.nsp_sizes.0 + self.nsp_sizes.1
    }

    /// What the steps from `from` on will add to the disk, in order (the ones predicted to be skipped left out).
    fn space_steps(&self, from: StepId) -> Vec<(StepId, space::Use)> {
        let commit = self.commit();
        StepId::ALL
            .iter()
            .filter(|&&id| id >= from && !id.always_runs() && !self.is_done(id, &commit))
            .map(|&id| (id, space::step_use(id, self.portable(), self.merge_in_place(), self.nca_bytes())))
            .collect()
    }

    /// Free space needed before `id` for it and every later step.
    fn space_need(&self, id: StepId) -> u64 {
        space::needs(&self.space_steps(id)).first().map_or(space::MARGIN, |n| n.1)
    }

    /// Stops before a step when the disk cannot hold it and what follows it. Skipped while a step runs beside the
    /// others: the check before that one already counted everything after it.
    fn check_space(&mut self, id: StepId) -> Result<(), Fail> {
        if !self.bg.is_empty() {
            return Ok(());
        }
        let need = self.space_need(id);
        let free = sys::free_bytes(&self.lay.root).unwrap_or(u64::MAX);
        self.log(&format!("    free space: {:.1} GB, needed from here on: {:.1} GB", free as f64 / 1e9, need as f64 / 1e9));
        if free < need {
            return Err(Fail::new("disk_space").with("free", format!("{:.1}", free as f64 / 1e9)).with("need", format!("{:.1}", need as f64 / 1e9)));
        }
        Ok(())
    }

    fn fail_log(&self, code: &str, log: &str) -> Fail {
        Fail { log: Some(self.lay.logs().join(format!("{log}.log"))), ..Fail::new(code) }
    }

    /// A game of THIS install runs (its executable is the one the link replaces): a process whose image is
    /// `build\splat3.exe`, whatever started it (the launcher, a shortcut, a check like `--selftest`).
    fn game_running(&self) -> bool {
        sys::count_image(&self.lay.game_exe()) > 0
    }

    /// Waits while the game of THIS install runs (the link must replace its executable): the update waits for the
    /// player, it never fails because of the game. Other games keep running. The time waited does not count as the
    /// step's work (its duration feeds this PC's estimates).
    fn wait_for_no_game(&mut self, id: StepId) -> Result<(), Fail> {
        let t0 = Instant::now();
        let mut waited = false;
        while self.game_running() {
            if self.cancelled() {
                return Err(Fail::cancelled());
            }
            if !waited {
                self.log("    waiting: the game of this install (splat3.exe) is open; the new executable is put in place once it is closed");
                waited = true;
            }
            let elapsed = self.started.elapsed().as_secs_f64();
            self.update(true, |s| {
                s.waiting = Some("game_running".into());
                s.replacing = false;
                s.current = Some(id);
                s.elapsed = elapsed;
            });
            std::thread::sleep(Duration::from_millis(1500));
        }
        if waited {
            self.log(&format!("    the game was closed after {:.0} s of waiting: continuing", t0.elapsed().as_secs_f64()));
            self.step_started += t0.elapsed();
            self.update(true, |s| s.waiting = None);
        }
        Ok(())
    }

    fn remove_merge_temps(&mut self) {
        for t in self.lay.merge_temps() {
            let _ = sys::remove_all(&t);
            self.log(&format!("    removed the merge tool's temporary folder {}", t.display()));
        }
    }

    fn remove_intermediate(&mut self, p: &Path) {
        if p.exists() {
            let size = sys::dir_size(p);
            match sys::remove_all(p) {
                Ok(()) => self.log(&format!("    removed {} ({:.2} GB, no longer needed)", p.display(), size as f64 / 1e9)),
                Err(e) => self.log(&format!("    could not remove {}: {e}", p.display())),
            }
        }
    }
}

/// Progress reported by a step's parser.
#[derive(Default, Debug, Clone)]
pub struct Prog {
    pub fraction: Option<f64>,
    pub stage: Option<String>,
    pub detail: Option<String>,
}

mod steps;

pub use steps::{
    Download, Existing, PREVIOUS_NEW, Plan, PlanStep, RollbackReport, UninstallReport, UpdateCheck, default_candidates, find_existing, first_need, folder_kind, fresh_estimate,
    needs_player_files, plan, recover_interrupted, rollback, settle_previous, uninstall, update_check,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_of_another_game_version_keeps_the_tools_and_forgets_the_game() {
        let rec = |c: &str| StepRecord { status: "done".into(), commit: c.into(), ..Default::default() };
        let mut st = StateFile { finished: true, keys_inputs: "k".into(), lift_inputs: "l".into(), compile_inputs: "c".into(), previous_commit: "p".into(), ..Default::default() };
        for id in StepId::ALL {
            st.steps.insert(id.name().into(), rec("c0"));
        }
        assert!(st.retarget(TARGET));
        let kept: Vec<&str> = st.steps.keys().map(String::as_str).collect();
        assert_eq!(kept, ["checks", "hactool", "prereqs", "python", "sources", "xlate"]);
        assert!(!st.finished && st.keys_inputs.is_empty() && st.lift_inputs.is_empty() && st.compile_inputs.is_empty() && st.previous_commit.is_empty());
        assert!(!st.done_with(StepId::Keys, "c0") && !st.done_with(StepId::Compile, "c0") && st.done_with(StepId::Xlate, "c0"));
        assert_eq!(st.target, TARGET);
        // the same game version again: nothing is touched; and a first install only records the target
        assert!(!st.retarget(TARGET));
        let mut fresh = StateFile::default();
        assert!(!fresh.retarget(TARGET) && fresh.target == TARGET);
    }

    #[test]
    fn version_hash_comes_from_targets_py() {
        let text = format!("TARGETS = {{\n  'other': {{'main': 'aa'}},\n  '{TARGET}': {{\n    'game': 'Splatoon 3',\n    'main':  'E20596A523990DD8B31DC452C7D9BAA89B409927EC10ED9B956806774521629C', }}}}");
        assert_eq!(expected_main_sha(&text, TARGET).unwrap(), "e20596a523990dd8b31dc452c7d9baa89b409927ec10ed9b956806774521629c");
        assert!(expected_main_sha(&format!("'{TARGET}': {{'main': 'abc'}}"), TARGET).is_none());
        assert!(expected_main_sha("nothing", TARGET).is_none());
    }

    #[test]
    fn selfcheck_words_are_case_sensitive_like_the_script() {
        assert!(!selfcheck_failed("[host] UnMapPhysicalMemory failed 0xdc01\nselftest: passed (0 failures)"));
        assert!(selfcheck_failed("check x: FAILED"));
        assert!(selfcheck_failed("[host] FATAL: boom"));
    }

    #[test]
    #[cfg(windows)]
    fn layout_matches_the_kit() {
        let l = Layout::new(r"C:\I");
        assert_eq!(l.game(), Path::new(r"C:\I\work\splatoon3-11.3.0"));
        assert_eq!(l.game_exe(), Path::new(r"C:\I\work\splatoon3-11.3.0\build\splat3.exe"));
        assert_eq!(l.xlate_exe(), Path::new(r"C:\I\work\xlate\totk_xlate.exe"));
        // the launcher's own rule for the translator finds the wizard's
        assert_eq!(crate::install::translator_path(&l.game(), None), l.xlate_exe());
        assert_eq!(l.venv_py(), Path::new(r"C:\I\work\venv\Scripts\python.exe"));
        assert_eq!(l.hactool(), Path::new(r"C:\I\tools\bin\hactool.exe"));
        assert_eq!(l.hactool_built(), Path::new(r"C:\I\src\toolchain\hactool\hactool.exe"));
        // a first install runs the portable Python; an older install keeps its virtual environment
        assert_eq!(l.python(), Path::new(r"C:\I\tools\python\python.exe"));
        assert_eq!(l.keys_json(), Path::new(r"C:\I\src\kit-tools\profile\s3save.keys.json"));
    }

    fn bg_run(tag: &str) -> (Run, PathBuf) {
        let root = std::env::temp_dir().join(format!("s3l-bg-{tag}-{}", crate::util::random_hex(4)));
        let req = InstallRequest { install_dir: root.display().to_string(), ..Default::default() };
        (Run::new(Installer::default(), req, None, Arc::new(|_| {}), Arc::new(|_| {})), root)
    }

    #[cfg(windows)]
    fn cmd_exe() -> PathBuf {
        std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("System32").join("cmd.exe")
    }

    #[cfg(windows)]
    #[test]
    fn a_step_beside_the_others_is_recorded_when_waited_for() {
        let (mut run, root) = bg_run("ok");
        let after: Box<dyn FnOnce(i32, PathBuf) -> StepResult + Send> = Box::new(|code, _| if code == 0 { Ok(Flow::Done) } else { Err(Fail::new("merge_failed")) });
        let r = run.spawn_background(StepId::Romfs, &cmd_exe(), &["/C".into(), "exit 0".into()], &std::env::temp_dir(), "bg-test", Box::new(|p| p.fraction = Some(0.5)), after);
        assert_eq!(r.unwrap(), Flow::Background);
        assert_eq!(run.bg.len(), 1);
        assert_eq!(run.bg[0].before, StepId::Xlate, "the translator step waits for the merge");
        // neither the lift nor the compile waits for it as a step (the compile waits inside its own work, before the
        // RomFS index and the link: Run::step_compile)
        assert!(run.join_background(Some(StepId::Lift)).is_none());
        assert!(run.join_background(Some(StepId::Compile)).is_none());
        assert_eq!(run.bg.len(), 1);
        // once it has ended it is recorded at the next look, before anything waits for it
        while !run.bg[0].handle.as_ref().is_some_and(|h| h.is_finished()) {
            std::thread::sleep(Duration::from_millis(20));
        }
        run.reap_background();
        assert!(run.bg.is_empty());
        assert!(run.join_background(Some(StepId::Xlate)).is_none());
        assert_eq!(run.state.steps.get("romfs").map(|r| r.status.as_str()), Some("done"));
        assert!(run.state.steps["romfs"].secs < 30.0, "its own duration, not the wait");
        assert!(root.join("logs").join("bg-test.log").is_file());
        sys::remove_all(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_step_waits_inside_its_work_for_the_one_it_names() {
        let (mut run, root) = bg_run("where");
        let after: Box<dyn FnOnce(i32, PathBuf) -> StepResult + Send> = Box::new(|code, _| if code == 0 { Ok(Flow::Done) } else { Err(Fail::new("merge_failed")) });
        run.spawn_background(StepId::Romfs, &cmd_exe(), &["/C".into(), "ping -n 2 127.0.0.1 >NUL".into()], &std::env::temp_dir(), "bg-where", Box::new(|_| {}), after).unwrap();
        // the compile's wait between its objects and its link: only the merge, while it still runs
        assert!(run.join_background_where(&|b: &Background| b.id == StepId::Romfs).is_none());
        assert!(run.bg.is_empty());
        assert_eq!(run.state.steps.get("romfs").map(|r| r.status.as_str()), Some("done"));
        sys::remove_all(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_failure_beside_stops_the_current_command_and_is_the_one_reported() {
        let (mut run, root) = bg_run("fail");
        let after: Box<dyn FnOnce(i32, PathBuf) -> StepResult + Send> = Box::new(|code, log| Err(Fail { log: Some(log), exit_code: Some(code), ..Fail::new("merge_failed") }));
        run.spawn_background(StepId::Romfs, &cmd_exe(), &["/C".into(), "exit 3".into()], &std::env::temp_dir(), "bg-fail", Box::new(|_| {}), after).unwrap();
        // the foreground command (a long one) is stopped as soon as the failure is known
        let t = Instant::now();
        let r = run.exec(StepId::Lift, &cmd_exe(), &["/C".into(), "ping -n 30 127.0.0.1 >NUL".into()], &std::env::temp_dir(), "fg-test", false, &mut |_, _| {}, &mut |_| {});
        assert_eq!(r.unwrap_err().code, "bg_failed");
        assert!(t.elapsed() < Duration::from_secs(20));
        let (outcome, err) = run.join_background(None).unwrap();
        assert_eq!(outcome, "failed");
        let e = err.unwrap();
        assert_eq!((e.step, e.code.as_str(), e.exit_code), (StepId::Romfs, "merge_failed", Some(3)));
        sys::remove_all(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_cancel_stops_the_steps_beside() {
        let (mut run, root) = bg_run("cancel");
        let after: Box<dyn FnOnce(i32, PathBuf) -> StepResult + Send> = Box::new(|_, _| Ok(Flow::Done));
        run.spawn_background(StepId::Romfs, &cmd_exe(), &["/C".into(), "ping -n 60 127.0.0.1 >NUL".into()], &std::env::temp_dir(), "bg-cancel", Box::new(|_| {}), after).unwrap();
        let halted = Arc::new(AtomicBool::new(false));
        let h = halted.clone();
        run.spawn_background_work(
            StepId::Prereqs,
            Box::new(move |halt, _| {
                while !halt.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(20));
                }
                h.store(true, Ordering::SeqCst);
                Err(Fail::cancelled())
            }),
        )
        .unwrap();
        let t = Instant::now();
        run.inst.cancel();
        let (outcome, err) = run.join_background(None).unwrap();
        assert_eq!(outcome, "cancelled");
        assert!(err.is_none() && halted.load(Ordering::SeqCst));
        assert!(t.elapsed() < Duration::from_secs(20), "the ping was killed");
        sys::remove_all(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn the_link_waits_for_the_game_to_be_closed_and_the_wait_is_not_counted_as_work() {
        // A process whose image is the install's `build\splat3.exe` (a copy of ping.exe: it holds the file as the game
        // does, for about three seconds).
        let (mut run, root) = bg_run("game");
        let exe = run.lay.game_exe();
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        let ping = cmd_exe().with_file_name("PING.EXE");
        std::fs::copy(&ping, &exe).unwrap();
        let mut child = std::process::Command::new(&exe).args(["-n", "4", "127.0.0.1"]).stdout(Stdio::null()).spawn().unwrap();
        let t = Instant::now();
        while !run.game_running() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(run.game_running(), "the stand-in game is seen as the game of this install");
        // a game of another folder does not count
        let other = Layout::new(root.join("other"));
        assert_eq!(sys::count_image(&other.game_exe()), 0);
        run.step_started = Instant::now();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        run.emit = Arc::new(move |s: &Snapshot| s2.lock().unwrap().push(s.waiting.clone()));
        run.wait_for_no_game(StepId::Compile).unwrap();
        let waited = t.elapsed();
        assert!(waited >= Duration::from_secs(2), "waited {waited:?}");
        assert!(!run.game_running());
        assert!(seen.lock().unwrap().iter().any(|w| w.as_deref() == Some("game_running")), "the screen says why it waits");
        assert_eq!(run.inst.snapshot().waiting, None);
        assert!(run.step_started.elapsed() < Duration::from_secs(1), "the wait is not the step's work");
        let _ = child.wait();
        sys::remove_all(&root).unwrap();
    }

    #[test]
    fn the_locker_waits_for_the_merge_and_its_estimate_comes_down_to_it() {
        let (mut run, root) = bg_run("data");
        for (id, w) in [(StepId::Checks, 1.0), (StepId::Sources, 4.0), (StepId::Python, 6.0), (StepId::Exefs, 2.0), (StepId::Hactool, 1.0), (StepId::Romfs, 156.0), (StepId::Lift, 205.0), (StepId::Compile, 573.0)] {
            run.weights.insert(id, w);
        }
        // the download beside the first steps does not hold the merge back
        run.weights.insert(StepId::Prereqs, 30.0);
        assert_eq!(run.game_data_eta(None, 0.0, 1.0), Some(1.0 + 4.0 + 6.0 + 2.0 + 1.0 + 156.0));
        // during the Python step with 3 s left of it
        assert_eq!(run.game_data_eta(Some(StepId::Python), 3.0, 1.0), Some(3.0 + 2.0 + 1.0 + 156.0));
        // a PC twice as slow as the reference
        assert_eq!(run.game_data_eta(Some(StepId::Hactool), 0.0, 2.0), Some(312.0));
        assert_eq!(run.game_data_eta(Some(StepId::Romfs), 0.0, 1.0), Some(156.0));
        // once the merge is over: nothing to wait for
        run.game_data = true;
        assert_eq!(run.game_data_eta(Some(StepId::Lift), 10.0, 1.0), None);
        sys::remove_all(&root).ok();
    }

    #[test]
    fn the_installs_folder_is_known_to_the_launcher_as_its_data_becomes_readable() {
        let inst = Installer::default();
        assert!(inst.target_root().is_none() && inst.game_data_root().is_none());
        {
            let mut g = inst.lock();
            g.snap.active = true;
            g.snap.install_dir = Some(r"C:\K".into());
            g.snap.game_root = Some(r"C:\K\work\splatoon3-11.3.0".into());
        }
        assert_eq!(inst.target_root(), Some(PathBuf::from(r"C:\K\work\splatoon3-11.3.0")));
        assert!(inst.game_data_root().is_none(), "not before the merge is complete");
        inst.lock().snap.game_data = true;
        assert_eq!(inst.game_data_root(), Some(PathBuf::from(r"C:\K\work\splatoon3-11.3.0")));
        // a launch of that game waits only while its executable is being replaced
        assert!(!inst.replacing(Path::new(r"C:\K\work\splatoon3-11.3.0")));
        inst.lock().snap.replacing = true;
        assert!(inst.replacing(Path::new(r"c:\k\work\splatoon3-11.3.0\")));
        assert!(!inst.replacing(Path::new(r"C:\Other\work\splatoon3-11.3.0")));
        inst.lock().snap.active = false;
        assert!(!inst.replacing(Path::new(r"C:\K\work\splatoon3-11.3.0")));
    }

    #[test]
    fn jobs_follow_threads_and_memory() {
        // 16 threads and 32 GB: every thread
        assert_eq!(jobs_for(16, 32 << 30), 16);
        // 16 GB: one per 1.25 GB beyond 4 GB
        assert_eq!(jobs_for(16, 16 << 30), 9);
        assert_eq!(jobs_for(4, 64 << 30), 4);
        assert_eq!(jobs_for(8, 6 << 30), 1);
    }

    #[test]
    fn state_records_follow_the_source_commit() {
        let mut s = StateFile::default();
        s.steps.insert("compile".into(), StepRecord { status: "done".into(), commit: "abc".into(), ..Default::default() });
        s.steps.insert("romfs".into(), StepRecord { status: "done".into(), commit: "old".into(), ..Default::default() });
        assert!(s.done_with(StepId::Compile, "abc"));
        assert!(!s.done_with(StepId::Compile, "def"));
        // the RomFS does not depend on the runtime source
        assert!(s.done_with(StepId::Romfs, "def"));
        let p = std::env::temp_dir().join(format!("s3l-state-{}.json", crate::util::random_hex(4)));
        s.save(&p).unwrap();
        assert_eq!(StateFile::load(&p).unwrap(), s);
        std::fs::remove_file(p).unwrap();
    }
}
