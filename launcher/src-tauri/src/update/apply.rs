//! Putting a verified new `Kalamar.exe` in place of the running one, and getting the old one back if the new one does
//! not start. Windows does not let a running program's file be overwritten or deleted, but it lets it be renamed:
//!
//! 1. the verified download is copied beside the program as `Kalamar.exe.new`, flushed and hashed again;
//! 2. the journal (`<data>\update\journal.json`) says `swapping`; `Kalamar.exe` becomes `Kalamar.exe.old` and
//!    `Kalamar.exe.new` becomes `Kalamar.exe` (two renames a few microseconds apart, retried while an antivirus holds the
//!    new file, undone at once if the second one fails); the journal says `trial`;
//! 3. the new program starts with `--update-trial`; the old one hides its window and waits (90 s at most);
//! 4. the new program writes the witness (`witness.json`, its version and process id) once its page is up, and the
//!    journal says `confirmed`: the old process quits and the new one deletes `Kalamar.exe.old` when it can;
//! 5. no witness (the new program exited, crashed or hung): the old process stops it, puts `Kalamar.exe.old` back as
//!    `Kalamar.exe` (the bad one becomes `Kalamar.exe.failed`, deleted later) and shows its window again.
//!
//! When the old process is gone (power cut, killed), the next start of `Kalamar.exe` reads the journal before anything
//! else ([`early_start`]): a new version that already had its trial without a witness puts the old one back itself and
//! starts it. The release's installer, when Kalamar was installed with one, is started instead of all this; a copy run
//! from a temporary folder (an archive) or from a folder it cannot write saves the new version where the player says.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const TRIAL_ARG: &str = "--update-trial";
/// How long the old process waits for the new one's witness: WebView2 and an antivirus scanning a new program can take a
/// while on a slow PC.
pub const TRIAL_TIMEOUT: Duration = Duration::from_secs(90);
/// Written by Tauri's NSIS installer beside the program it installs.
pub const NSIS_UNINSTALLER: &str = "uninstall.exe";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Journal {
    /// The two renames are under way.
    Swapping { exe: String, from: String, to: String },
    /// The new version is in place, on trial: confirmed by its witness, or rolled back.
    Trial { exe: String, from: String, to: String, attempts: u32 },
    /// The new version started (witness written).
    Confirmed { exe: String, from: String, to: String },
    /// The new version did not start; the old one is back.
    RolledBack { exe: String, from: String, to: String, reason: String, shown: bool },
    /// The release's installer was started; the next start tells whether it ran.
    InstallerStarted { from: String, to: String },
    /// Finished.
    Done { from: String, to: String, shown: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// The file does not match the signed size and SHA-256.
    Integrity,
    Disk(String),
    /// A file stays locked (an antivirus scanning it, another program holding it).
    Locked(String),
    NotWritable(String),
    Spawn(String),
    /// Another Kalamar runs from this file, or a previous update's file is still in use.
    Busy(String),
    Io(String),
}

impl ApplyError {
    pub fn code(&self) -> &'static str {
        match self {
            ApplyError::Integrity => "integrity",
            ApplyError::Disk(_) => "disk",
            ApplyError::Locked(_) => "locked",
            ApplyError::NotWritable(_) => "not_writable",
            ApplyError::Spawn(_) => "spawn",
            ApplyError::Busy(_) => "other_instance",
            ApplyError::Io(_) => "io",
        }
    }
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

fn io_err(e: &std::io::Error) -> ApplyError {
    match e.raw_os_error() {
        Some(39 | 112) => ApplyError::Disk(e.to_string()),
        Some(5 | 32 | 33) => ApplyError::Locked(e.to_string()),
        _ if e.kind() == std::io::ErrorKind::PermissionDenied => ApplyError::NotWritable(e.to_string()),
        _ => ApplyError::Io(e.to_string()),
    }
}

/// The files beside the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub exe: PathBuf,
    pub new: PathBuf,
    pub old: PathBuf,
    pub failed: PathBuf,
}

impl Paths {
    pub fn of(exe: &Path) -> Paths {
        let with = |ext: &str| {
            let mut s = exe.as_os_str().to_os_string();
            s.push(ext);
            PathBuf::from(s)
        };
        Paths { exe: exe.to_path_buf(), new: with(".new"), old: with(".old"), failed: with(".failed") }
    }
}

// ---- small file helpers ---------------------------------------------------------------------------------------------

pub fn sha256_file(p: &Path) -> std::io::Result<String> {
    use sha2::Digest;
    let mut f = std::fs::File::open(p)?;
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

/// The file has the signed size and SHA-256.
pub fn verify_file(p: &Path, sha256: &str, size: u64) -> Result<(), ApplyError> {
    let len = std::fs::metadata(p).map_err(|e| io_err(&e))?.len();
    if len != size {
        return Err(ApplyError::Integrity);
    }
    let got = sha256_file(p).map_err(|e| io_err(&e))?;
    if got != sha256 { Err(ApplyError::Integrity) } else { Ok(()) }
}

/// Opens `p` for reading so that nothing can write to it while the handle lives (Windows: other opens may read, and
/// rename or delete when `allow_delete`, never write). What is checked through this handle is what runs: a file
/// cannot change between its check and its start.
pub fn open_locked(p: &Path, allow_delete: bool) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        const FILE_SHARE_DELETE: u32 = 4;
        o.share_mode(if allow_delete { FILE_SHARE_READ | FILE_SHARE_DELETE } else { FILE_SHARE_READ });
    }
    #[cfg(not(windows))]
    let _ = allow_delete;
    o.open(p)
}

fn sha256_of(f: &mut std::fs::File) -> std::io::Result<String> {
    use sha2::Digest;
    use std::io::Seek;
    f.seek(std::io::SeekFrom::Start(0))?;
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

/// [`verify_file`] through a handle that keeps the file from being written ([`open_locked`]); the handle is returned
/// so that the caller holds it until the file has been started.
pub fn verify_locked(p: &Path, sha256: &str, size: u64, allow_delete: bool) -> Result<std::fs::File, ApplyError> {
    let mut f = open_locked(p, allow_delete).map_err(|e| io_err(&e))?;
    if f.metadata().map_err(|e| io_err(&e))?.len() != size {
        return Err(ApplyError::Integrity);
    }
    if sha256_of(&mut f).map_err(|e| io_err(&e))? != sha256 {
        return Err(ApplyError::Integrity);
    }
    Ok(f)
}

/// The two handles are the same file (volume and file index), whatever its name now.
pub fn same_handle_file(a: &std::fs::File, b: &std::fs::File) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
        let id = |f: &std::fs::File| {
            // SAFETY: a valid open handle and a structure of the size the call writes.
            let mut i: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            let ok = unsafe { GetFileInformationByHandle(f.as_raw_handle() as _, &mut i) } != 0;
            ok.then_some((i.dwVolumeSerialNumber, i.nFileIndexHigh, i.nFileIndexLow))
        };
        matches!((id(a), id(b)), (Some(x), Some(y)) if x == y)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::MetadataExt;
        matches!((a.metadata(), b.metadata()), (Ok(x), Ok(y)) if x.dev() == y.dev() && x.ino() == y.ino())
    }
}

fn transient(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(5 | 32 | 33))
}

/// Retries an operation that an antivirus or the indexer may block for a moment (access denied, sharing violation).
pub fn retry<T>(mut f: impl FnMut() -> std::io::Result<T>, tries: u32, wait: Duration) -> std::io::Result<T> {
    let mut n = 0;
    loop {
        match f() {
            Err(e) if transient(&e) && n + 1 < tries => {
                n += 1;
                std::thread::sleep(wait);
            }
            r => return r,
        }
    }
}

pub fn rename_retry(a: &Path, b: &Path) -> std::io::Result<()> {
    retry(|| std::fs::rename(a, b), 24, Duration::from_millis(250))
}

/// Removes a file (gone already is fine), retrying a short while.
pub fn remove_retry(p: &Path, tries: u32) -> std::io::Result<()> {
    retry(
        || match std::fs::remove_file(p) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        },
        tries,
        Duration::from_millis(250),
    )
}

/// Two paths name the same file (case and short names aside).
pub fn same_file(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase();
    norm(a) == norm(b)
}

// ---- journal and witness ----------------------------------------------------------------------------------------------

pub fn journal_path(data: &Path) -> PathBuf {
    data.join("journal.json")
}

pub fn witness_path(data: &Path) -> PathBuf {
    data.join("witness.json")
}

pub fn load_journal(data: &Path) -> Option<Journal> {
    let t = crate::util::read_text_limited(&journal_path(data), 64 * 1024).ok().flatten()?;
    serde_json::from_str(&t).ok()
}

pub fn save_journal(data: &Path, j: &Journal) -> Result<(), ApplyError> {
    let text = serde_json::to_vec_pretty(j).map_err(|e| ApplyError::Io(e.to_string()))?;
    crate::util::atomic_write(&journal_path(data), &text).map_err(|e| ApplyError::Io(e.to_string()))
}

pub fn clear_journal(data: &Path) {
    let _ = std::fs::remove_file(journal_path(data));
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Witness {
    version: String,
    pid: u32,
    at: u64,
}

fn witness_ok(data: &Path, version: &str, pid: u32) -> bool {
    let Ok(Some(t)) = crate::util::read_text_limited(&witness_path(data), 4096) else { return false };
    serde_json::from_str::<Witness>(&t).is_ok_and(|w| w.version == version && w.pid == pid)
}

// ---- the replacement ------------------------------------------------------------------------------------------------------

/// Copies the verified download beside the program as `Kalamar.exe.new`, flushes it and checks it again through a
/// handle that keeps it from being written ([`verify_locked`], renaming allowed: [`swap`] renames it); hold the handle
/// until the new program has started ([`lock_in_place`]).
pub fn prepare(staged: &Path, sha256: &str, size: u64, p: &Paths) -> Result<std::fs::File, ApplyError> {
    verify_file(staged, sha256, size)?;
    if let Some(free) = p.exe.parent().and_then(crate::installer::sys::free_bytes)
        && free < size + 16 * 1024 * 1024
    {
        return Err(ApplyError::Disk(format!("{free} bytes free")));
    }
    remove_retry(&p.new, 8).map_err(|e| io_err(&e))?;
    let copied = std::fs::copy(staged, &p.new).and_then(|_| std::fs::OpenOptions::new().write(true).open(&p.new)?.sync_all());
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&p.new);
        return Err(io_err(&e));
    }
    #[cfg(unix)]
    {
        // A downloaded program keeps the download's mode (no execute bit): the new version must be runnable.
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(staged).map(|m| m.permissions().mode()).unwrap_or(0o644);
        if let Err(e) = std::fs::set_permissions(&p.new, std::fs::Permissions::from_mode(mode | 0o111)) {
            let _ = std::fs::remove_file(&p.new);
            return Err(io_err(&e));
        }
    }
    match verify_locked(&p.new, sha256, size, true) {
        Ok(guard) => Ok(guard),
        Err(e) => {
            let _ = std::fs::remove_file(&p.new);
            Err(e)
        }
    }
}

/// After [`swap`]: the program now at `p.exe` is the file `checked` (the one verified, not another put under its name
/// meanwhile), held so that nothing can write, rename or delete it until it has been started. `Integrity` otherwise.
pub fn lock_in_place(p: &Paths, checked: &std::fs::File) -> Result<std::fs::File, ApplyError> {
    let now = open_locked(&p.exe, false).map_err(|e| io_err(&e))?;
    if same_handle_file(&now, checked) { Ok(now) } else { Err(ApplyError::Integrity) }
}

/// The two renames, journaled. On success the new program is in place and on trial.
pub fn swap(p: &Paths, data: &Path, from: &str, to: &str) -> Result<(), ApplyError> {
    // An old file left by an earlier update: removed now, unless something still runs it.
    if p.old.exists() {
        remove_retry(&p.old, 8).map_err(|e| ApplyError::Busy(format!("{}: {e}", p.old.display())))?;
    }
    let exe = p.exe.display().to_string();
    save_journal(data, &Journal::Swapping { exe: exe.clone(), from: from.into(), to: to.into() })?;
    if let Err(e) = rename_retry(&p.exe, &p.old) {
        clear_journal(data);
        let _ = std::fs::remove_file(&p.new);
        return Err(io_err(&e));
    }
    if let Err(e) = rename_retry(&p.new, &p.exe) {
        // Put the running program's file back at once; the journal stays until it is.
        if retry(|| std::fs::rename(&p.old, &p.exe), 80, Duration::from_millis(250)).is_ok() {
            clear_journal(data);
        }
        let _ = std::fs::remove_file(&p.new);
        return Err(io_err(&e));
    }
    let _ = save_journal(data, &Journal::Trial { exe, from: from.into(), to: to.into(), attempts: 0 });
    Ok(())
}

/// `Kalamar.exe.old` back as `Kalamar.exe`; the version that failed becomes `Kalamar.exe.failed` (deleted when nothing
/// runs it).
pub fn restore(p: &Paths) -> Result<(), ApplyError> {
    if !p.old.is_file() {
        return Err(ApplyError::Io(format!("{} is missing", p.old.display())));
    }
    let _ = remove_retry(&p.failed, 4);
    if p.exe.exists() {
        rename_retry(&p.exe, &p.failed).map_err(|e| io_err(&e))?;
    }
    retry(|| std::fs::rename(&p.old, &p.exe), 80, Duration::from_millis(250)).map_err(|e| io_err(&e))?;
    let _ = remove_retry(&p.failed, 2);
    Ok(())
}

/// Starts the program at `exe` for its trial.
pub fn spawn_trial(exe: &Path) -> std::io::Result<Child> {
    let mut c = Command::new(exe);
    c.arg(TRIAL_ARG).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(d) = exe.parent() {
        c.current_dir(d);
    }
    c.spawn()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrialOutcome {
    Started,
    /// The new program exited before its witness (its exit code, when it had one).
    Exited(Option<i32>),
    /// No witness in time: it was stopped.
    TimedOut,
}

impl TrialOutcome {
    pub fn reason(&self) -> String {
        match self {
            TrialOutcome::Started => "started".into(),
            TrialOutcome::Exited(c) => format!("exited ({})", c.map(|c| c.to_string()).unwrap_or_else(|| "?".into())),
            TrialOutcome::TimedOut => "did not answer".into(),
        }
    }
}

/// Waits for the trial's witness (the old process does this, window hidden). A program that hangs is stopped.
pub fn watch(child: &mut Child, data: &Path, to: &str, timeout: Duration, poll: Duration) -> TrialOutcome {
    let pid = child.id();
    let start = Instant::now();
    loop {
        if witness_ok(data, to, pid) {
            return TrialOutcome::Started;
        }
        if let Ok(Some(st)) = child.try_wait() {
            return if witness_ok(data, to, pid) { TrialOutcome::Started } else { TrialOutcome::Exited(st.code()) };
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return if witness_ok(data, to, pid) { TrialOutcome::Started } else { TrialOutcome::TimedOut };
        }
        std::thread::sleep(poll);
    }
}

/// After a failed trial (old process): the old program back in place, the journal says so.
pub fn roll_back(p: &Paths, data: &Path, from: &str, to: &str, reason: &str) -> Result<(), ApplyError> {
    restore(p)?;
    let _ = save_journal(data, &Journal::RolledBack { exe: p.exe.display().to_string(), from: from.into(), to: to.into(), reason: reason.into(), shown: true });
    let _ = std::fs::remove_file(witness_path(data));
    Ok(())
}

/// The new program's page is up: writes the witness (the old process then quits). The versions (from, to) when this
/// run was a trial.
pub fn confirm(data: &Path, exe: &Path, version: &str, pid: u32) -> Option<(String, String)> {
    match load_journal(data) {
        Some(Journal::Trial { exe: j, from, to, .. }) if to == version && same_file(Path::new(&j), exe) => {
            let w = Witness { version: version.into(), pid, at: crate::util::now_secs() };
            let ok = serde_json::to_vec(&w).ok().is_some_and(|b| crate::util::atomic_write(&witness_path(data), &b).is_ok());
            if !ok {
                return None;
            }
            let _ = save_journal(data, &Journal::Confirmed { exe: j, from: from.clone(), to: to.clone() });
            Some((from, to))
        }
        _ => None,
    }
}

/// After the witness: the old program's file goes once its process has quit (it does right after reading the witness).
pub fn finish_confirmed(data: &Path, exe: &Path, version: &str, wait: Duration) {
    let Some(Journal::Confirmed { from, to, .. }) = load_journal(data) else { return };
    if to != version {
        return;
    }
    let p = Paths::of(exe);
    let start = Instant::now();
    while p.old.exists() && std::fs::remove_file(&p.old).is_err() && start.elapsed() < wait {
        std::thread::sleep(Duration::from_millis(500));
    }
    let _ = std::fs::remove_file(&p.failed);
    // The witness stays until a later start (`cleanup_leftovers`): the old process may not have read it yet.
    let _ = save_journal(data, &Journal::Done { from, to, shown: true });
}

/// What the start-up check decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Early {
    Continue,
    /// The previous version was put back and started: this process quits at once.
    Exit,
}

/// Run first thing at start, before the window: finishes or undoes an interrupted update. `relaunch` starts the
/// program at the given path (the old version put back).
pub fn early_start(data: &Path, exe: &Path, version: &str, trial: bool, relaunch: &dyn Fn(&Path) -> std::io::Result<()>) -> Early {
    let Some(j) = load_journal(data) else { return Early::Continue };
    let p = Paths::of(exe);
    match j {
        // Interrupted between the renames: this program is the new one in place (both renames done) ...
        Journal::Swapping { exe: j_exe, from, to } if to == version && same_file(Path::new(&j_exe), exe) => {
            trial_start(data, &p, j_exe, from, to, 0, trial, relaunch)
        }
        // ... or the old one, still in place: the update did not happen.
        Journal::Swapping { from, .. } if from == version => {
            let _ = std::fs::remove_file(&p.new);
            clear_journal(data);
            Early::Continue
        }
        Journal::Trial { exe: j_exe, from, to, attempts } if to == version && same_file(Path::new(&j_exe), exe) => {
            trial_start(data, &p, j_exe, from, to, attempts, trial, relaunch)
        }
        // Another program's or version's journal, or a finished one: read later by `take_notice` and the cleanup.
        _ => Early::Continue,
    }
}

#[allow(clippy::too_many_arguments)]
fn trial_start(data: &Path, p: &Paths, exe: String, from: String, to: String, attempts: u32, trial: bool, relaunch: &dyn Fn(&Path) -> std::io::Result<()>) -> Early {
    // Started by the old process, which watches: just count the attempt.
    // Started by hand with no witness after an earlier attempt: that attempt failed and nobody undid it; undo it now.
    if !trial && attempts >= 1 && p.old.is_file() && restore(p).is_ok() {
        let _ = save_journal(data, &Journal::RolledBack { exe, from, to, reason: "did not start".into(), shown: false });
        let _ = relaunch(&p.exe);
        return Early::Exit;
    }
    let _ = save_journal(data, &Journal::Trial { exe, from, to, attempts: attempts + 1 });
    Early::Continue
}

/// Leftovers of earlier updates beside the program, removed when no update is under way (each removal tried once: a
/// file another Kalamar still runs stays until the next start).
pub fn cleanup_leftovers(data: &Path, exe: &Path) {
    if matches!(load_journal(data), Some(Journal::Swapping { .. } | Journal::Trial { .. } | Journal::Confirmed { .. })) {
        return;
    }
    let p = Paths::of(exe);
    for f in [&p.old, &p.failed, &p.new] {
        let _ = std::fs::remove_file(f);
    }
    let _ = std::fs::remove_file(witness_path(data));
}

/// What to tell the player once about the last update, read at start: (`updated` | `rolled_back` |
/// `installer_unfinished`, from, to).
pub fn take_notice(data: &Path, version: &str) -> Option<(&'static str, String, String)> {
    match load_journal(data)? {
        Journal::RolledBack { exe, from, to, reason, shown: false } if from == version => {
            let _ = save_journal(data, &Journal::RolledBack { exe, from: from.clone(), to: to.clone(), reason, shown: true });
            Some(("rolled_back", from, to))
        }
        Journal::Done { from, to, shown: false } if to == version => {
            let _ = save_journal(data, &Journal::Done { from: from.clone(), to: to.clone(), shown: true });
            Some(("updated", from, to))
        }
        Journal::InstallerStarted { from, to } => {
            if to == version {
                let _ = save_journal(data, &Journal::Done { from: from.clone(), to: to.clone(), shown: true });
                Some(("updated", from, to))
            } else if from == version {
                clear_journal(data);
                Some(("installer_unfinished", from, to))
            } else {
                clear_journal(data);
                None
            }
        }
        _ => None,
    }
}

// ---- where the program runs from ------------------------------------------------------------------------------------------

/// Where the running program is, for choosing how to update it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    /// Installed by the NSIS installer (its uninstaller is beside it).
    pub installed: bool,
    /// Under a temporary folder: most likely opened from inside an archive.
    pub temp: bool,
    /// Its folder accepts a new file.
    pub writable: bool,
}

/// The temporary folders of this session and of the system.
pub fn temp_roots() -> Vec<PathBuf> {
    let mut v = vec![std::env::temp_dir()];
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        v.push(PathBuf::from(l).join("Temp"));
    }
    if let Some(w) = std::env::var_os("SystemRoot") {
        v.push(PathBuf::from(w).join("Temp"));
    }
    v
}

fn norm(p: &Path) -> String {
    let s = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase().replace('/', "\\");
    if s.ends_with('\\') { s } else { s + "\\" }
}

pub fn under_any(p: &Path, roots: &[PathBuf]) -> bool {
    let me = norm(p);
    roots.iter().any(|r| me.starts_with(&norm(r)))
}

pub fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".kalamar-write-test-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(f) => {
            drop(f);
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn place(exe: &Path, temp: &[PathBuf]) -> Place {
    let dir = exe.parent().unwrap_or(Path::new("."));
    Place { installed: dir.join(NSIS_UNINSTALLER).is_file(), temp: under_any(exe, temp), writable: dir_writable(dir) }
}

/// Saves the verified new program in a folder the player chose (the running copy cannot replace itself there):
/// `Kalamar.exe` if the name is free, else `Kalamar-<version>.exe`. Returns the new file.
pub fn save_elsewhere(staged: &Path, sha256: &str, size: u64, dir: &Path, version: &str, temp: &[PathBuf]) -> Result<PathBuf, ApplyError> {
    if !dir.is_absolute() || !dir.is_dir() {
        return Err(ApplyError::Io("not a folder".into()));
    }
    if under_any(dir, temp) {
        return Err(ApplyError::NotWritable("a temporary folder".into()));
    }
    verify_file(staged, sha256, size)?;
    let name = crate::installer::exe_name("Kalamar");
    let target = [dir.join(&name), dir.join(format!("Kalamar-{version}{}", crate::installer::EXE_SUFFIX))].into_iter().find(|t| !t.exists()).ok_or_else(|| ApplyError::Busy("both names are taken".into()))?;
    let tmp = dir.join(format!(".Kalamar-{version}{}.part-{}", crate::installer::EXE_SUFFIX, std::process::id()));
    let copied = std::fs::copy(staged, &tmp).and_then(|_| std::fs::OpenOptions::new().write(true).open(&tmp)?.sync_all());
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_err(&e));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(staged).map(|m| m.permissions().mode()).unwrap_or(0o644);
        if let Err(e) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode | 0o111)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(io_err(&e));
        }
    }
    if let Err(e) = verify_file(&tmp, sha256, size) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if target.exists() {
        let _ = std::fs::remove_file(&tmp);
        return Err(ApplyError::Busy("the name was just taken".into()));
    }
    rename_retry(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io_err(&e)
    })?;
    Ok(target)
}

// ---- Windows: other instances, the installer, the folder ------------------------------------------------------------------------

/// Other processes running this very program file (not this one).
pub fn other_instances(exe: &Path) -> Vec<u32> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
        let me = std::process::id();
        let want = exe.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let mut found = Vec::new();
        // SAFETY: the snapshot handle is checked and closed; the entry is sized as the API requires; each process handle
        // is checked and closed; the name buffer's length is passed in and read back.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() || snap == INVALID_HANDLE_VALUE {
                return found;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                if e.th32ProcessID != me && String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase() == want {
                    let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, e.th32ProcessID);
                    if !h.is_null() {
                        let mut buf = [0u16; 1024];
                        let mut n = buf.len() as u32;
                        if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut n) != 0 {
                            let path = PathBuf::from(String::from_utf16_lossy(&buf[..n as usize]));
                            if same_file(&path, exe) {
                                found.push(e.th32ProcessID);
                            }
                        }
                        CloseHandle(h);
                    }
                }
                ok = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
        }
        found
    }
    #[cfg(target_os = "linux")]
    {
        let me = std::process::id();
        let want = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
        let mut found = Vec::new();
        if let Ok(rd) = std::fs::read_dir("/proc") {
            for e in rd.flatten() {
                let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
                if pid != me && std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|p| p == want) {
                    found.push(pid);
                }
            }
        }
        found
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = exe;
        Vec::new()
    }
}

/// Opens the verified installer the way Explorer would (so that it can ask for elevation itself).
pub fn open_installer(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let verb = wide(std::ffi::OsStr::new("open"));
        let file = wide(path.as_os_str());
        let dir = wide(path.parent().unwrap_or(Path::new(".")).as_os_str());
        // SAFETY: every pointer is a NUL-terminated UTF-16 buffer alive for the call; no window handle.
        let r = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), dir.as_ptr(), SW_SHOWNORMAL) };
        if r as isize > 32 { Ok(()) } else { Err(format!("the installer did not start (ShellExecute {})", r as isize)) }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("not supported".into())
    }
}

/// Shows a file selected in Explorer (the folder a new version was saved to).
pub fn show_in_folder(file: &Path) {
    #[cfg(windows)]
    {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(file.as_os_str());
        let _ = Command::new(crate::winutil::windows_exe("explorer.exe")).arg(arg).spawn();
    }
    #[cfg(not(windows))]
    let _ = file;
}

/// Held while this launcher downloads or applies an update: a second launcher does not do the same at the same time.
pub struct Lock(#[allow(dead_code)] std::fs::File);

pub fn try_lock(data: &Path) -> Option<Lock> {
    std::fs::create_dir_all(data).ok()?;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.share_mode(0);
    }
    o.open(data.join("lock")).ok().map(Lock)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The release's secret key is never read by `release_sign`, which links the whole launcher and its dependencies:
    /// only the official minisign tool reads it.
    #[test]
    fn no_program_of_this_tree_reads_the_signing_key() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let tool = std::fs::read_to_string(root.join("examples").join("release_sign.rs")).unwrap();
        assert!(!tool.contains("SecretKey") && !tool.contains("generate_and_write_encrypted_keypair"));
        let script = std::fs::read_to_string(root.parent().unwrap().join("scripts").join("make-release.ps1")).unwrap();
        assert!(script.contains("& $MinisignFull -S -s $KeyFull") && !script.contains("release_sign -- sign"));
    }

    /// Between the check of a file and its start, the same user could write another program in its place. Checked through a held handle, it cannot be written, and a file put under its name is noticed.
    #[cfg(windows)]
    #[test]
    fn a_checked_file_cannot_change_before_it_starts() {
        let d = temp("locked");
        let f = d.join("setup.exe");
        std::fs::write(&f, b"MZ signed").unwrap();
        assert!(matches!(verify_locked(&f, &sha(b"other"), 9, false), Err(ApplyError::Integrity)));
        let held = verify_locked(&f, &sha(b"MZ signed"), 9, false).unwrap();
        assert!(std::fs::OpenOptions::new().write(true).open(&f).is_err(), "written while held");
        assert!(std::fs::rename(&f, d.join("moved.exe")).is_err(), "renamed while held");
        assert!(std::fs::read(&f).is_ok(), "still readable (the system starts it)");
        drop(held);
        assert!(std::fs::OpenOptions::new().write(true).open(&f).is_ok());
        // Swap mode: the checked `.new` may be renamed into place, but a file put under the program's name instead is not
        // the checked one.
        let p = Paths::of(&d.join("Kalamar.exe"));
        std::fs::write(&p.new, b"MZ new").unwrap();
        let checked = verify_locked(&p.new, &sha(b"MZ new"), 6, true).unwrap();
        assert!(std::fs::OpenOptions::new().write(true).open(&p.new).is_err());
        std::fs::rename(&p.new, &p.exe).unwrap();
        let held = lock_in_place(&p, &checked).unwrap();
        assert!(std::fs::rename(&p.exe, d.join("away.exe")).is_err(), "the program in place cannot be swapped any more");
        drop(held);
        std::fs::rename(&p.exe, d.join("away.exe")).unwrap();
        std::fs::write(&p.exe, b"MZ planted").unwrap();
        assert!(matches!(lock_in_place(&p, &checked), Err(ApplyError::Integrity)));
        drop(checked);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("s3l-upd-{tag}-{}", crate::util::random_hex(6)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sha(b: &[u8]) -> String {
        crate::installer::bundle::sha256_hex(b)
    }

    #[test]
    fn prepare_checks_the_copy_and_swap_keeps_the_old_file() {
        let d = temp("swap");
        let (app, data) = (d.join("app"), d.join("data"));
        std::fs::create_dir_all(&app).unwrap();
        let p = Paths::of(&app.join("Kalamar.exe"));
        std::fs::write(&p.exe, b"old program").unwrap();
        let staged = d.join("staged.exe");
        std::fs::write(&staged, b"new program").unwrap();
        // a wrong hash or size: nothing is copied
        assert_eq!(prepare(&staged, &sha(b"other"), 11, &p).err(), Some(ApplyError::Integrity));
        assert_eq!(prepare(&staged, &sha(b"new program"), 12, &p).err(), Some(ApplyError::Integrity));
        assert!(!p.new.exists());
        drop(prepare(&staged, &sha(b"new program"), 11, &p).unwrap());
        assert_eq!(std::fs::read(&p.new).unwrap(), b"new program");
        swap(&p, &data, "0.1.0", "0.2.0").unwrap();
        assert_eq!(std::fs::read(&p.exe).unwrap(), b"new program");
        assert_eq!(std::fs::read(&p.old).unwrap(), b"old program");
        assert!(!p.new.exists());
        assert!(matches!(load_journal(&data), Some(Journal::Trial { attempts: 0, .. })));
        // and back
        restore(&p).unwrap();
        assert_eq!(std::fs::read(&p.exe).unwrap(), b"old program");
        assert!(!p.old.exists() && !p.failed.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_second_rename_puts_the_program_back() {
        let d = temp("swapfail");
        let data = d.join("data");
        let p = Paths::of(&d.join("Kalamar.exe"));
        std::fs::write(&p.exe, b"old").unwrap();
        // no .new file: the second rename fails
        assert!(swap(&p, &data, "0.1.0", "0.2.0").is_err());
        assert_eq!(std::fs::read(&p.exe).unwrap(), b"old");
        assert!(!p.old.exists());
        assert_eq!(load_journal(&data), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_interrupted_or_unwatched_trial_is_finished_or_undone_at_the_next_start() {
        let d = temp("early");
        let data = d.join("data");
        let p = Paths::of(&d.join("Kalamar.exe"));
        let started = std::cell::RefCell::new(Vec::new());
        let relaunch = |x: &Path| {
            started.borrow_mut().push(x.to_path_buf());
            Ok(())
        };
        // the power went off after both renames: the new program starts by hand, that run is its trial
        std::fs::write(&p.exe, b"new").unwrap();
        std::fs::write(&p.old, b"old").unwrap();
        save_journal(&data, &Journal::Swapping { exe: p.exe.display().to_string(), from: "0.1.0".into(), to: "0.2.0".into() }).unwrap();
        assert_eq!(early_start(&data, &p.exe, "0.2.0", false, &relaunch), Early::Continue);
        assert!(matches!(load_journal(&data), Some(Journal::Trial { attempts: 1, .. })));
        // it died before its witness; started by hand again: the old one comes back and is started
        assert_eq!(early_start(&data, &p.exe, "0.2.0", false, &relaunch), Early::Exit);
        assert_eq!(std::fs::read(&p.exe).unwrap(), b"old");
        assert_eq!(started.borrow().as_slice(), std::slice::from_ref(&p.exe));
        assert!(matches!(load_journal(&data), Some(Journal::RolledBack { shown: false, .. })));
        // the old version says so once
        assert_eq!(take_notice(&data, "0.1.0"), Some(("rolled_back", "0.1.0".into(), "0.2.0".into())));
        assert_eq!(take_notice(&data, "0.1.0"), None);
        cleanup_leftovers(&data, &p.exe);
        assert!(!p.failed.exists() && !p.old.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_watched_trial_only_counts_and_the_witness_confirms_it() {
        let d = temp("watched");
        let data = d.join("data");
        let p = Paths::of(&d.join("Kalamar.exe"));
        std::fs::write(&p.exe, b"new").unwrap();
        std::fs::write(&p.old, b"old").unwrap();
        let exe = p.exe.display().to_string();
        save_journal(&data, &Journal::Trial { exe: exe.clone(), from: "0.1.0".into(), to: "0.2.0".into(), attempts: 3 }).unwrap();
        let never = |_: &Path| -> std::io::Result<()> { panic!("no relaunch while watched") };
        assert_eq!(early_start(&data, &p.exe, "0.2.0", true, &never), Early::Continue);
        assert!(matches!(load_journal(&data), Some(Journal::Trial { attempts: 4, .. })));
        // the old file stays while the trial runs
        cleanup_leftovers(&data, &p.exe);
        assert!(p.old.exists());
        assert_eq!(confirm(&data, &p.exe, "0.2.0", 4242), Some(("0.1.0".into(), "0.2.0".into())));
        assert!(witness_ok(&data, "0.2.0", 4242) && !witness_ok(&data, "0.2.0", 1) && !witness_ok(&data, "0.3.0", 4242));
        finish_confirmed(&data, &p.exe, "0.2.0", Duration::from_secs(2));
        assert!(!p.old.exists());
        assert!(matches!(load_journal(&data), Some(Journal::Done { shown: true, .. })));
        // a later start of another version, or of the same one, is not a trial any more
        assert_eq!(confirm(&data, &p.exe, "0.2.0", 1), None);
        assert_eq!(early_start(&data, &p.exe, "0.2.0", false, &never), Early::Continue);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_journal_of_another_program_or_version_changes_nothing() {
        let d = temp("other");
        let data = d.join("data");
        let p = Paths::of(&d.join("Kalamar.exe"));
        std::fs::write(&p.exe, b"x").unwrap();
        std::fs::write(&p.old, b"keep").unwrap();
        let never = |_: &Path| -> std::io::Result<()> { panic!("no relaunch") };
        save_journal(&data, &Journal::Trial { exe: d.join("Elsewhere.exe").display().to_string(), from: "0.1.0".into(), to: "0.2.0".into(), attempts: 5 }).unwrap();
        assert_eq!(early_start(&data, &p.exe, "0.2.0", false, &never), Early::Continue);
        assert_eq!(early_start(&data, &p.exe, "0.3.0", false, &never), Early::Continue);
        assert!(p.old.exists(), "nothing restored for another program");
        // the old version still in place after a cut before the renames
        save_journal(&data, &Journal::Swapping { exe: p.exe.display().to_string(), from: "0.1.0".into(), to: "0.2.0".into() }).unwrap();
        std::fs::write(&p.new, b"half").unwrap();
        assert_eq!(early_start(&data, &p.exe, "0.1.0", false, &never), Early::Continue);
        assert!(!p.new.exists() && load_journal(&data).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_installer_outcome_is_told_once() {
        let d = temp("inst");
        save_journal(&d, &Journal::InstallerStarted { from: "0.1.0".into(), to: "0.2.0".into() }).unwrap();
        assert_eq!(take_notice(&d, "0.2.0"), Some(("updated", "0.1.0".into(), "0.2.0".into())));
        assert_eq!(take_notice(&d, "0.2.0"), None);
        save_journal(&d, &Journal::InstallerStarted { from: "0.1.0".into(), to: "0.2.0".into() }).unwrap();
        assert_eq!(take_notice(&d, "0.1.0"), Some(("installer_unfinished", "0.1.0".into(), "0.2.0".into())));
        assert_eq!(load_journal(&d), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn places() {
        let d = temp("place");
        let exe = d.join("Kalamar.exe");
        std::fs::write(&exe, b"x").unwrap();
        let pl = place(&exe, &[]);
        assert!(pl.writable && !pl.installed && !pl.temp);
        assert!(place(&exe, std::slice::from_ref(&d)).temp, "under a temporary root");
        assert!(place(&exe, &temp_roots()).temp, "this test folder is under %TEMP%");
        std::fs::write(d.join(NSIS_UNINSTALLER), b"x").unwrap();
        assert!(place(&exe, &[]).installed);
        assert!(!under_any(&exe, &[d.join("Kal")]), "a prefix of a name is not a parent folder");
        assert!(!dir_writable(&d.join("missing")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn saving_elsewhere_never_overwrites() {
        let d = temp("else");
        let staged = d.join("staged");
        std::fs::write(&staged, b"new").unwrap();
        let out = d.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let first = save_elsewhere(&staged, &sha(b"new"), 3, &out, "0.2.0", &[]).unwrap();
        assert_eq!(first, out.join(crate::installer::exe_name("Kalamar")));
        let second = save_elsewhere(&staged, &sha(b"new"), 3, &out, "0.2.0", &[]).unwrap();
        assert_eq!(second, out.join(format!("Kalamar-0.2.0{}", crate::installer::EXE_SUFFIX)));
        assert!(matches!(save_elsewhere(&staged, &sha(b"new"), 3, &out, "0.2.0", &[]), Err(ApplyError::Busy(_))));
        assert_eq!(save_elsewhere(&staged, &sha(b"bad"), 3, &d, "0.2.0", &[]), Err(ApplyError::Integrity));
        assert!(matches!(save_elsewhere(&staged, &sha(b"new"), 3, &out, "0.2.0", std::slice::from_ref(&d)), Err(ApplyError::NotWritable(_))));
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 2, "no temporary file left");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn one_lock_at_a_time() {
        let d = temp("lock");
        let a = try_lock(&d);
        assert!(a.is_some());
        #[cfg(windows)]
        assert!(try_lock(&d).is_none(), "a second holder is refused");
        drop(a);
        assert!(try_lock(&d).is_some());
        let _ = std::fs::remove_dir_all(&d);
    }
}
