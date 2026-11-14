//! Where the launcher keeps its data, and the one-time move of an earlier launcher's folder.
//!
//! The data folder is `%APPDATA%\dev.kalamar.launcher` (the Tauri identifier, `IDENTIFIER`), or the folder named by
//! `KALAMAR_DATA` (the older `SPLAT3_LAUNCHER_DATA` is still read), which is never moved. Launchers before this one used
//! the identifier `dev.splat3recomp.launcher`; the first start of this one takes that folder over, before anything
//! else reads it (the check of an update on trial included, `lib::run`):
//!
//! | found at start | what happens |
//! |---|---|
//! | the new folder only, or neither | the new folder (created when needed) |
//! | the old folder only | it is renamed to the new name: one atomic rename on the same volume, nothing copied |
//! | the old folder holds a game installation (`config.json` `root` / `install.dir`, or a `kalamar-install.json`) | that installation stays where it is (its build records hold absolute paths: moved, the next update would recompile everything); the launcher's own entries are renamed one by one into `dev.kalamar.launcher.moving`, which is then renamed to the new name |
//! | the game is installed directly in the old folder (not in a subfolder) | nothing moves: the old folder stays the data folder |
//! | the rename is refused as across volumes | a copy, each file checked against its source, then the switch; the old folder is left as it was |
//! | both folders | the new one is used; the old one is left untouched, never merged (logged) |
//! | an older launcher is open (its WebView holds `%LOCALAPPDATA%\dev.splat3recomp.launcher\EBWebView\lockfile`), an update of the launcher is on trial (`update\journal.json`), a file of the old folder is in use (a game, a relay log, another launcher on it) or another launcher is moving it | nothing moves: this session uses the old folder, and holds a file open in it so that no other launcher moves it meanwhile; the next start tries again |
//!
//! A move is journaled (`data-move.json`, inside the folder being moved, so it travels with it). A launcher stopped at
//! any point finishes or undoes it at its next start: the rename either happened or not; one by one, each entry is on
//! one side or the other and the journal lists what stays; a partial copy is thrown away and made again. Links and
//! junctions are never followed: a rename moves the link itself, a copy leaves it out (logged). After the move, the
//! absolute paths into the old folder that the launcher's own JSON files hold (`config.json` and the like) are
//! rewritten to the new folder, except those into an installation that stayed. `data-folder.json` records the move and
//! `data-folder.log` every decision.
//!
//! Tests point the whole thing elsewhere with `KALAMAR_APPDATA`: a folder standing for `%USERPROFILE%\AppData`, its
//! `Roaming` and `Local` subfolders then stand for `%APPDATA%` and `%LOCALAPPDATA%` (the WebView's profile included).

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// The Tauri identifier (`tauri.conf.json`), the name of the data folder.
pub const IDENTIFIER: &str = "dev.kalamar.launcher";
/// The identifier of the launchers before this one.
pub const LEGACY_IDENTIFIER: &str = "dev.splat3recomp.launcher";
/// A data folder of the player's choosing (portable use, tests): used as it is, never moved.
pub const ENV_DATA: &str = "KALAMAR_DATA";
/// The older name of `KALAMAR_DATA`, still read when the new one is unset.
pub const LEGACY_ENV_DATA: &str = "SPLAT3_LAUNCHER_DATA";
/// Tests: a folder standing for `%USERPROFILE%\AppData` (`Roaming`, `Local` inside it).
pub const ENV_APPDATA: &str = "KALAMAR_APPDATA";
pub const JOURNAL: &str = "data-move.json";
pub const RECORD: &str = "data-folder.json";
pub const LOG: &str = "data-folder.log";
/// The marker of an installation made by the install wizard (`installer::STATE_FILE`).
const INSTALL_MARKER: &str = crate::installer::STATE_FILE;
/// How long an update's `confirmed` journal keeps the old folder in place: the old process reads the witness there.
const CONFIRMED_HOLD: Duration = Duration::from_secs(600);

/// `%APPDATA%` and `%LOCALAPPDATA%`, or what stands for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    pub roaming: PathBuf,
    pub local: PathBuf,
}

impl Roots {
    pub fn from_env(get: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Option<Roots> {
        if let Some(a) = get(ENV_APPDATA).filter(|v| !v.is_empty()) {
            let a = PathBuf::from(a);
            return Some(Roots { roaming: a.join("Roaming"), local: a.join("Local") });
        }
        #[cfg(windows)]
        {
            let roaming = PathBuf::from(get("APPDATA").filter(|v| !v.is_empty())?);
            let local = get("LOCALAPPDATA").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| roaming.clone());
            Some(Roots { roaming, local })
        }
        #[cfg(not(windows))]
        {
            // XDG: data under `$XDG_DATA_HOME` (~/.local/share), caches and the WebView's profile under `$XDG_CACHE_HOME`.
            let home = PathBuf::from(get("HOME").filter(|v| !v.is_empty())?);
            let dir = |var: &str, fallback: &str| get(var).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(fallback));
            Some(Roots { roaming: dir("XDG_DATA_HOME", ".local/share"), local: dir("XDG_CACHE_HOME", ".cache") })
        }
    }
    pub fn data(&self) -> PathBuf {
        self.roaming.join(IDENTIFIER)
    }
    pub fn legacy(&self) -> PathBuf {
        self.roaming.join(LEGACY_IDENTIFIER)
    }
    /// Where entries go one by one before the whole is renamed to [`Roots::data`].
    pub fn staging(&self) -> PathBuf {
        self.roaming.join(format!("{IDENTIFIER}.moving"))
    }
    /// The WebView's profile (Tauri's `app_local_data_dir`).
    pub fn webview(&self) -> PathBuf {
        self.local.join(IDENTIFIER)
    }
    /// The lock file an older launcher's WebView holds while its window exists.
    pub fn legacy_webview_lock(&self) -> PathBuf {
        self.local.join(LEGACY_IDENTIFIER).join("EBWebView").join("lockfile")
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The whole folder renamed at once.
    Rename,
    /// The launcher's entries renamed one by one; a game installation stayed in the old folder.
    Split,
    /// Copied and checked (the rename was refused as across volumes); the old folder left as it was.
    Copy,
}

/// Why the old folder stays in use for this session.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// An older launcher is open.
    OldLauncher,
    /// An update of the launcher is on trial or just confirmed: the old process reads its witness in the old folder.
    UpdateTrial,
    /// A file of the old folder is in use (a game, a relay, another launcher working in it).
    InUse,
    /// Another launcher is moving it right now.
    Busy,
    /// The game is installed directly in the old folder: it stays the data folder.
    GameInFolder,
    /// Anything else (the message is in the log).
    Failed,
}

/// What the start found and did; shown in Settings > About and written to `data-folder.log`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// `KALAMAR_DATA` (or `SPLAT3_LAUNCHER_DATA`): that folder, never moved.
    Override { var: String },
    /// Neither folder existed.
    Fresh,
    /// The new folder, nothing to move.
    Current,
    /// Moved during this start.
    Moved { from: String, mode: Mode, kept: Vec<String>, rewritten: Vec<String>, skipped: Vec<String> },
    /// The old folder is used for this session; the move is tried again at the next start.
    Deferred { from: String, reason: Reason },
    /// Both folders exist: the new one is used and the old one left as it is. `left`: `game` (what stayed after a
    /// split move: the game installation), `copy` (the source of a copy), `data` (anything else, never merged).
    Both { from: String, left: &'static str },
}

/// The data folder of this process, decided once.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub dir: PathBuf,
    pub outcome: Outcome,
    /// The WebView's profile when `KALAMAR_APPDATA` points elsewhere (else Tauri's own choice, the same folder).
    pub webview: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Journal {
    version: u32,
    from: PathBuf,
    to: PathBuf,
    mode: Mode,
    /// Entries of the old folder that stay there (a game installation).
    kept: Vec<String>,
    started: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
struct Record {
    moved_from: String,
    at: u64,
    mode: Option<Mode>,
    kept: Vec<String>,
    rewritten: Vec<String>,
    skipped: Vec<String>,
}

static RESOLVED: OnceLock<Resolved> = OnceLock::new();
/// The file held open in the old folder while this session uses it (deleted by the system when the process ends).
static GUARD: OnceLock<std::fs::File> = OnceLock::new();

/// The data folder, decided (and the old one moved) at the first call: `lib::run` makes it before anything else.
pub fn current() -> &'static Resolved {
    RESOLVED.get_or_init(|| {
        let r = decide(&|k| std::env::var_os(k));
        crate::trace::mark(&format!("datadir:{} {}", r.dir.display(), describe(&r.outcome)));
        r
    })
}

fn decide(get: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Resolved {
    for var in [ENV_DATA, LEGACY_ENV_DATA] {
        if let Some(v) = get(var).filter(|v| !v.is_empty()) {
            return Resolved { dir: v.into(), outcome: Outcome::Override { var: var.into() }, webview: None };
        }
    }
    // A test build never reaches into the real profile: only a folder that `KALAMAR_APPDATA` names.
    let real = |k: &str| if cfg!(test) && get(ENV_APPDATA).is_none() { None } else { get(k) };
    let Some(roots) = Roots::from_env(&real) else {
        return Resolved { dir: std::env::temp_dir().join("kalamar-launcher"), outcome: Outcome::Fresh, webview: None };
    };
    let webview = get(ENV_APPDATA).filter(|v| !v.is_empty()).map(|_| roots.webview());
    // Two launchers starting together: one moves, the other waits and finds the result.
    let lock = MoveLock::take(Duration::from_secs(20));
    let probes = Probes { old_launcher_open: &|| file_held(&roots.legacy_webview_lock()), rename: &|a, b| std::fs::rename(a, b), stop_at: None };
    let (dir, outcome) = resolve(&roots, &probes, lock.is_some(), crate::util::now_secs()).unwrap_or_else(|Stopped| (roots.legacy(), Outcome::Fresh));
    let _ = lock;
    if let Outcome::Deferred { .. } = outcome {
        hold(&dir);
    }
    // Every start that moves, defers or finds both folders says so; a start in place adds nothing.
    if !matches!(outcome, Outcome::Current) {
        log(&dir, &describe(&outcome));
    }
    Resolved { dir, outcome, webview }
}

/// One line for the log.
pub fn describe(o: &Outcome) -> String {
    match o {
        Outcome::Override { var } => format!("data folder from {var}"),
        Outcome::Fresh => "new data folder".into(),
        Outcome::Current => "data folder in place".into(),
        Outcome::Moved { from, mode, kept, rewritten, skipped } => format!(
            "moved from {from} ({mode:?}); stayed there: {kept:?}; paths rewritten in: {rewritten:?}; not copied (links): {skipped:?}"
        ),
        Outcome::Deferred { from, reason } => format!("still using {from} for this session ({reason:?}); the move is tried again at the next start"),
        Outcome::Both { from, left } => format!("{from} also exists ({left}); left as it is, nothing merged"),
    }
}

// ---- the decision ---------------------------------------------------------------------------------------------------

/// What the move needs from the system; tests replace it.
pub(crate) struct Probes<'a> {
    pub old_launcher_open: &'a dyn Fn() -> bool,
    pub rename: &'a dyn Fn(&Path, &Path) -> std::io::Result<()>,
    /// Tests: the process "dies" at this named point (nothing after it runs, not even a clean-up).
    pub stop_at: Option<&'a str>,
}

/// The process died at a test's stop point.
#[derive(Debug)]
pub(crate) struct Stopped;

fn point(p: &Probes, name: &str) -> Result<(), Stopped> {
    if p.stop_at == Some(name) { Err(Stopped) } else { Ok(()) }
}

/// Decides the folder of this session, finishing or doing the move. `locked`: this process holds the move lock.
pub(crate) fn resolve(roots: &Roots, p: &Probes, locked: bool, now: u64) -> Result<(PathBuf, Outcome), Stopped> {
    let (old, new, staging) = (roots.legacy(), roots.data(), roots.staging());
    let from = old.display().to_string();
    // A move that stopped half-way is finished (or undone) first.
    if staging.is_dir()
        && let Some(j) = read_journal(&staging)
    {
        if !locked {
            return Ok((if old.is_dir() { old } else { new }, Outcome::Deferred { from, reason: Reason::Busy }));
        }
        return resume(p, &j, &staging);
    }
    if new.is_dir() {
        if let Some(j) = read_journal(&new) {
            return finish(p, &j);
        }
        if !old.is_dir() {
            return Ok((new, Outcome::Current));
        }
        if !is_empty_dir(&new) {
            let left = match read_record(&new) {
                Some(r) if r.mode == Some(Mode::Copy) => "copy",
                Some(r) if r.mode == Some(Mode::Split) && only(&old, &r.kept) => "game",
                _ => "data",
            };
            return Ok((new, Outcome::Both { from, left }));
        }
        // An empty folder (made by something else) does not count.
        let _ = std::fs::remove_dir(&new);
    }
    if !old.is_dir() {
        return Ok((new, Outcome::Fresh));
    }
    let deferred = |reason| Ok((old.clone(), Outcome::Deferred { from: from.clone(), reason }));
    if !locked {
        return deferred(Reason::Busy);
    }
    if (p.old_launcher_open)() {
        return deferred(Reason::OldLauncher);
    }
    if update_in_progress(&old, now) {
        return deferred(Reason::UpdateTrial);
    }
    let Some(kept) = kept_entries(&old) else { return deferred(Reason::GameInFolder) };
    let mut j = Journal { version: 1, from: old.clone(), to: new.clone(), mode: Mode::Rename, kept, started: now };
    if !j.kept.is_empty() {
        j.mode = Mode::Split;
        return split(p, &j, &staging);
    }
    if write_journal(&old, &j).is_err() {
        return deferred(Reason::Failed);
    }
    point(p, "rename:journal")?;
    match retry(|| (p.rename)(&old, &new)) {
        Ok(()) => {}
        Err(e) if e.raw_os_error() == Some(17) => {
            // ERROR_NOT_SAME_DEVICE: a copy instead.
            let _ = std::fs::remove_file(old.join(JOURNAL));
            j.mode = Mode::Copy;
            return copy(p, &j, &staging);
        }
        Err(_) => {
            let _ = std::fs::remove_file(old.join(JOURNAL));
            return deferred(Reason::InUse);
        }
    }
    point(p, "rename:moved")?;
    finish(p, &j)
}

/// Retries an operation that an antivirus or the indexer may block for a moment (about two seconds in all).
fn retry(mut f: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut n = 0;
    loop {
        match f() {
            Err(e) if matches!(e.raw_os_error(), Some(5 | 32 | 33)) && n < 8 => {
                n += 1;
                std::thread::sleep(Duration::from_millis(250));
            }
            r => return r,
        }
    }
}

/// The entries of the old folder that hold a game installation (they stay where they are). `None`: the game is
/// installed in the folder itself, which then stays the data folder.
fn kept_entries(old: &Path) -> Option<Vec<String>> {
    let mut kept: Vec<String> = Vec::new();
    let mut add = |name: String| {
        if !kept.iter().any(|k| k.eq_ignore_ascii_case(&name)) {
            kept.push(name);
        }
    };
    let cfg: serde_json::Value = crate::util::read_text_limited(&old.join("config.json"), 1 << 20)
        .ok()
        .flatten()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default();
    for anchor in [cfg.get("root"), cfg.get("install").and_then(|i| i.get("dir"))].into_iter().flatten().filter_map(|v| v.as_str()) {
        match under(Path::new(anchor), old) {
            Some(rel) if rel.as_os_str().is_empty() => return None,
            Some(rel) => {
                if let Some(Component::Normal(first)) = rel.components().next() {
                    add(first.to_string_lossy().into_owned());
                }
            }
            None => {}
        }
    }
    if old.join(INSTALL_MARKER).is_file() {
        return None;
    }
    for e in std::fs::read_dir(old).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) && e.path().join(INSTALL_MARKER).is_file() {
            add(e.file_name().to_string_lossy().into_owned());
        }
    }
    kept.sort();
    Some(kept)
}

/// `path` relative to `base` when it is inside it (case and separators aside, as Windows compares them).
fn under(path: &Path, base: &Path) -> Option<PathBuf> {
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_string();
    let (p, b) = (norm(path), norm(base));
    if p.len() < b.len() || !p.is_char_boundary(b.len()) || !p[..b.len()].eq_ignore_ascii_case(&b) {
        return None;
    }
    let rest = &p[b.len()..];
    if rest.is_empty() {
        return Some(PathBuf::new());
    }
    rest.strip_prefix('\\').map(PathBuf::from)
}

/// The folder holds nothing but `names` (what a split move left: the game installation).
fn only(dir: &Path, names: &[String]) -> bool {
    std::fs::read_dir(dir).map(|rd| rd.flatten().all(|e| names.iter().any(|k| k.eq_ignore_ascii_case(&e.file_name().to_string_lossy())))).unwrap_or(false)
}

fn is_empty_dir(d: &Path) -> bool {
    std::fs::read_dir(d).map(|mut rd| rd.next().is_none()).unwrap_or(false)
}

/// An update of the launcher is under way in the old folder: the new version on trial confirms in it, the old process
/// waits for the witness there (`update::apply`).
fn update_in_progress(old: &Path, now: u64) -> bool {
    let j = old.join("update").join("journal.json");
    let Some(v) = crate::util::read_text_limited(&j, 64 * 1024).ok().flatten().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else {
        return false;
    };
    match v.get("phase").and_then(|p| p.as_str()) {
        Some("swapping" | "trial") => true,
        Some("confirmed") => crate::util::mtime_secs(&j).is_some_and(|t| now.saturating_sub(t) < CONFIRMED_HOLD.as_secs()),
        _ => false,
    }
}

// ---- the three ways ---------------------------------------------------------------------------------------------------

/// Split: the launcher's entries into the staging folder one by one, then the staging folder renamed to the new name.
fn split(p: &Probes, j: &Journal, staging: &Path) -> Result<(PathBuf, Outcome), Stopped> {
    let deferred = |reason| Ok((j.from.clone(), Outcome::Deferred { from: j.from.display().to_string(), reason }));
    if std::fs::create_dir(staging).is_err() {
        return deferred(Reason::Failed);
    }
    if write_journal(staging, j).is_err() {
        let _ = std::fs::remove_dir_all(staging);
        return deferred(Reason::Failed);
    }
    point(p, "split:journal")?;
    resume(p, j, staging)
}

/// Goes on with a split or copy whose journal is in the staging folder (a fresh one, or one a stopped launcher left).
fn resume(p: &Probes, j: &Journal, staging: &Path) -> Result<(PathBuf, Outcome), Stopped> {
    let from = j.from.display().to_string();
    if j.mode == Mode::Copy {
        // A copy is never half trusted: what a stopped launcher left is thrown away and copied again.
        let _ = std::fs::remove_dir_all(staging);
        return copy(p, j, staging);
    }
    let pending: Vec<std::ffi::OsString> = std::fs::read_dir(&j.from)
        .map(|rd| rd.flatten().map(|e| e.file_name()).filter(|n| !is_kept(j, n) && n != JOURNAL).collect())
        .unwrap_or_default();
    for name in pending {
        let (a, b) = (j.from.join(&name), staging.join(&name));
        if b.exists() {
            continue; // moved before, and something made it again in the old folder: the moved one wins, the other stays
        }
        if retry(|| (p.rename)(&a, &b)).is_err() {
            undo_split(p, j, staging);
            return Ok((j.from.clone(), Outcome::Deferred { from, reason: Reason::InUse }));
        }
        point(p, &format!("split:moved:{}", name.to_string_lossy()))?;
    }
    point(p, "split:entries")?;
    if retry(|| (p.rename)(staging, &j.to)).is_err() {
        undo_split(p, j, staging);
        return Ok((j.from.clone(), Outcome::Deferred { from, reason: Reason::InUse }));
    }
    point(p, "split:committed")?;
    finish(p, j)
}

fn is_kept(j: &Journal, name: &std::ffi::OsStr) -> bool {
    j.kept.iter().any(|k| k.eq_ignore_ascii_case(&name.to_string_lossy()))
}

/// Puts every entry of the staging folder back in the old folder, then removes the staging folder (only once empty).
fn undo_split(p: &Probes, j: &Journal, staging: &Path) {
    for e in std::fs::read_dir(staging).into_iter().flatten().flatten() {
        let name = e.file_name();
        if name != JOURNAL && !j.from.join(&name).exists() {
            let _ = retry(|| (p.rename)(&e.path(), &j.from.join(&name)));
        }
    }
    let _ = std::fs::remove_file(staging.join(JOURNAL));
    let _ = std::fs::remove_dir(staging);
}

/// Copy: every file of the old folder (but what stays and the links) into the staging folder, each one checked against
/// its source, then the staging folder renamed to the new name. The old folder is left as it was.
fn copy(p: &Probes, j: &Journal, staging: &Path) -> Result<(PathBuf, Outcome), Stopped> {
    let from = j.from.display().to_string();
    // Only a staging folder this launcher made (with its journal) is ever removed.
    let failed = |staging: &Path| {
        let _ = std::fs::remove_dir_all(staging);
        Ok((j.from.clone(), Outcome::Deferred { from: from.clone(), reason: Reason::Failed }))
    };
    if std::fs::create_dir(staging).is_err() {
        return Ok((j.from.clone(), Outcome::Deferred { from: from.clone(), reason: Reason::Failed }));
    }
    if write_journal(staging, j).is_err() {
        return failed(staging);
    }
    point(p, "copy:journal")?;
    let mut skipped = Vec::new();
    if copy_tree(&j.from, staging, Path::new(""), j, &mut skipped).is_err() {
        return failed(staging);
    }
    point(p, "copy:copied")?;
    if retry(|| (p.rename)(staging, &j.to)).is_err() {
        return failed(staging);
    }
    point(p, "copy:committed")?;
    let r = finish(p, j)?;
    // The links left out are only known now: they go in the record.
    if let (dir, Outcome::Moved { from, mode, kept, rewritten, .. }) = r {
        if let Some(mut rec) = read_record(&dir) {
            rec.skipped = skipped.clone();
            let _ = write_record(&dir, &rec);
        }
        return Ok((dir, Outcome::Moved { from, mode, kept, rewritten, skipped }));
    }
    Ok(r)
}

fn copy_tree(src: &Path, dst: &Path, rel: &Path, j: &Journal, skipped: &mut Vec<String>) -> std::io::Result<()> {
    for e in std::fs::read_dir(src.join(rel))?.flatten() {
        let r = rel.join(e.file_name());
        if rel.as_os_str().is_empty() && (is_kept(j, &e.file_name()) || e.file_name() == JOURNAL) {
            continue;
        }
        // The entry's own type, never what a link points to.
        let t = e.file_type()?;
        if t.is_symlink() {
            skipped.push(r.display().to_string());
        } else if t.is_dir() {
            std::fs::create_dir_all(dst.join(&r))?;
            copy_tree(src, dst, &r, j, skipped)?;
        } else if t.is_file() {
            let (a, b) = (src.join(&r), dst.join(&r));
            std::fs::copy(&a, &b)?;
            std::fs::OpenOptions::new().write(true).open(&b)?.sync_all()?;
            if crate::update::apply::sha256_file(&a)? != crate::update::apply::sha256_file(&b)? {
                return Err(std::io::Error::other(format!("{}: the copy differs from its source", r.display())));
            }
        } else {
            skipped.push(r.display().to_string());
        }
    }
    Ok(())
}

/// After the move: the paths into the old folder rewritten, the record written, the journal removed. Safe to run
/// again: a rewritten path no longer points into the old folder.
fn finish(p: &Probes, j: &Journal) -> Result<(PathBuf, Outcome), Stopped> {
    let dir = j.to.clone();
    let rewritten = rewrite_paths(&dir, j);
    point(p, "finish:rewritten")?;
    let rec = Record {
        moved_from: j.from.display().to_string(),
        at: crate::util::now_secs(),
        mode: Some(j.mode),
        kept: j.kept.clone(),
        rewritten: rewritten.clone(),
        skipped: Vec::new(),
    };
    let _ = write_record(&dir, &rec);
    let _ = std::fs::remove_file(dir.join(JOURNAL));
    let from = j.from.display().to_string();
    Ok((dir, Outcome::Moved { from, mode: j.mode, kept: j.kept.clone(), rewritten, skipped: Vec::new() }))
}

// ---- paths in the launcher's own files --------------------------------------------------------------------------------

/// The JSON files whose absolute paths follow the move: the launcher's own files at the top of its folder and the
/// servers' small files. Returns the ones changed (relative names).
fn rewrite_paths(dir: &Path, j: &Journal) -> Vec<String> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut add_json = |d: &Path| {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|t| t.is_file()) && e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("json")) {
                files.push(e.path());
            }
        }
    };
    add_json(dir);
    add_json(&dir.join("online"));
    for e in std::fs::read_dir(dir.join("online")).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            add_json(&e.path());
        }
    }
    let mut changed = Vec::new();
    for f in files {
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name == JOURNAL || name == RECORD {
            continue;
        }
        let Ok(Some(text)) = crate::util::read_text_limited(&f, 4 << 20) else { continue };
        if let Some(out) = rewrite_text(&text, &j.from, &j.to, &j.kept)
            && serde_json::from_str::<serde_json::Value>(out.trim_start_matches('\u{feff}')).is_ok()
            && crate::util::atomic_write(&f, out.as_bytes()).is_ok()
        {
            changed.push(f.strip_prefix(dir).unwrap_or(&f).display().to_string());
        }
    }
    changed.sort();
    changed
}

/// The text of a JSON file with every path into `from` (but those into a kept entry) pointing into `to` instead. Paths
/// are matched as JSON writes them: backslashes doubled (`C:\\Users\\<name>\\...`) or forward slashes; case aside,
/// ASCII only; the old folder's name must end there (`...launcher2` is another folder). `None` when nothing changed.
fn rewrite_text(text: &str, from: &Path, to: &Path, kept: &[String]) -> Option<String> {
    let base = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_string();
    let (f, t) = (base(from), base(to));
    // (old prefix, new prefix, separator) as each style writes it
    let styles = [(f.replace('\\', "\\\\"), t.replace('\\', "\\\\"), "\\\\"), (f.replace('\\', "/"), t.replace('\\', "/"), "/")];
    let mut out = text.to_string();
    let mut any = false;
    for (old, new, sep) in &styles {
        let mut res = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(i) = find_ascii_ci(rest, old) {
            let after = &rest[i + old.len()..];
            let ends = after.starts_with('"') || after.starts_with(sep);
            let into_kept = after.strip_prefix(sep).is_some_and(|tail| {
                let comp = tail.split(['"', '/', '\\']).next().unwrap_or("");
                kept.iter().any(|k| k.eq_ignore_ascii_case(comp))
            });
            res.push_str(&rest[..i]);
            if ends && !into_kept {
                res.push_str(new);
                any = true;
            } else {
                res.push_str(&rest[i..i + old.len()]);
            }
            rest = after;
        }
        res.push_str(rest);
        out = res;
    }
    any.then_some(out)
}

fn find_ascii_ci(hay: &str, needle: &str) -> Option<usize> {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| hay.is_char_boundary(i) && h[i..i + n.len()].eq_ignore_ascii_case(n))
}

// ---- small files ------------------------------------------------------------------------------------------------------

fn read_journal(dir: &Path) -> Option<Journal> {
    crate::util::read_text_limited(&dir.join(JOURNAL), 64 * 1024).ok().flatten().and_then(|t| serde_json::from_str(&t).ok())
}

fn write_journal(dir: &Path, j: &Journal) -> std::io::Result<()> {
    let text = serde_json::to_vec_pretty(j).map_err(std::io::Error::other)?;
    crate::util::atomic_write(&dir.join(JOURNAL), &text).map_err(|e| std::io::Error::other(e.to_string()))
}

fn read_record(dir: &Path) -> Option<Record> {
    crate::util::read_text_limited(&dir.join(RECORD), 1 << 20).ok().flatten().and_then(|t| serde_json::from_str(&t).ok())
}

fn write_record(dir: &Path, r: &Record) -> std::io::Result<()> {
    let text = serde_json::to_vec_pretty(r).map_err(std::io::Error::other)?;
    crate::util::atomic_write(&dir.join(RECORD), &text).map_err(|e| std::io::Error::other(e.to_string()))
}

/// Appends a dated line to `data-folder.log` (kept under 256 KiB: the older half goes to `data-folder.log.1`).
pub fn log(dir: &Path, line: &str) {
    use std::io::Write;
    let path = dir.join(LOG);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = std::fs::rename(&path, dir.join(format!("{LOG}.1")));
    }
    if std::fs::create_dir_all(dir).is_ok()
        && let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path)
    {
        let _ = writeln!(f, "{} [{}] {line}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), std::process::id());
    }
}

/// Holds a file open in the old folder for the rest of this session, so that no other launcher renames the folder
/// under this one (an open file anywhere inside a folder makes Windows refuse its rename). The system deletes the
/// file when the process ends, however it ends.
fn hold(dir: &Path) {
    let path = dir.join(format!("launcher-in-use-{}.lock", std::process::id()));
    let mut o = std::fs::OpenOptions::new();
    o.read(true).write(true).create(true).truncate(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const DELETE: u32 = 0x0001_0000;
        const GENERIC_READ: u32 = 0x8000_0000;
        const GENERIC_WRITE: u32 = 0x4000_0000;
        const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
        o.access_mode(GENERIC_READ | GENERIC_WRITE | DELETE).share_mode(0x7).custom_flags(FILE_FLAG_DELETE_ON_CLOSE);
    }
    if let Ok(f) = o.open(&path) {
        let _ = GUARD.set(f);
    }
}

/// Another process holds `path` open (an older launcher's WebView holds its `lockfile` while its window exists). Opened
/// for reading with no sharing, closed at once; nothing is written.
fn file_held(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.share_mode(0);
    }
    matches!(o.open(path), Err(e) if matches!(e.raw_os_error(), Some(32 | 33 | 5)))
}

/// The session-wide lock two starting launchers share, so that only one moves the folder.
struct MoveLock {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

impl MoveLock {
    #[cfg(windows)]
    fn take(wait: Duration) -> Option<MoveLock> {
        use windows_sys::Win32::Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0};
        use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
        let wide: Vec<u16> = "Local\\Kalamar.DataFolderMove".encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: a named mutex of the session, not owned at creation; the handle is checked, and closed on failure here
        // or by `Drop`.
        let h = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
        if h.is_null() {
            return None;
        }
        // SAFETY: a bounded wait on the handle just created. An abandoned mutex (a launcher that died while moving) is
        // ours now; its journal says what it left.
        match unsafe { WaitForSingleObject(h, wait.as_millis().min(u32::MAX as u128) as u32) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Some(MoveLock { handle: h }),
            _ => {
                // SAFETY: the handle created above, not owned.
                unsafe { CloseHandle(h) };
                None
            }
        }
    }

    #[cfg(not(windows))]
    fn take(_wait: Duration) -> Option<MoveLock> {
        Some(MoveLock {})
    }
}

#[cfg(windows)]
impl Drop for MoveLock {
    fn drop(&mut self) {
        // SAFETY: the mutex this thread owns, then its handle.
        unsafe {
            windows_sys::Win32::System::Threading::ReleaseMutex(self.handle);
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct T {
        base: PathBuf,
        roots: Roots,
    }

    impl T {
        fn new(tag: &str) -> T {
            let base = std::env::temp_dir().join(format!("kal-dd-{tag}-{}", crate::util::random_hex(4)));
            let roots = Roots { roaming: base.join("Roaming"), local: base.join("Local") };
            std::fs::create_dir_all(&roots.roaming).unwrap();
            std::fs::create_dir_all(&roots.local).unwrap();
            T { base, roots }
        }
        fn old(&self) -> PathBuf {
            self.roots.legacy()
        }
        fn new_dir(&self) -> PathBuf {
            self.roots.data()
        }
        /// A data folder as an earlier launcher left it: its files, a server's session, caches, the update folder, and
        /// a configuration whose paths point into it.
        fn fill_old(&self, game_inside: bool) {
            let o = self.old();
            let esc = |p: &Path| p.display().to_string().replace('\\', "\\\\");
            let root = if game_inside { o.join("game").join("work").join("splatoon3-11.2.0") } else { self.base.join("Kalamar").join("work").join("splatoon3-11.2.0") };
            let install = root.parent().unwrap().parent().unwrap().to_path_buf();
            std::fs::create_dir_all(o.join("online").join("abc123")).unwrap();
            std::fs::create_dir_all(o.join("game-cache").join("k")).unwrap();
            std::fs::create_dir_all(o.join("update")).unwrap();
            std::fs::create_dir_all(o.join("runtime-src").join("ab12")).unwrap();
            let cfg = format!(
                "{{\n  \"root\": \"{}\",\n  \"relay_exe\": \"{}\",\n  \"install\": {{ \"dir\": \"{}\" }},\n  \"language\": \"fr\"\n}}",
                esc(&root),
                esc(&o.join("tools").join("relay.exe")),
                esc(&install)
            );
            std::fs::write(o.join("config.json"), cfg).unwrap();
            std::fs::write(o.join("profile.json"), r#"{"schema":1,"name":"Kay"}"#).unwrap();
            std::fs::write(o.join("last_launch.json"), format!(r#"{{"log":"{}/runtime-src/ab12/x.log"}}"#, o.display().to_string().replace('\\', "/"))).unwrap();
            std::fs::write(o.join("online").join("servers.json"), r#"{"servers":[]}"#).unwrap();
            std::fs::write(o.join("online").join("abc123").join("session.dat"), b"\x01\x00SECRET\xff").unwrap();
            std::fs::write(o.join("game-cache").join("k").join("catalogue.json"), "{}").unwrap();
            std::fs::write(o.join("update").join("state.json"), r#"{"last_check":1}"#).unwrap();
            std::fs::write(o.join("runtime-src").join("ab12").join("x.log"), b"log").unwrap();
            if game_inside {
                std::fs::create_dir_all(root.join("build")).unwrap();
                std::fs::write(root.join("build").join("game.exe"), b"exe").unwrap();
                std::fs::write(install.join(INSTALL_MARKER), b"{}").unwrap();
            }
        }
        fn run(&self, stop_at: Option<&str>) -> Result<(PathBuf, Outcome), Stopped> {
            self.run_with(stop_at, &|| false, &|a, b| std::fs::rename(a, b))
        }
        fn run_with(&self, stop_at: Option<&str>, open: &dyn Fn() -> bool, rename: &dyn Fn(&Path, &Path) -> std::io::Result<()>) -> Result<(PathBuf, Outcome), Stopped> {
            resolve(&self.roots, &Probes { old_launcher_open: open, rename, stop_at }, true, crate::util::now_secs())
        }
    }

    impl Drop for T {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    /// Relative path -> bytes of every file under `d`.
    fn snapshot(d: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(base: &Path, d: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                if e.file_type().unwrap().is_dir() {
                    walk(base, &e.path(), out);
                } else {
                    out.insert(e.path().strip_prefix(base).unwrap().display().to_string(), std::fs::read(e.path()).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(d, d, &mut out);
        out
    }

    /// The old folder's files, as they must be found in the new one (the paths in the JSON files aside).
    fn without_paths(mut s: BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
        for k in ["config.json", "last_launch.json", JOURNAL, RECORD, LOG] {
            s.remove(k);
        }
        s
    }

    fn cfg(d: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(d.join("config.json")).unwrap()).unwrap()
    }

    #[test]
    fn the_identifier_is_the_one_tauri_uses() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], IDENTIFIER);
        assert_ne!(IDENTIFIER, LEGACY_IDENTIFIER);
    }

    #[test]
    #[cfg(windows)]
    fn the_folder_named_by_a_variable_is_used_as_it_is_the_new_name_first() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| std::ffi::OsString::from(v));
        let r = decide(&env(&[(ENV_DATA, r"D:\new"), (LEGACY_ENV_DATA, r"D:\old")]));
        assert_eq!((r.dir, r.outcome), (PathBuf::from(r"D:\new"), Outcome::Override { var: ENV_DATA.into() }));
        let r = decide(&env(&[(LEGACY_ENV_DATA, r"D:\old")]));
        assert_eq!((r.dir, r.outcome), (PathBuf::from(r"D:\old"), Outcome::Override { var: LEGACY_ENV_DATA.into() }));
        let roots = Roots::from_env(&env(&[("APPDATA", r"C:\U\AppData\Roaming"), ("LOCALAPPDATA", r"C:\U\AppData\Local")])).unwrap();
        assert_eq!(roots.data(), PathBuf::from(r"C:\U\AppData\Roaming\dev.kalamar.launcher"));
        assert_eq!(roots.legacy(), PathBuf::from(r"C:\U\AppData\Roaming\dev.splat3recomp.launcher"));
        assert_eq!(roots.legacy_webview_lock(), PathBuf::from(r"C:\U\AppData\Local\dev.splat3recomp.launcher\EBWebView\lockfile"));
        let test = Roots::from_env(&env(&[(ENV_APPDATA, r"T:\fake"), ("APPDATA", r"C:\real")])).unwrap();
        assert_eq!((test.roaming, test.local), (PathBuf::from(r"T:\fake\Roaming"), PathBuf::from(r"T:\fake\Local")));
    }

    #[test]
    fn a_fresh_start_uses_the_new_folder_and_moves_nothing() {
        let t = T::new("fresh");
        assert_eq!(t.run(None).unwrap(), (t.new_dir(), Outcome::Fresh));
        std::fs::create_dir_all(t.new_dir()).unwrap();
        std::fs::write(t.new_dir().join("config.json"), "{}").unwrap();
        assert_eq!(t.run(None).unwrap(), (t.new_dir(), Outcome::Current));
    }

    #[test]
    fn the_old_folder_is_renamed_whole_and_its_paths_follow() {
        let t = T::new("whole");
        t.fill_old(false);
        let before = snapshot(&t.old());
        let (dir, out) = t.run(None).unwrap();
        assert_eq!(dir, t.new_dir());
        let Outcome::Moved { mode, kept, rewritten, .. } = out else { panic!("{out:?}") };
        assert_eq!((mode, kept), (Mode::Rename, Vec::<String>::new()));
        assert_eq!(rewritten, ["config.json", "last_launch.json"]);
        assert!(!t.old().exists());
        assert_eq!(without_paths(snapshot(&t.new_dir())), without_paths(before));
        let c = cfg(&t.new_dir());
        assert_eq!(c["relay_exe"], t.new_dir().join("tools").join("relay.exe").display().to_string());
        assert_eq!(c["root"], t.base.join("Kalamar").join("work").join("splatoon3-11.2.0").display().to_string(), "a path elsewhere is left alone");
        assert!(std::fs::read_to_string(t.new_dir().join("last_launch.json")).unwrap().contains(&format!("{}/runtime-src", t.new_dir().display().to_string().replace('\\', "/"))));
        assert!(!t.new_dir().join(JOURNAL).exists() && t.new_dir().join(RECORD).is_file());
        // the next start: in place
        assert_eq!(t.run(None).unwrap(), (t.new_dir(), Outcome::Current));
    }

    #[test]
    fn a_launcher_stopped_at_any_point_of_a_rename_finishes_it_at_the_next_start() {
        for stop in ["rename:journal", "rename:moved", "finish:rewritten"] {
            let t = T::new("stop");
            t.fill_old(false);
            let before = without_paths(snapshot(&t.old()));
            assert!(t.run(Some(stop)).is_err(), "{stop}");
            let (dir, out) = t.run(None).unwrap();
            assert_eq!(dir, t.new_dir(), "{stop}");
            assert!(matches!(out, Outcome::Moved { mode: Mode::Rename, .. }), "{stop}: {out:?}");
            assert!(!t.old().exists(), "{stop}");
            assert_eq!(without_paths(snapshot(&t.new_dir())), before, "{stop}");
            assert_eq!(cfg(&t.new_dir())["relay_exe"], t.new_dir().join("tools").join("relay.exe").display().to_string(), "{stop}");
            assert!(!t.new_dir().join(JOURNAL).exists(), "{stop}");
        }
    }

    #[test]
    fn when_both_folders_exist_the_new_one_is_used_and_nothing_is_merged() {
        let t = T::new("both");
        t.fill_old(false);
        std::fs::create_dir_all(t.new_dir()).unwrap();
        std::fs::write(t.new_dir().join("config.json"), r#"{"language":"en"}"#).unwrap();
        let (old_before, new_before) = (snapshot(&t.old()), snapshot(&t.new_dir()));
        let (dir, out) = t.run(None).unwrap();
        assert_eq!((dir, out), (t.new_dir(), Outcome::Both { from: t.old().display().to_string(), left: "data" }));
        assert_eq!(snapshot(&t.old()), old_before);
        assert_eq!(snapshot(&t.new_dir()), new_before);
        // an empty new folder (made by something else) does not count
        let t = T::new("empty-new");
        t.fill_old(false);
        std::fs::create_dir_all(t.new_dir()).unwrap();
        assert!(matches!(t.run(None).unwrap().1, Outcome::Moved { .. }));
    }

    #[test]
    fn an_open_older_launcher_or_an_update_on_trial_keeps_the_old_folder_for_the_session() {
        let t = T::new("defer");
        t.fill_old(false);
        let before = snapshot(&t.old());
        let (dir, out) = t.run_with(None, &|| true, &|a, b| std::fs::rename(a, b)).unwrap();
        assert_eq!((dir, out), (t.old(), Outcome::Deferred { from: t.old().display().to_string(), reason: Reason::OldLauncher }));
        assert_eq!(snapshot(&t.old()), before);
        let journal = t.old().join("update").join("journal.json");
        for phase in ["swapping", "trial", "confirmed"] {
            std::fs::write(&journal, format!(r#"{{"phase":"{phase}","exe":"C:\\K\\Kalamar.exe","from":"0.1.0","to":"0.2.0","attempts":0}}"#)).unwrap();
            assert!(matches!(t.run(None).unwrap().1, Outcome::Deferred { reason: Reason::UpdateTrial, .. }), "{phase}");
        }
        // a confirmed update long ago, or a finished one, no longer holds it
        assert!(!update_in_progress(&t.old(), crate::util::now_secs() + 3600));
        std::fs::write(&journal, r#"{"phase":"done","from":"0.1.0","to":"0.2.0","shown":true}"#).unwrap();
        let (dir, out) = t.run(None).unwrap();
        assert!(matches!(out, Outcome::Moved { .. }) && dir == t.new_dir());
        assert!(t.new_dir().join("update").join("journal.json").is_file(), "the update's journal moves with the folder");
    }

    #[test]
    fn another_launcher_moving_it_or_a_refused_rename_keeps_the_old_folder() {
        let t = T::new("busy");
        t.fill_old(false);
        let before = snapshot(&t.old());
        let (dir, out) = resolve(&t.roots, &Probes { old_launcher_open: &|| false, rename: &|a, b| std::fs::rename(a, b), stop_at: None }, false, 0).unwrap();
        assert_eq!((dir, out), (t.old(), Outcome::Deferred { from: t.old().display().to_string(), reason: Reason::Busy }));
        let denied = |_: &Path, _: &Path| Err(std::io::Error::from_raw_os_error(5));
        let (dir, out) = t.run_with(None, &|| false, &denied).unwrap();
        assert_eq!((dir, out), (t.old(), Outcome::Deferred { from: t.old().display().to_string(), reason: Reason::InUse }));
        assert_eq!(snapshot(&t.old()), before, "no journal left behind");
    }

    #[cfg(windows)]
    #[test]
    fn a_file_open_in_the_old_folder_stops_the_rename_and_the_session_guard_does_too() {
        let t = T::new("open");
        t.fill_old(false);
        let held = std::fs::File::open(t.old().join("profile.json")).unwrap();
        let (dir, out) = t.run(None).unwrap();
        assert_eq!((dir, &out), (t.old(), &Outcome::Deferred { from: t.old().display().to_string(), reason: Reason::InUse }));
        assert!(!t.new_dir().exists() && t.old().join("config.json").is_file());
        drop(held);
        // the guard a deferred session holds: same effect, and its file goes with the handle
        let guard = {
            let path = t.old().join("launcher-in-use-test.lock");
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).access_mode(0xC001_0000).share_mode(7).custom_flags(0x0400_0000).open(path).unwrap()
        };
        assert!(matches!(t.run(None).unwrap().1, Outcome::Deferred { reason: Reason::InUse, .. }));
        drop(guard);
        assert!(!t.old().join("launcher-in-use-test.lock").exists());
        assert!(matches!(t.run(None).unwrap().1, Outcome::Moved { .. }));
    }

    #[test]
    #[cfg(windows)]
    fn a_game_installed_in_the_old_folder_stays_there_and_the_rest_moves() {
        let t = T::new("split");
        t.fill_old(true);
        let game_before = snapshot(&t.old().join("game"));
        let mut rest_before = without_paths(snapshot(&t.old()));
        rest_before.retain(|k, _| !Path::new(k).starts_with("game"));
        let (dir, out) = t.run(None).unwrap();
        assert_eq!(dir, t.new_dir());
        let Outcome::Moved { mode, kept, rewritten, .. } = out else { panic!("{out:?}") };
        assert_eq!((mode, kept), (Mode::Split, vec!["game".to_string()]));
        assert!(rewritten.contains(&"config.json".to_string()));
        assert_eq!(snapshot(&t.old().join("game")), game_before, "the installation is not touched");
        assert_eq!(std::fs::read_dir(t.old()).unwrap().count(), 1, "only the installation is left");
        assert_eq!(without_paths(snapshot(&t.new_dir())), rest_before);
        let c = cfg(&t.new_dir());
        assert_eq!(c["root"], t.old().join("game").join("work").join("splatoon3-11.2.0").display().to_string(), "the configuration still points at the game where it is");
        assert_eq!(c["install"]["dir"], t.old().join("game").display().to_string());
        assert_eq!(c["relay_exe"], t.new_dir().join("tools").join("relay.exe").display().to_string());
        assert!(!t.roots.staging().exists());
        // later starts: the new folder, the old one only holds the game
        assert_eq!(t.run(None).unwrap(), (t.new_dir(), Outcome::Both { from: t.old().display().to_string(), left: "game" }));
    }

    #[test]
    fn a_split_move_stopped_at_any_point_is_finished_at_the_next_start() {
        for stop in ["split:journal", "split:moved:config.json", "split:moved:online", "split:entries", "split:committed", "finish:rewritten"] {
            let t = T::new("split-stop");
            t.fill_old(true);
            let mut rest = without_paths(snapshot(&t.old()));
            rest.retain(|k, _| !Path::new(k).starts_with("game"));
            assert!(t.run(Some(stop)).is_err(), "{stop}");
            let (dir, out) = t.run(None).unwrap();
            assert_eq!(dir, t.new_dir(), "{stop}");
            assert!(matches!(out, Outcome::Moved { mode: Mode::Split, .. }), "{stop}: {out:?}");
            assert_eq!(without_paths(snapshot(&t.new_dir())), rest, "{stop}");
            assert!(t.old().join("game").join(INSTALL_MARKER).is_file(), "{stop}");
            assert_eq!(cfg(&t.new_dir())["install"]["dir"], t.old().join("game").display().to_string(), "{stop}");
            assert!(!t.roots.staging().exists() && !t.new_dir().join(JOURNAL).exists(), "{stop}");
        }
    }

    #[test]
    fn a_split_move_that_meets_a_file_in_use_puts_everything_back() {
        let t = T::new("split-undo");
        t.fill_old(true);
        let before = snapshot(&t.old());
        let refuse_online = |a: &Path, b: &Path| {
            if a.file_name().is_some_and(|n| n == "online") { Err(std::io::Error::from_raw_os_error(32)) } else { std::fs::rename(a, b) }
        };
        let (dir, out) = t.run_with(None, &|| false, &refuse_online).unwrap();
        assert_eq!((dir, out), (t.old(), Outcome::Deferred { from: t.old().display().to_string(), reason: Reason::InUse }));
        assert_eq!(snapshot(&t.old()), before);
        assert!(!t.roots.staging().exists() && !t.new_dir().exists());
    }

    #[test]
    fn a_game_installed_in_the_folder_itself_keeps_the_folder_where_it_is() {
        let t = T::new("inroot");
        t.fill_old(false);
        let o = t.old();
        std::fs::write(o.join("config.json"), format!(r#"{{"install":{{"dir":"{}"}}}}"#, o.display().to_string().replace('\\', "\\\\"))).unwrap();
        let before = snapshot(&o);
        assert_eq!(t.run(None).unwrap(), (o.clone(), Outcome::Deferred { from: o.display().to_string(), reason: Reason::GameInFolder }));
        std::fs::write(o.join("config.json"), "{}").unwrap();
        std::fs::write(o.join(INSTALL_MARKER), "{}").unwrap();
        assert!(matches!(t.run(None).unwrap().1, Outcome::Deferred { reason: Reason::GameInFolder, .. }));
        assert_eq!(snapshot(&o).len(), before.len() + 1);
    }

    #[test]
    fn across_volumes_the_folder_is_copied_checked_and_left_in_place() {
        let t = T::new("copy");
        t.fill_old(true);
        #[cfg(windows)]
        let linked = {
            // a junction inside the old folder, to a folder outside it: never followed
            let target = t.base.join("outside");
            std::fs::create_dir_all(&target).unwrap();
            std::fs::write(target.join("big.bin"), b"outside").unwrap();
            std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(t.old().join("link")).arg(&target).output().is_ok_and(|o| o.status.success())
        };
        let before = snapshot(&t.old().join("online"));
        let cross = |a: &Path, b: &Path| if a == t.old() { Err(std::io::Error::from_raw_os_error(17)) } else { std::fs::rename(a, b) };
        // it was the whole folder's rename that was refused: no game anchors in this config
        std::fs::write(t.old().join("config.json"), r#"{"language":"fr"}"#).unwrap();
        std::fs::remove_file(t.old().join("game").join(INSTALL_MARKER)).unwrap();
        for stop in [Some("copy:journal"), Some("copy:copied"), None] {
            let r = t.run_with(stop, &|| false, &cross);
            if stop.is_some() {
                assert!(r.is_err());
                continue;
            }
            let (dir, out) = r.unwrap();
            assert_eq!(dir, t.new_dir());
            let Outcome::Moved { mode, skipped, .. } = out else { panic!("{out:?}") };
            assert_eq!(mode, Mode::Copy);
            #[cfg(windows)]
            if linked {
                assert_eq!(skipped, ["link"]);
                assert!(!t.new_dir().join("link").exists());
            }
            let _ = skipped;
        }
        assert_eq!(snapshot(&t.new_dir().join("online")), before);
        assert!(t.old().join("config.json").is_file(), "the source is left as it was");
        assert!(!t.roots.staging().exists());
        assert_eq!(t.run(None).unwrap(), (t.new_dir(), Outcome::Both { from: t.old().display().to_string(), left: "copy" }));
    }

    #[test]
    fn a_staging_folder_without_a_journal_is_not_touched() {
        let t = T::new("foreign");
        std::fs::create_dir_all(t.roots.staging().join("x")).unwrap();
        assert_eq!(t.run(None).unwrap().1, Outcome::Fresh);
        assert!(t.roots.staging().join("x").is_dir());
    }

    #[test]
    fn paths_are_rewritten_in_both_styles_but_not_into_kept_entries_or_other_folders() {
        let (from, to) = (Path::new(r"C:\Users\A\AppData\Roaming\dev.splat3recomp.launcher"), Path::new(r"C:\Users\A\AppData\Roaming\dev.kalamar.launcher"));
        let text = r#"{"a":"C:\\Users\\A\\AppData\\Roaming\\dev.splat3recomp.launcher\\tools\\r.exe","b":"c:/users/a/appdata/roaming/DEV.SPLAT3RECOMP.LAUNCHER/x","c":"C:\\Users\\A\\AppData\\Roaming\\dev.splat3recomp.launcher","d":"C:\\Users\\A\\AppData\\Roaming\\dev.splat3recomp.launcher\\game\\work","e":"C:\\Users\\A\\AppData\\Roaming\\dev.splat3recomp.launcher2\\y"}"#;
        let out = rewrite_text(text, from, to, &["game".into()]).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["a"], r"C:\Users\A\AppData\Roaming\dev.kalamar.launcher\tools\r.exe");
        assert_eq!(v["b"], "C:/Users/A/AppData/Roaming/dev.kalamar.launcher/x");
        assert_eq!(v["c"], r"C:\Users\A\AppData\Roaming\dev.kalamar.launcher");
        assert_eq!(v["d"], r"C:\Users\A\AppData\Roaming\dev.splat3recomp.launcher\game\work");
        assert_eq!(v["e"], r"C:\Users\A\AppData\Roaming\dev.splat3recomp.launcher2\y");
        assert_eq!(rewrite_text(r#"{"z":"D:\\elsewhere"}"#, from, to, &[]), None);
        assert_eq!(under(Path::new(r"c:\users\a\appdata\roaming\dev.splat3recomp.launcher\game\w"), from), Some(PathBuf::from(r"game\w")));
        assert_eq!(under(Path::new(r"C:\Users\A\AppData\Roaming\dev.splat3recomp.launcherX"), from), None);
    }

    #[test]
    fn the_log_keeps_dated_lines() {
        let t = T::new("log");
        log(&t.base, "one");
        log(&t.base, "two");
        let text = std::fs::read_to_string(t.base.join(LOG)).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.lines().nth(1).unwrap().ends_with("two"));
        assert!(describe(&Outcome::Deferred { from: "X".into(), reason: Reason::OldLauncher }).contains("next start"));
    }
}
