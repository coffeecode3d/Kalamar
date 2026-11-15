//! Launching the game and tracking the child process.
//!
//! A launch, for root `W` and `U = W\userdata`:
//!   * creates `U\logs` and `U\profile`;
//!   * sets TOTK_SHADER_CACHE=U\shader-cache, TOTK_PIPELINE_CACHE=U\pipeline-cache.bin, TOTK_ROMFS_PROOF_DIR=U,
//!     TOTK_XLATE=<parent of W>\xlate\totk_xlate.exe (everything else in the environment is inherited);
//!   * starts `W\build\splat3.exe <extra args>` with working directory U, stderr to `U\logs\run-<yyyyMMdd-HHmmss>.log`
//!     and stdout to the same name plus `.out`, without a new window.

use crate::error::{AppError, AppResult};
use crate::install;
use crate::util;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const ENV_SHADER_CACHE: &str = "TOTK_SHADER_CACHE";
pub const ENV_PIPELINE_CACHE: &str = "TOTK_PIPELINE_CACHE";
pub const ENV_ROMFS_PROOF_DIR: &str = "TOTK_ROMFS_PROOF_DIR";
pub const ENV_XLATE: &str = "TOTK_XLATE";
/// Where the runtime keeps saves and caches. The runtime has a path baked in at build time; passing the launchers own
/// userdata profile folder keeps a launch inside the user data of the root it was started from.
pub const ENV_PROFILE_DIR: &str = "TOTK_PROFILE_DIR";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub exe: PathBuf,
    pub cwd: PathBuf,
    pub args: Vec<String>,
    /// Variables to set on top of the inherited environment, in a fixed order, then the caller's extras.
    pub env: Vec<(String, String)>,
    /// Variables removed from the inherited environment first, so a card behaves the same whatever the launcher was
    /// started from (a stray `SPLAT3_LAN` in the shell must not turn the Hub card into a LAN launch).
    pub env_remove: Vec<String>,
    pub stderr_log: PathBuf,
    pub stdout_log: PathBuf,
    pub mode: String,
}

/// Variables the launcher owns: removed from the inherited environment before a launch sets its own.
pub const OWNED_ENV: &[&str] = &[
    "SPLAT3_START", "TOTK_START_SCENE", "SPLAT3_LAN", "SPLAT3_LAN_ROOM", "SPLAT3_LAN_NAME", "SPLAT3_LAN_BIND", "SPLAT3_NET_LOG",
    "SPLAT3_NICKNAME", "TOTK_PLAYER_NAME", "TOTK_SAVE_PROFILE", "SPLAT3_WINDOW_TITLE", "TOTK_INPUT_SCRIPT",
    // private match (automatch.rs; a renamed variable is stripped too, see commands::launch_with)
    "SPLAT3_MATCH", "SPLAT3_MATCH_ID", "SPLAT3_MATCH_RULES", "SPLAT3_MATCH_HOST", "SPLAT3_MATCH_RESULT", "SPLAT3_MATCH_EXIT", "SPLAT3_MATCH_STATUS",
    "SPLAT3_MATCH_REVEAL", "SPLAT3_MATCH_WAIT", "SPLAT3_MATCH_SHOTS", "SPLAT3_MATCH_PLAY", "SPLAT3_MATCH_ON_ERROR", "SPLAT3_MATCH_TIMEOUTS", "SPLAT3_MATCH_DIAG", "SPLAT3_MATCH_CMD", "SPLAT3_MATCH_LOADOUT", "SPLAT3_MATCH_STAGES",
    // Discord presence (discord/mod.rs): set below for every launch; the engine's test pipe name never reaches a game
    "SPLAT3_DISCORD_PRESENCE", "SPLAT3_DISCORD_IPC",
];

/// The prefix the runtime documents its variables under and reads first (`KLM_FPS_LIMIT`, `KLM_MATCH`).
pub const ENV_PREFIX: &str = "KLM_";
/// The older prefixes the runtime still reads after the neutral one: the upstream's `TOTK_` and the earlier
/// `SPLAT3_`. The launcher's own tables keep these names; [`runtime_env`] and [`env_names`] turn them into what a launch
/// sets and removes, [`env_name`] into what the interface shows.
pub const LEGACY_ENV_PREFIXES: [&str; 2] = ["TOTK_", "SPLAT3_"];

/// The name the runtime documents for a variable: `KLM_<NAME>` for `TOTK_<NAME>` and `SPLAT3_<NAME>`, any other name
/// as it is.
pub fn env_name(name: &str) -> String {
    LEGACY_ENV_PREFIXES.iter().find_map(|p| name.strip_prefix(p)).map_or_else(|| name.to_string(), |rest| format!("{ENV_PREFIX}{rest}"))
}

/// Every name the runtime may read a variable under (the neutral one and the older ones): what removing it removes.
pub fn env_names(name: &str) -> Vec<String> {
    let rest = std::iter::once(&ENV_PREFIX).chain(LEGACY_ENV_PREFIXES.iter()).find_map(|p| name.strip_prefix(p));
    match rest {
        Some(rest) => std::iter::once(ENV_PREFIX).chain(LEGACY_ENV_PREFIXES).map(|p| format!("{p}{rest}")).collect(),
        None => vec![name.to_string()],
    }
}

/// The environment a launch sets: each runtime variable under its neutral name, and under its older name too for a game
/// compiled before the runtime read the neutral names (until it is recompiled; a current one reads the neutral name
/// first, so both carry the same value). Other variables as they are.
pub fn runtime_env(env: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(env.len() * 2);
    for (k, v) in env {
        let neutral = env_name(k);
        if neutral != *k {
            out.push((neutral, v.clone()));
        }
        out.push((k.clone(), v.clone()));
    }
    out
}

/// The variables of a launch as the interface shows them: under their documented (neutral) names.
pub fn shown_env(env: &[(String, String)]) -> Vec<(String, String)> {
    env.iter().map(|(k, v)| (env_name(k), v.clone())).collect()
}

/// Applies a launch's removals and variables to a command (see [`runtime_env`]).
pub fn apply_env(cmd: &mut Command, remove: &[String], env: &[(String, String)]) {
    for k in remove.iter().flat_map(|k| env_names(k)) {
        cmd.env_remove(k);
    }
    for (k, v) in runtime_env(env) {
        cmd.env(k, v);
    }
}

/// Timestamp used in log names, `yyyyMMdd-HHmmss` in local time.
pub fn log_stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Pure builder of the launch description; touches no file.
pub fn build_launch_spec(
    root: &Path,
    u: &Path,
    translator_override: Option<&Path>,
    extra_args: &[String],
    extra_env: &[(String, String)],
    stamp: &str,
    mode: &str,
) -> LaunchSpec {
    let log = u.join("logs").join(format!("run-{stamp}.log"));
    let mut out_name = log.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    out_name.push(".out");
    let mut env = vec![
        (ENV_SHADER_CACHE.to_string(), u.join("shader-cache").display().to_string()),
        (ENV_PIPELINE_CACHE.to_string(), u.join("pipeline-cache.bin").display().to_string()),
        (ENV_ROMFS_PROOF_DIR.to_string(), u.display().to_string()),
        (ENV_XLATE.to_string(), install::translator_path(root, translator_override).display().to_string()),
        (ENV_PROFILE_DIR.to_string(), u.join("profile").display().to_string()),
        // The launcher is the one source of the Discord presence: the game it starts never opens Discord's pipe
        // (`runtime/discord_rpc.c`), whether the presence is on or not.
        (crate::discord::ENV_PRESENCE.to_string(), crate::discord::ENV_PRESENCE_LAUNCHER.to_string()),
    ];
    let mut args = vec!["--profile".to_string(), u.join("profile").display().to_string()];
    args.extend(extra_args.iter().cloned());
    env.extend(extra_env.iter().cloned());
    LaunchSpec {
        exe: install::exe_path(root),
        cwd: u.to_path_buf(),
        args,
        env,
        env_remove: OWNED_ENV.iter().map(|s| s.to_string()).collect(),
        stdout_log: log.with_file_name(out_name),
        stderr_log: log,
        mode: mode.to_string(),
    }
}

/// Splits a user-typed argument string on whitespace (no quoting) and refuses control characters.
pub fn parse_extra_args(s: &str) -> AppResult<Vec<String>> {
    if s.len() > 1024 || s.chars().any(|c| c.is_control() && c != '\t') {
        return Err(AppError::InvalidInput("extra arguments: too long or contain control characters".into()));
    }
    Ok(s.split_whitespace().map(str::to_string).collect())
}

/// Ids (as in [`install::Check`]) of the required parts of the install that are missing; the launch is refused then.
pub fn missing_for_launch(root: &Path, translator_override: Option<&Path>) -> Vec<String> {
    install::detect_light(Some(root), translator_override).missing.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Idle,
    Running,
    Exited,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameStatus {
    pub state: RunState,
    pub pid: Option<u32>,
    pub mode: Option<String>,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    pub exit_code: Option<i32>,
    pub stopped_by_user: bool,
    /// Exit code other than 0, and not stopped from the launcher.
    pub abnormal: bool,
    pub log_path: Option<String>,
    pub stdout_path: Option<String>,
    /// Last lines of the log, filled when the game ended abnormally.
    pub tail: Option<String>,
    /// The game started itself again once (first launch: the profile goes into the new save, then it restarts). The
    /// new process is not a child of the launcher; "stop" still ends it.
    pub restarted: bool,
}

impl GameStatus {
    pub fn idle() -> Self {
        GameStatus {
            state: RunState::Idle,
            pid: None,
            mode: None,
            started_at: None,
            ended_at: None,
            exit_code: None,
            stopped_by_user: false,
            abnormal: false,
            log_path: None,
            stdout_path: None,
            tail: None,
            restarted: false,
        }
    }
}

struct Shared {
    status: GameStatus,
    child: Option<Child>,
    stop_requested: bool,
    exe: Option<PathBuf>,
    /// The `--profile` folder of the running instance.
    needle: Option<String>,
    /// Tells the UI when a stop changes the status without a child exiting (the game that restarted itself).
    notify: Option<Notifier>,
}

/// Line the runtime logs right before it starts the game again (`runtime/save_profile.c`).
pub const RESTART_LINE: &str = "restarting so the game loads the new save";

/// The script of [`kill_same_exe`]: its values come from its environment.
#[cfg(windows)]
const KILL_SCRIPT: &str = "Get-CimInstance Win32_Process -Filter \"Name='splat3.exe'\" | Where-Object { $_.ExecutablePath -eq $env:KLM_KILL_PATH -and $_.CommandLine -and $_.CommandLine.Contains($env:KLM_KILL_NEEDLE) } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }";

/// Ends every process running exactly this executable whose command line holds `needle` (the restarted game is not our
/// child; the needle is its `--profile` folder, which tells two instances apart). Matches on the full path, so another
/// build of the game is never touched.
fn kill_same_exe(exe: &Path, needle: &str) {
    #[cfg(not(windows))]
    {
        // The same scan as `ps`, over /proc: the executable's real path and the arguments after NUL separators.
        let want = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
        let Ok(rd) = std::fs::read_dir("/proc") else { return };
        for e in rd.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<i32>().ok()) else { continue };
            if pid == std::process::id() as i32 {
                continue;
            }
            if std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|p| p == want)
                && std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c.split(|b| *b == 0).any(|a| a == needle.as_bytes()))
            {
                // SAFETY: a plain signal to a process the scan above matched by its executable and arguments.
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
        }
        return;
    }
    #[cfg(windows)]
    {
        let mut cmd = Command::new(crate::winutil::system_exe(r"WindowsPowerShell\v1.0\powershell.exe"));
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", KILL_SCRIPT]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        // The path and the needle reach the script as environment variables, never inside its text: no quoting to get
        // right (PowerShell also takes the typographic quotes U+2018 to U+201B as quotes).
        cmd.env("KLM_KILL_PATH", exe.as_os_str()).env("KLM_KILL_NEEDLE", needle);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let _ = cmd.status();
    }
}

pub type Notifier = Arc<dyn Fn(&GameStatus) + Send + Sync>;

#[derive(Clone)]
pub struct GameManager {
    shared: Arc<Mutex<Shared>>,
}

impl Default for GameManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Tail lines shown after an abnormal exit: stderr log, or the stdout companion when stderr is empty.
pub fn failure_tail(stderr_log: &Path, stdout_log: &Path) -> Option<String> {
    for p in [stderr_log, stdout_log] {
        if let Ok(Some(t)) = util::tail_lines(p, 40)
            && !t.trim().is_empty() {
                return Some(t);
            }
    }
    None
}

impl GameManager {
    pub fn new() -> Self {
        GameManager { shared: Arc::new(Mutex::new(Shared { status: GameStatus::idle(), child: None, stop_requested: false, exe: None, needle: None, notify: None })) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn status(&self) -> GameStatus {
        self.lock().status.clone()
    }

    /// Starts the child process. Refuses when one is already running.
    pub fn launch(&self, spec: &LaunchSpec, notify: Notifier) -> AppResult<GameStatus> {
        let mut g = self.lock();
        if g.status.state == RunState::Running {
            return Err(AppError::AlreadyRunning);
        }
        if !spec.exe.is_file() {
            return Err(AppError::NotInstalled(vec!["exe".into()]));
        }
        for dir in [spec.cwd.join("logs"), spec.cwd.join("profile")] {
            std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, &e))?;
        }
        let err = std::fs::File::create(&spec.stderr_log).map_err(|e| AppError::io(&spec.stderr_log, &e))?;
        let out = std::fs::File::create(&spec.stdout_log).map_err(|e| AppError::io(&spec.stdout_log, &e))?;
        let mut cmd = Command::new(&spec.exe);
        cmd.args(&spec.args).current_dir(&spec.cwd).stdin(Stdio::null()).stdout(out).stderr(err);
        apply_env(&mut cmd, &spec.env_remove, &spec.env);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let child = cmd.spawn().map_err(|e| AppError::io(&spec.exe, &e))?;
        g.status = GameStatus {
            state: RunState::Running,
            pid: Some(child.id()),
            mode: Some(spec.mode.clone()),
            started_at: Some(util::now_secs()),
            log_path: Some(spec.stderr_log.display().to_string()),
            stdout_path: Some(spec.stdout_log.display().to_string()),
            ..GameStatus::idle()
        };
        g.child = Some(child);
        g.exe = Some(spec.exe.clone());
        g.needle = spec.args.iter().position(|a| a == "--profile").and_then(|i| spec.args.get(i + 1)).cloned();
        g.stop_requested = false;
        g.notify = Some(notify.clone());
        let snapshot = g.status.clone();
        drop(g);
        notify(&snapshot);

        let me = self.clone();
        let (stderr_log, stdout_log) = (spec.stderr_log.clone(), spec.stdout_log.clone());
        std::thread::spawn(move || me.watch(&stderr_log, &stdout_log, notify));
        Ok(snapshot)
    }

    fn watch(&self, stderr_log: &Path, stdout_log: &Path, notify: Notifier) {
        let exit = loop {
            {
                let mut g = self.lock();
                match g.child.as_mut().map(|c| c.try_wait()) {
                    Some(Ok(Some(st))) => break Ok(st),
                    Some(Ok(None)) => {}
                    Some(Err(e)) => break Err(e),
                    None => return,
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        let mut g = self.lock();
        g.child = None;
        let code = exit.ok().and_then(|s| s.code());
        let stopped = g.stop_requested;
        let abnormal = !stopped && code != Some(0);
        g.status.state = RunState::Exited;
        g.status.ended_at = Some(util::now_secs());
        g.status.exit_code = code;
        g.status.stopped_by_user = stopped;
        g.status.abnormal = abnormal;
        g.status.tail = if abnormal { failure_tail(stderr_log, stdout_log) } else { None };
        g.status.restarted = !abnormal && util::tail_lines(stderr_log, 400).ok().flatten().is_some_and(|t| t.contains(RESTART_LINE));
        let snapshot = g.status.clone();
        drop(g);
        notify(&snapshot);
    }

    /// Kills the running game (the "stop" button), and the process it restarted into, if any.
    pub fn stop(&self) -> AppResult<()> {
        let mut g = self.lock();
        let exe = g.exe.clone();
        let needle = g.needle.clone().unwrap_or_default();
        let restarted = g.status.restarted;
        if g.child.is_none() && !restarted {
            return Err(AppError::NotRunning);
        }
        g.stop_requested = true;
        let killed = g.child.as_mut().map(|c| c.kill());
        let mut after = None;
        if restarted {
            g.status.restarted = false;
            g.status.stopped_by_user = true;
            g.status.ended_at = Some(util::now_secs());
            after = g.notify.clone().map(|n| (n, g.status.clone()));
        }
        drop(g);
        if restarted
            && let Some(e) = exe
        {
            kill_same_exe(&e, &needle);
        }
        if let Some((notify, snapshot)) = after {
            notify(&snapshot);
        }
        match killed {
            Some(Err(e)) => Err(AppError::Internal(format!("cannot stop the game: {e}"))),
            _ => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// A match found while a game is open

/// The `mode` of a launch that plays a match (private match code or community server): the game of such a launch is
/// a match in progress, never closed for another one.
pub const MATCH_MODE: &str = "match";

/// How long a game asked to close (its window's close request) has to end by itself before it is terminated.
pub const CLOSE_GRACE: Duration = Duration::from_secs(6);
/// How long a terminated game has to be gone before closing it counts as failed.
pub const CLOSE_FORCE_WAIT: Duration = Duration::from_secs(3);

/// What the game that is open (if any) stands for when a match needs the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenGame {
    /// No game of the launcher runs.
    Nothing,
    /// A game started for solo play (or any mode but a match): it cannot play the match and is closed for it.
    Solo,
    /// The launcher's own match in progress: left alone.
    Match,
}

/// Classifies the first player's game by the launcher's own state: running (or restarted into its second process) and
/// started by a match launch, or not.
pub fn open_game(s: &GameStatus) -> OpenGame {
    if s.state != RunState::Running && !s.restarted {
        OpenGame::Nothing
    } else if s.mode.as_deref() == Some(MATCH_MODE) {
        OpenGame::Match
    } else {
        OpenGame::Solo
    }
}

/// How an open game was dealt with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    /// Nothing was open.
    Nothing,
    /// A match in progress: not touched.
    KeptMatch,
    /// The game ended by itself when asked to close.
    Clean,
    /// The game did not end in time and was terminated.
    Forced,
}

/// What [`close_open_game`] needs of the game it closes.
pub trait GameCloser {
    fn open_game(&self) -> OpenGame;
    /// Asks the game to close the way its window's close button does; `true` when the request was made.
    fn ask_to_close(&self) -> bool;
    /// The game still runs.
    fn is_open(&self) -> bool;
    /// Terminates the game (only the one this launcher started, and the process it restarted into).
    fn force_close(&self) -> AppResult<()>;
}

fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = std::time::Instant::now() + limit;
    loop {
        if done() {
            return true;
        }
        if std::time::Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Closes a game opened for solo play so that a match can start: ask it to close and wait `grace` for it to end by
/// itself, else terminate it and wait `force_wait` for it to be gone. A match in progress is never touched. `Err` when
/// the game is still there at the end (the match cannot start then).
pub fn close_open_game(g: &dyn GameCloser, grace: Duration, force_wait: Duration) -> AppResult<Closed> {
    match g.open_game() {
        OpenGame::Nothing => return Ok(Closed::Nothing),
        OpenGame::Match => return Ok(Closed::KeptMatch),
        OpenGame::Solo => {}
    }
    if g.ask_to_close() && wait_until(grace, || !g.is_open()) {
        return Ok(Closed::Clean);
    }
    if let Err(e) = g.force_close()
        && g.is_open()
    {
        return Err(AppError::Internal(format!("the game that is open could not be closed: {e}")));
    }
    if wait_until(force_wait, || !g.is_open()) {
        Ok(Closed::Forced)
    } else {
        Err(AppError::Internal("the game that is open did not end when it was terminated".into()))
    }
}

impl GameCloser for GameManager {
    fn open_game(&self) -> OpenGame {
        open_game(&self.status())
    }
    fn ask_to_close(&self) -> bool {
        let mut g = self.lock();
        // The game ends because the launcher asked: its exit is not a crash (no failure tail, no "abnormal").
        g.stop_requested = true;
        let (pid, exe) = (g.status.pid, g.exe.clone());
        drop(g);
        exe.is_some_and(|e| crate::winutil::request_close(pid, &e))
    }
    fn is_open(&self) -> bool {
        let s = self.status();
        s.state == RunState::Running || s.restarted
    }
    fn force_close(&self) -> AppResult<()> {
        match self.stop() {
            Err(AppError::NotRunning) => Ok(()),
            r => r,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path as the launch specification holds it (`display()`).
    fn shown(p: &Path) -> String {
        p.display().to_string()
    }

    // The whole launch specification for a game root.
    #[test]
    fn spec_of_a_plain_launch() {
        let root = Path::new(r"C:\work\game-root");
        let userdata = install::userdata_dir(root);
        let spec = build_launch_spec(root, &userdata, None, &["--seconds".into(), "30".into()], &[], "20261110-020318", "hub");
        assert_eq!(spec.exe, install::exe_path(root));
        assert_eq!(spec.cwd, userdata); // the working directory is the user data folder
        assert_eq!(spec.args, vec!["--profile".to_string(), shown(&userdata.join("profile")), "--seconds".into(), "30".into()]); // --profile, then the extra arguments
        assert_eq!(spec.stderr_log, userdata.join("logs").join("run-20261110-020318.log"));
        assert_eq!(spec.stdout_log, userdata.join("logs").join("run-20261110-020318.log.out"));
        let env: std::collections::BTreeMap<_, _> = spec.env.iter().cloned().collect();
        assert_eq!(env["TOTK_SHADER_CACHE"], shown(&userdata.join("shader-cache")));
        assert_eq!(env["TOTK_PIPELINE_CACHE"], shown(&userdata.join("pipeline-cache.bin")));
        assert_eq!(env["TOTK_ROMFS_PROOF_DIR"], shown(&userdata));
        assert_eq!(env["TOTK_XLATE"], shown(&install::translator_path(root, None))); // next to the game root
        assert_eq!(env["TOTK_PROFILE_DIR"], shown(&userdata.join("profile"))); // keeps the launch inside this root's user data
        // the launcher shows the Discord presence, the game it starts stays quiet
        assert_eq!(env["SPLAT3_DISCORD_PRESENCE"], "launcher");
        assert_eq!(spec.env.len(), 6);
        assert_eq!(spec.env[0].0, "TOTK_SHADER_CACHE"); // same order as the script
    }

    #[test]
    fn translator_override_and_extra_env() {
        let spec = build_launch_spec(
            Path::new(r"C:\w\r"),
            Path::new(r"C:\w\r\userdata-p2"),
            Some(Path::new(r"D:\tools\xlate.exe")),
            &[],
            &[("TOTK_LAN".into(), "lan".into())],
            "s",
            "lan",
        );
        let env: std::collections::BTreeMap<_, _> = spec.env.iter().cloned().collect();
        assert_eq!(env["TOTK_XLATE"], r"D:\tools\xlate.exe");
        assert_eq!(env["TOTK_LAN"], "lan");
        // a second instance: its own working directory, logs, caches and profile folder, same translator
        let ud = Path::new(r"C:\w\r\userdata-p2");
        assert_eq!(spec.cwd, ud);
        assert_eq!(env["TOTK_PROFILE_DIR"], shown(&ud.join("profile")));
        assert_eq!(env["TOTK_SHADER_CACHE"], shown(&ud.join("shader-cache")));
        assert_eq!(spec.args, vec!["--profile".to_string(), shown(&ud.join("profile"))]);
        assert!(spec.stderr_log.starts_with(&ud.join("logs")));
        assert!(spec.env_remove.iter().any(|v| v == "SPLAT3_START") && spec.env_remove.iter().any(|v| v == "SPLAT3_LAN"));
        // every launch, the second player's too, keeps the game off Discord's pipe; an inherited test pipe name goes
        assert_eq!(env["SPLAT3_DISCORD_PRESENCE"], "launcher");
        assert!(spec.env_remove.iter().any(|v| v == "SPLAT3_DISCORD_IPC"));
    }

    #[test]
    fn runtime_variables_go_under_the_neutral_name_and_the_older_one() {
        assert_eq!(env_name("SPLAT3_MATCH_RULES"), "KLM_MATCH_RULES");
        assert_eq!(env_name("TOTK_SAVE_PROFILE"), "KLM_SAVE_PROFILE");
        assert_eq!(env_name("KLM_LANG"), "KLM_LANG");
        assert_eq!(env_name("PATH"), "PATH");
        assert_eq!(env_names("SPLAT3_LAN"), ["KLM_LAN", "TOTK_LAN", "SPLAT3_LAN"]);
        assert_eq!(env_names("KLM_LANG"), ["KLM_LANG", "TOTK_LANG", "SPLAT3_LANG"]);
        assert_eq!(env_names("OTHER"), ["OTHER"]);
        let env = runtime_env(&[("SPLAT3_START".into(), "hub".into()), ("TOTK_XLATE".into(), "x.exe".into()), ("KLM_LANG".into(), "fr".into()), ("RUST_LOG".into(), "1".into())]);
        let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(pairs, [("KLM_START", "hub"), ("SPLAT3_START", "hub"), ("KLM_XLATE", "x.exe"), ("TOTK_XLATE", "x.exe"), ("KLM_LANG", "fr"), ("RUST_LOG", "1")]);
        // what the interface shows: the documented names only
        let shown = shown_env(&[("SPLAT3_START".into(), "hub".into()), ("PATH".into(), "p".into())]);
        assert_eq!(shown, [("KLM_START".to_string(), "hub".to_string()), ("PATH".to_string(), "p".to_string())]);
    }

    #[test]
    fn extra_args_parsing() {
        assert_eq!(parse_extra_args("  --boot   --seconds 30 ").unwrap(), vec!["--boot", "--seconds", "30"]);
        assert!(parse_extra_args("").unwrap().is_empty());
        assert!(parse_extra_args("a\u{0}b").is_err());
        assert!(parse_extra_args(&"x".repeat(2000)).is_err());
    }

    #[test]
    fn missing_install_is_reported() {
        let miss = missing_for_launch(Path::new(r"Z:\nowhere\root"), None);
        assert!(miss.contains(&"root".to_string()));
        assert!(miss.contains(&"exe".to_string()));
    }

    fn running(mode: &str, restarted: bool, state: RunState) -> GameStatus {
        GameStatus { state, mode: Some(mode.into()), pid: Some(1), restarted, ..GameStatus::idle() }
    }

    #[test]
    fn what_the_open_game_stands_for() {
        assert_eq!(open_game(&GameStatus::idle()), OpenGame::Nothing);
        assert_eq!(open_game(&running("hub", false, RunState::Exited)), OpenGame::Nothing);
        for mode in ["hub", "original", "range", "lan"] {
            assert_eq!(open_game(&running(mode, false, RunState::Running)), OpenGame::Solo, "{mode}");
        }
        assert_eq!(open_game(&running("match", false, RunState::Running)), OpenGame::Match);
        // the game that restarted itself is still the game of its mode
        assert_eq!(open_game(&running("original", true, RunState::Exited)), OpenGame::Solo);
        assert_eq!(open_game(&running("match", true, RunState::Exited)), OpenGame::Match);
    }

    /// A game that records what is done to it. `obeys`: it ends when asked to close; `dies`: it ends when terminated.
    struct Fake {
        kind: OpenGame,
        obeys: bool,
        dies: bool,
        open: std::sync::atomic::AtomicBool,
        asked: std::sync::atomic::AtomicUsize,
        forced: std::sync::atomic::AtomicUsize,
    }

    impl Fake {
        fn new(kind: OpenGame, obeys: bool, dies: bool) -> Fake {
            Fake { kind, obeys, dies, open: (kind != OpenGame::Nothing).into(), asked: 0.into(), forced: 0.into() }
        }
        fn calls(&self) -> (usize, usize) {
            (self.asked.load(std::sync::atomic::Ordering::SeqCst), self.forced.load(std::sync::atomic::Ordering::SeqCst))
        }
    }

    impl GameCloser for Fake {
        fn open_game(&self) -> OpenGame {
            self.kind
        }
        fn ask_to_close(&self) -> bool {
            self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.obeys {
                self.open.store(false, std::sync::atomic::Ordering::SeqCst);
            }
            true
        }
        fn is_open(&self) -> bool {
            self.open.load(std::sync::atomic::Ordering::SeqCst)
        }
        fn force_close(&self) -> AppResult<()> {
            self.forced.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.dies {
                self.open.store(false, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            } else {
                Err(AppError::Internal("access denied".into()))
            }
        }
    }

    const SHORT: Duration = Duration::from_millis(250);

    #[test]
    fn no_game_open_nothing_is_done() {
        let g = Fake::new(OpenGame::Nothing, true, true);
        assert_eq!(close_open_game(&g, SHORT, SHORT).unwrap(), Closed::Nothing);
        assert_eq!(g.calls(), (0, 0));
    }

    #[test]
    fn a_solo_game_is_asked_to_close_first() {
        let g = Fake::new(OpenGame::Solo, true, true);
        assert_eq!(close_open_game(&g, SHORT, SHORT).unwrap(), Closed::Clean);
        assert_eq!(g.calls(), (1, 0), "asked, never terminated");
    }

    #[test]
    fn a_solo_game_that_does_not_close_is_terminated() {
        let g = Fake::new(OpenGame::Solo, false, true);
        assert_eq!(close_open_game(&g, SHORT, SHORT).unwrap(), Closed::Forced);
        assert_eq!(g.calls(), (1, 1));
    }

    #[test]
    fn our_own_match_is_never_touched() {
        let g = Fake::new(OpenGame::Match, true, true);
        assert_eq!(close_open_game(&g, SHORT, SHORT).unwrap(), Closed::KeptMatch);
        assert_eq!(g.calls(), (0, 0));
        assert!(g.is_open());
    }

    /// A solo game whose termination is accepted but which never goes away.
    struct Stuck;

    impl GameCloser for Stuck {
        fn open_game(&self) -> OpenGame {
            OpenGame::Solo
        }
        fn ask_to_close(&self) -> bool {
            false
        }
        fn is_open(&self) -> bool {
            true
        }
        fn force_close(&self) -> AppResult<()> {
            Ok(())
        }
    }

    #[test]
    fn a_game_that_will_not_end_is_an_error() {
        // terminating is refused and the game is still there
        let g = Fake::new(OpenGame::Solo, false, false);
        let e = close_open_game(&g, SHORT, SHORT).unwrap_err().to_string();
        assert!(e.contains("could not be closed") && e.contains("access denied"), "{e}");
        assert_eq!(g.calls(), (1, 1));
        // terminating is accepted but the game is still there after the wait
        let e = close_open_game(&Stuck, SHORT, SHORT).unwrap_err().to_string();
        assert!(e.contains("did not end"), "{e}");
    }

    #[test]
    fn a_game_gone_when_the_terminate_fails_is_closed() {
        // it ended between the request and the terminate: nothing left to do
        struct Gone(std::sync::atomic::AtomicBool);
        impl GameCloser for Gone {
            fn open_game(&self) -> OpenGame {
                OpenGame::Solo
            }
            fn ask_to_close(&self) -> bool {
                false
            }
            fn is_open(&self) -> bool {
                self.0.load(std::sync::atomic::Ordering::SeqCst)
            }
            fn force_close(&self) -> AppResult<()> {
                self.0.store(false, std::sync::atomic::Ordering::SeqCst);
                Err(AppError::Internal("already gone".into()))
            }
        }
        assert_eq!(close_open_game(&Gone(true.into()), SHORT, SHORT).unwrap(), Closed::Forced);
    }

    #[cfg(windows)]
    mod process {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};

        fn system_exe(name: &str) -> PathBuf {
            let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
            root.join("System32").join(name)
        }

        fn spec_for(tag: &str, exe: PathBuf, args: &[&str]) -> (LaunchSpec, PathBuf) {
            let dir = std::env::temp_dir().join(format!("s3l-proc-{tag}-{}", util::random_hex(4)));
            let cwd = dir.join("userdata");
            let spec = LaunchSpec {
                exe,
                cwd: cwd.clone(),
                args: args.iter().map(|s| s.to_string()).collect(),
                env: vec![("S3L_TEST_VAR".into(), "hello".into())],
                env_remove: vec!["S3L_TEST_GONE".into()],
                stderr_log: cwd.join("logs").join("run-t.log"),
                stdout_log: cwd.join("logs").join("run-t.log.out"),
                mode: "solo".into(),
            };
            std::fs::create_dir_all(cwd.join("logs")).unwrap();
            (spec, dir)
        }

        fn wait_exit(m: &GameManager) -> GameStatus {
            for _ in 0..100 {
                let s = m.status();
                if s.state == RunState::Exited {
                    return s;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            panic!("child did not exit");
        }

        #[test]
        fn exit_code_stdout_env_and_abnormal_tail() {
            // SAFETY: a variable only this test reads, set before the child starts.
            unsafe { std::env::set_var("S3L_TEST_GONE", "inherited") };
            let (spec, dir) = spec_for("code", system_exe("cmd.exe"), &["/C", "echo %S3L_TEST_VAR% %S3L_TEST_GONE% & cd & exit 3"]);
            let m = GameManager::new();
            let calls = Arc::new(AtomicUsize::new(0));
            let c2 = calls.clone();
            m.launch(&spec, Arc::new(move |_| { c2.fetch_add(1, Ordering::SeqCst); })).unwrap();
            let s = wait_exit(&m);
            assert_eq!(s.exit_code, Some(3));
            assert!(s.abnormal);
            let tail = s.tail.unwrap();
            assert!(tail.contains("hello"), "{tail}"); // env reached the child
            assert!(!tail.contains("inherited"), "{tail}"); // the inherited variable was removed
            assert!(tail.contains("userdata"), "{tail}"); // working directory
            assert_eq!(calls.load(Ordering::SeqCst), 2); // running + exited
            std::fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn a_launch_sets_the_neutral_and_older_names_and_removes_every_inherited_one() {
            // SAFETY: variables only this test reads, set before the child starts.
            unsafe {
                std::env::set_var("KLM_KALTEST_GONE", "inherited-neutral");
                std::env::set_var("SPLAT3_KALTEST_GONE", "inherited-older");
            }
            let (mut spec, dir) = spec_for("names", system_exe("cmd.exe"), &["/C", "echo [%KLM_KALTEST_SET%] [%SPLAT3_KALTEST_SET%] [%KLM_KALTEST_GONE%] [%SPLAT3_KALTEST_GONE%] & exit 4"]);
            spec.env = vec![("SPLAT3_KALTEST_SET".into(), "v1".into())];
            spec.env_remove = vec!["SPLAT3_KALTEST_GONE".into()];
            let m = GameManager::new();
            m.launch(&spec, Arc::new(|_| {})).unwrap();
            let tail = wait_exit(&m).tail.unwrap();
            assert!(tail.contains("[v1] [v1] [%KLM_KALTEST_GONE%] [%SPLAT3_KALTEST_GONE%]"), "{tail}");
            std::fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn clean_exit_and_double_launch_and_stop() {
            let (spec, dir) = spec_for("dbl", system_exe("ping.exe"), &["-n", "30", "127.0.0.1"]);
            let m = GameManager::new();
            m.launch(&spec, Arc::new(|_| {})).unwrap();
            assert!(matches!(m.launch(&spec, Arc::new(|_| {})), Err(AppError::AlreadyRunning)));
            assert_eq!(m.status().state, RunState::Running);
            m.stop().unwrap();
            let s = wait_exit(&m);
            assert!(s.stopped_by_user);
            assert!(!s.abnormal);
            assert!(matches!(m.stop(), Err(AppError::NotRunning)));
            // A new launch is possible after the exit.
            let (ok, dir2) = spec_for("ok", system_exe("cmd.exe"), &["/C", "exit 0"]);
            m.launch(&ok, Arc::new(|_| {})).unwrap();
            let s = wait_exit(&m);
            assert_eq!(s.exit_code, Some(0));
            assert!(!s.abnormal && s.tail.is_none());
            std::fs::remove_dir_all(dir).unwrap();
            std::fs::remove_dir_all(dir2).unwrap();
        }

        #[test]
        fn a_solo_process_is_closed_for_the_match_and_a_match_process_is_not() {
            // `ping.exe` stands in for the game (it has no window, so only the terminate step can end it).
            let (spec, dir) = spec_for("close", system_exe("ping.exe"), &["-n", "30", "127.0.0.1"]);
            let m = GameManager::new();
            m.launch(&spec, Arc::new(|_| {})).unwrap();
            let pid = m.status().pid.unwrap();
            assert_eq!(m.open_game(), OpenGame::Solo);
            assert_eq!(close_open_game(&m, Duration::from_millis(300), Duration::from_secs(5)).unwrap(), Closed::Forced);
            let s = wait_exit(&m);
            assert!(s.stopped_by_user && !s.abnormal, "closed by the launcher is not a crash: {s:?}");
            assert_eq!(crate::installer::sys::process_alive(pid, "ping.exe"), Some(false));
            assert_eq!(m.open_game(), OpenGame::Nothing);
            // the match can start now
            let (ok, dir2) = spec_for("closed-ok", system_exe("cmd.exe"), &["/C", "exit 0"]);
            m.launch(&ok, Arc::new(|_| {})).unwrap();
            wait_exit(&m);

            // a match in progress: left running, and still refuses a second launch as before
            let (mut spec, dir3) = spec_for("keep", system_exe("ping.exe"), &["-n", "30", "127.0.0.1"]);
            spec.mode = MATCH_MODE.into();
            let m = GameManager::new();
            m.launch(&spec, Arc::new(|_| {})).unwrap();
            let pid = m.status().pid.unwrap();
            assert_eq!(close_open_game(&m, Duration::from_millis(300), Duration::from_millis(300)).unwrap(), Closed::KeptMatch);
            assert_eq!(m.status().state, RunState::Running);
            assert_eq!(crate::installer::sys::process_alive(pid, "ping.exe"), Some(true));
            assert!(matches!(m.launch(&spec, Arc::new(|_| {})), Err(AppError::AlreadyRunning)));
            m.stop().unwrap();
            wait_exit(&m);
            for d in [dir, dir2, dir3] {
                std::fs::remove_dir_all(d).unwrap();
            }
        }

        #[test]
        fn a_process_that_ends_when_asked_is_not_terminated() {
            // The game ends by itself when its window is asked to close: simulated by ending the stand-in at the request.
            struct Obeys(GameManager);
            impl GameCloser for Obeys {
                fn open_game(&self) -> OpenGame {
                    self.0.open_game()
                }
                fn ask_to_close(&self) -> bool {
                    let _ = self.0.lock().child.as_mut().map(|c| c.kill());
                    true
                }
                fn is_open(&self) -> bool {
                    self.0.is_open()
                }
                fn force_close(&self) -> AppResult<()> {
                    panic!("terminated a game that was closing");
                }
            }
            let (spec, dir) = spec_for("obeys", system_exe("ping.exe"), &["-n", "30", "127.0.0.1"]);
            let m = GameManager::new();
            m.launch(&spec, Arc::new(|_| {})).unwrap();
            assert_eq!(close_open_game(&Obeys(m.clone()), Duration::from_secs(5), Duration::from_secs(1)).unwrap(), Closed::Clean);
            assert_eq!(m.status().state, RunState::Exited);
            std::fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn missing_exe_is_an_error_not_a_panic() {
            let (mut spec, dir) = spec_for("noexe", system_exe("cmd.exe"), &[]);
            spec.exe = dir.join("nope.exe");
            assert!(matches!(GameManager::new().launch(&spec, Arc::new(|_| {})), Err(AppError::NotInstalled(_))));
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
