//! The typed IPC surface. Every command returns `Result<T, AppError>`; nothing here panics on missing files.

use crate::config::{self, LANGUAGES, LauncherConfig};
use crate::error::{AppError, AppResult, FieldError};
use crate::install::{self, InstallStatus};
use crate::lan::{self, LanRequest};
use crate::launch::{self, GameManager, GameStatus, Notifier, RunState};
use crate::game::catalogue::{self, Catalogue};
use crate::game::service::{GameService, IconProgress};
use crate::profile::{self, Profile};
use crate::{automatch, gamelang, matchcode, migrate, online, relay, servercard, startmode};
use crate::settings::{self, Change, Schema, SettingsState};
use crate::util;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

pub struct AppState {
    pub data_dir: PathBuf,
    pub config: Mutex<LauncherConfig>,
    pub game: GameManager,
    /// The advanced second local player.
    pub game2: GameManager,
    pub gamedata: GameService,
    pub relay: relay::RelayManager,
    pub installer: crate::installer::Installer,
    /// A game update chosen for "when the launcher closes", until the window is closed.
    pub at_close: Mutex<Option<AtClose>>,
    /// That update runs now, the window gone: the launcher quits once it is done, unless the window was brought back.
    pub closing: std::sync::atomic::AtomicBool,
}

/// What "update when the launcher closes" needs at that moment: the update's request and the words of the icon.
#[derive(Debug, Clone)]
pub struct AtClose {
    pub request: InstallRequest,
    pub texts: crate::tray::TrayTexts,
}

impl AppState {
    pub fn new(data_dir: PathBuf, cfg: LauncherConfig) -> AppState {
        AppState {
            data_dir,
            config: Mutex::new(cfg),
            game: GameManager::new(),
            game2: GameManager::new(),
            gamedata: GameService::default(),
            relay: relay::RelayManager::default(),
            installer: crate::installer::Installer::default(),
            at_close: Mutex::new(None),
            closing: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub(crate) fn cfg(&self) -> LauncherConfig {
        self.config.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub(crate) fn update_config(&self, f: impl FnOnce(&mut LauncherConfig)) -> AppResult<LauncherConfig> {
        let mut g = self.config.lock().unwrap_or_else(|p| p.into_inner());
        let mut next = g.clone();
        f(&mut next);
        config::save(&self.data_dir, &next)?;
        *g = next.clone();
        Ok(next)
    }

    /// The configured game root, else the guessed default.
    fn root(&self) -> AppResult<PathBuf> {
        let cfg = self.cfg();
        cfg.root.map(PathBuf::from).or_else(config::default_root).ok_or(AppError::NoRoot)
    }

    /// The abilities a sub slot can hold, read from the install's keys file (`game::subs`). The install folder is the
    /// wizard's choice, else two levels above the game folder (`<install>\work\<game>`), as `proto::glue` finds it.
    pub(crate) fn sub_capable(&self) -> Option<Vec<String>> {
        let configured = self.cfg().install.dir.map(PathBuf::from);
        let from_root = self.root().ok().and_then(|r| r.parent().and_then(Path::parent).map(Path::to_path_buf));
        configured.into_iter().chain(from_root).find_map(|d| crate::game::subs::read_sub_capable(&installer::Layout::new(d).keys_json()))
    }

    fn translator(&self) -> Option<PathBuf> {
        self.cfg().translator.map(PathBuf::from)
    }

    /// The game folder whose settings file (`build\kalamar.ini`) the Settings page reads and writes: the configured
    /// root when it holds a built game; otherwise the folder an installation started in this session builds into (it
    /// becomes the configured root at that installation's layout step, so what the player sets while it runs is the
    /// game's from its first launch). No step of an installation writes that file.
    pub(crate) fn settings_root(&self) -> AppResult<PathBuf> {
        settings_root_of(self.cfg().root.map(PathBuf::from), self.installer.target_root(), config::default_root())
    }

    /// The game folder whose game data the Locker reads: the configured root when its merged RomFS is there, else the
    /// running installation's as soon as its merge is complete (long before the compile ends).
    fn data_root(&self) -> AppResult<PathBuf> {
        let root = self.root();
        if let Ok(r) = &root
            && crate::game::romfs::locate(r).is_some()
        {
            return root;
        }
        self.installer.game_data_root().map(Ok).unwrap_or(root)
    }
}

/// [`AppState::settings_root`], from the configured root, an installation's target folder and the guessed root (used
/// only without both).
fn settings_root_of(configured: Option<PathBuf>, target: Option<PathBuf>, guessed: Option<PathBuf>) -> AppResult<PathBuf> {
    match (configured, target) {
        (Some(c), _) if install::exe_path(&c).is_file() => Ok(c),
        (_, Some(t)) => Ok(t),
        (Some(c), None) => Ok(c),
        (None, None) => guessed.ok_or(AppError::NoRoot),
    }
}

type St<'a> = State<'a, AppState>;

#[derive(Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub data_dir: String,
    /// How the data folder was found at this start (moved from an earlier launcher's, kept for this session, ...).
    pub data_folder: crate::datadir::Outcome,
}

impl AppInfo {
    fn of(state: &AppState) -> AppInfo {
        AppInfo { name: "Kalamar", version: env!("CARGO_PKG_VERSION"), data_dir: state.data_dir.display().to_string(), data_folder: crate::datadir::current().outcome.clone() }
    }
}

#[derive(Serialize)]
pub struct ConfigView {
    #[serde(flatten)]
    pub config: LauncherConfig,
    /// The root actually used (configured, else the guessed default).
    pub effective_root: Option<String>,
    pub effective_translator: Option<String>,
    /// This build names a Discord application: Settings › Discord exists (`discord::available`).
    pub discord_available: bool,
}

fn config_view(state: &AppState) -> ConfigView {
    let config = state.cfg();
    let root = state.root().ok();
    let tr = root.as_ref().map(|r| install::translator_path(r, state.translator().as_deref()).display().to_string());
    ConfigView { effective_root: root.map(|r| r.display().to_string()), effective_translator: tr, config, discord_available: crate::discord::available() }
}

#[tauri::command]
pub fn app_info(state: St) -> AppInfo {
    AppInfo::of(&state)
}

#[tauri::command]
pub fn get_config(state: St) -> ConfigView {
    config_view(&state)
}

/// Everything the first screen needs, in one round trip and without touching anything big: the configuration, the
/// light install status (a few `stat` calls), the game processes, the saved profile (a small file), the one-time
/// migration notice, the servers (a small file) and the command line. The catalogue, the icons and the RomFS are not
/// part of it: they are read when the Locker is opened or a profile has to be checked for a launch.
#[derive(Serialize)]
pub struct Boot {
    pub info: AppInfo,
    pub config: ConfigView,
    pub status: InstallStatus,
    pub game: GameStatus,
    pub game2: GameStatus,
    pub last_run: Option<GameStatus>,
    /// An installation is running (or was just started): the page opens the wizard.
    pub install_active: bool,
    pub migration: Option<migrate::Migration>,
    pub servers: ServersView,
    pub profile: Profile,
    pub cli: crate::cli::Cli,
}

/// Also embedded in the window at creation (`shell::create_main_window`), so the first screen needs no round trip at
/// all; the command stays for a page that reloads.
pub fn boot_data(state: &AppState) -> Boot {
    crate::trace::timed("boot", || {
        let root = state.root().ok();
        Boot {
            info: AppInfo::of(state),
            config: config_view(state),
            status: install::detect_light(root.as_deref(), state.translator().as_deref()),
            game: state.game.status(),
            game2: state.game2.status(),
            last_run: util::read_text_limited(&last_run_path(&state.data_dir), 1 << 20).ok().flatten().and_then(|t| serde_json::from_str(&t).ok()),
            install_active: state.installer.active(),
            migration: migrate::marker(&state.data_dir).filter(|m| !m.shown),
            servers: servers_view_of(state),
            profile: profile::load_profile(&state.data_dir),
            cli: crate::shell::take_cli(),
        }
    })
}

#[tauri::command(async)]
pub fn boot(state: St) -> Boot {
    boot_data(&state)
}

/// "Back to the game": brings the running game's window to the front. `false` when it has none (yet).
#[tauri::command(async)]
pub fn game_focus(state: St) -> bool {
    crate::winutil::focus_game(state.game.status().pid, &installer::exe_name(installer::GAME_STEM))
}

fn existing_dir(p: &str, what: &str) -> AppResult<PathBuf> {
    let p = p.trim().trim_matches('"');
    if p.is_empty() || p.len() > 1024 || p.contains('\0') {
        return Err(AppError::InvalidInput(format!("{what}: empty or invalid path")));
    }
    let pb = PathBuf::from(p);
    if !pb.is_dir() {
        return Err(AppError::Validation(vec![FieldError { key: what.into(), message: "folder does not exist".into() }]));
    }
    Ok(pb)
}

#[tauri::command]
pub fn set_root(state: St, path: Option<String>) -> AppResult<ConfigView> {
    let root = match path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => Some(existing_dir(p, "root")?.display().to_string()),
        None => None,
    };
    state.update_config(|c| c.root = root)?;
    Ok(config_view(&state))
}

#[tauri::command]
pub fn set_translator(state: St, path: Option<String>) -> AppResult<ConfigView> {
    let tr = match path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            let pb = PathBuf::from(p.trim_matches('"'));
            if !pb.is_file() {
                return Err(AppError::Validation(vec![FieldError { key: "translator".into(), message: "file does not exist".into() }]));
            }
            Some(pb.display().to_string())
        }
        None => None,
    };
    state.update_config(|c| c.translator = tr)?;
    Ok(config_view(&state))
}

#[tauri::command]
pub fn set_language(state: St, language: String) -> AppResult<ConfigView> {
    if !LANGUAGES.contains(&language.as_str()) {
        return Err(AppError::InvalidInput(format!("unknown language '{language}'")));
    }
    state.update_config(|c| c.language = language)?;
    Ok(config_view(&state))
}

/// What "Game language" needs besides the settings file: whether the system fonts of Chinese and Korean are there
/// (without them the game exits before its first image in those languages), and the game language that "like the
/// launcher" gives now.
#[derive(Serialize)]
pub struct GameLanguageInfo {
    pub fonts_ok: bool,
    pub fonts_dir: Option<String>,
    pub like_launcher: String,
}

#[tauri::command]
pub fn game_language_info(state: St) -> GameLanguageInfo {
    let cfg = state.cfg();
    let like_launcher = gamelang::from_launcher(&gamelang::effective_launcher_language(&cfg.language)).to_string();
    match state.settings_root() {
        Ok(root) => {
            let ini = settings::read_ini(&install::ini_path(&root));
            let dir = gamelang::fonts_dir(&root, &ini, |k| std::env::var(k).ok());
            GameLanguageInfo { fonts_ok: gamelang::fonts_present(&dir), fonts_dir: Some(dir.display().to_string()), like_launcher }
        }
        Err(_) => GameLanguageInfo { fonts_ok: false, fonts_dir: None, like_launcher },
    }
}

/// "Like the launcher" or "like Windows": what the game's language follows while the settings file has no `lang`
/// (the page removes or sets `lang` itself through the settings commands).
#[tauri::command]
pub fn set_game_lang_follow(state: St, follow: String) -> AppResult<ConfigView> {
    if follow != gamelang::FOLLOW_LAUNCHER && follow != gamelang::FOLLOW_WINDOWS {
        return Err(AppError::InvalidInput(format!("unknown game language mode '{follow}'")));
    }
    state.update_config(|c| c.game_lang_follow = follow)?;
    Ok(config_view(&state))
}

#[tauri::command]
pub fn set_lan_enabled(state: St, enabled: bool) -> AppResult<ConfigView> {
    state.update_config(|c| c.lan_enabled = enabled)?;
    Ok(config_view(&state))
}

#[tauri::command]
pub fn install_status(state: St) -> InstallStatus {
    crate::trace::timed("install_status", || {
        let root = state.root().ok();
        install::detect(root.as_deref(), state.translator().as_deref())
    })
}

// ---- settings --------------------------------------------------------------------------------------------------

#[tauri::command]
pub fn settings_schema(state: St) -> Schema {
    Schema::load(&state.data_dir)
}

#[tauri::command]
pub fn settings_load(state: St) -> AppResult<SettingsState> {
    let root = state.settings_root()?;
    settings::load(&install::ini_path(&root), &Schema::load(&state.data_dir))
}

/// Saves the game's settings, also while an installation builds that game (its file does not exist yet, or the
/// compile runs beside): nothing of the installation writes it, and the write is atomic.
#[tauri::command]
pub fn settings_save(state: St, changes: Vec<Change>) -> AppResult<SettingsState> {
    let root = state.settings_root()?;
    // Chinese and Korean are refused without the extracted system fonts (the page greys them out; this is the same rule).
    if let Some(c) = changes.iter().find(|c| c.key.eq_ignore_ascii_case("lang")) {
        let ini = settings::read_ini(&install::ini_path(&root));
        let fonts_ok = gamelang::fonts_present(&gamelang::fonts_dir(&root, &ini, |k| std::env::var(k).ok()));
        gamelang::check_lang_change(c.value.as_deref(), fonts_ok).map_err(|message| AppError::Validation(vec![FieldError { key: "lang".into(), message }]))?;
    }
    settings::save(&install::ini_path(&root), &Schema::load(&state.data_dir), &changes)
}

// ---- launching -------------------------------------------------------------------------------------------------

/// The two instances the launcher can run: the player's own, and the advanced "second local player" (another user
/// data folder, profile name and nickname, for tests on one PC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    First,
    Second,
}

impl Slot {
    fn event(self) -> &'static str {
        match self {
            Slot::First => "game-status",
            Slot::Second => "game-status-p2",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Slot::First => "first",
            Slot::Second => "second",
        }
    }
}

fn last_run_path(data_dir: &Path) -> PathBuf {
    data_dir.join("last_run.json")
}

fn last_launch_path(data_dir: &Path, slot: Slot) -> PathBuf {
    data_dir.join(if slot == Slot::First { "last_launch.json" } else { "last_launch_p2.json" })
}

/// "Minimize while the game runs" applies to the first player's offline and LAN launches, not to a private match
/// (that flow brings the launcher back by itself) and not to the second local player.
fn follows_game(ui: &config::UiPrefs, slot: Slot, mode: &str) -> bool {
    ui.minimize_on_play && slot == Slot::First && mode != "match"
}

/// The launcher returns when the game it minimized for has ended. A game that restarted itself (a first launch) is
/// still running as another process, so the launcher stays out of the way until that one is stopped from it.
fn comes_back(ui: &config::UiPrefs, status: &GameStatus) -> bool {
    ui.minimize_on_play && status.state == RunState::Exited && !status.restarted
}

fn notifier(app: AppHandle, data_dir: PathBuf, slot: Slot) -> Notifier {
    std::sync::Arc::new(move |status: &GameStatus| {
        if slot == Slot::First
            && status.state == RunState::Exited
            && let Ok(text) = serde_json::to_string_pretty(status)
        {
            let _ = util::atomic_write(&last_run_path(&data_dir), text.as_bytes());
        }
        let _ = app.emit(slot.event(), status);
        // The game window was hidden for most of a match: bring the launcher back for the result screen.
        if slot == Slot::First && status.state == RunState::Exited && status.mode.as_deref() == Some("match") {
            crate::shell::bring_back(&app);
        } else if slot == Slot::First && status.state == RunState::Exited {
            use tauri::Manager;
            let ui = app.state::<AppState>().cfg().ui;
            if comes_back(&ui, status) {
                crate::shell::bring_back(&app);
            }
        }
    })
}

/// What the launcher did at the last launch of an instance, saved so the page can report on it afterwards.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LaunchRecord {
    pub started_at: u64,
    /// The card: `hub`, `range`, `lan` or `original`.
    pub mode: String,
    /// `first` or `second` (the second local player).
    pub slot: String,
    /// `passed` (TOTK_SAVE_PROFILE was set), `disabled` (switched off), `skipped` (the user launched without it),
    /// `incomplete` (a new save needs the whole appearance), `invalid` (the profile fails validation).
    pub profile: String,
    pub missing: Vec<String>,
    pub detail: Option<String>,
    pub skip_first_launch: bool,
    pub unlock_all: bool,
    /// A save existed in the user data folder when the game was launched (partial profiles are accepted then).
    pub save_existed: bool,
    pub log_path: String,
    /// `logs\totk.log` next to the executable: where a game that restarted itself writes.
    pub restart_log_path: String,
    pub userdata: String,
    pub profile_dir: String,
    /// `SPLAT3_START`, `SPLAT3_LAN`, the room and the nickname, for the in-app help.
    pub scene: Option<String>,
    pub lan: Option<String>,
    pub room: Option<String>,
    pub nickname: Option<String>,
    /// The environment the launcher added, for the report (names and values; nothing secret is ever put there).
    pub env: Vec<(String, String)>,
}

fn profile_options(cfg: &LauncherConfig) -> profile::ApplyOptions {
    profile::ApplyOptions { skip_first_launch: cfg.skip_first_launch, unlock_all: cfg.unlock_all, unlock_chunks: cfg.unlock_chunks }
}

/// The profile part of a launch.
struct ProfileStep {
    /// Environment to add (`TOTK_SAVE_PROFILE`, nickname).
    env: Vec<(String, String)>,
    /// `passed`, `disabled`, `incomplete` or `invalid`.
    state: String,
    missing: Vec<String>,
    detail: Option<String>,
    save_exists: bool,
}

/// Folder name of the second local player: a plain name, never the first player's `userdata` (the first player's save).
pub fn valid_second_dir(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 40
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !ok {
        return Err("folder name: 1 to 40 letters, digits, . _ or -".into());
    }
    if name.eq_ignore_ascii_case("userdata") {
        return Err("userdata is the first player's folder: use another name".into());
    }
    Ok(())
}

fn second_userdata(root: &Path, cfg: &LauncherConfig) -> AppResult<PathBuf> {
    valid_second_dir(&cfg.second_dir).map_err(|m| AppError::Validation(vec![FieldError { key: "second_dir".into(), message: m }]))?;
    Ok(root.join(&cfg.second_dir))
}

fn profile_step(state: &AppState, userdata: &Path, name: Option<&str>) -> ProfileStep {
    let cfg = state.cfg();
    let save_exists = profile::save_exists(&userdata.join("profile"));
    let step = |env, state: &str, missing: Vec<&str>, detail| ProfileStep { env, state: state.into(), missing: missing.iter().map(|s| s.to_string()).collect(), detail, save_exists };
    if !cfg.apply_profile {
        return step(vec![], "disabled", vec![], None);
    }
    let mut p = profile::load_profile(&state.data_dir);
    if let Some(n) = name {
        p.name = n.to_string();
    }
    // A new save needs the whole appearance; with a save every field is optional (absent = unchanged).
    let missing = if save_exists { vec![] } else { profile::missing_appearance(&p) };
    if !missing.is_empty() {
        return step(vec![], "incomplete", missing, None);
    }
    let clean = match validated(state, &p) {
        Ok(c) => c,
        Err(e) => return step(vec![], "invalid", vec![], Some(profile_error_text(&e))),
    };
    match profile::prepare_launch(&clean, profile_options(&cfg), userdata, save_exists) {
        Ok(env) => step(env, "passed", vec![], None),
        Err(e) => step(vec![], "invalid", vec![], Some(e.to_string())),
    }
}

fn profile_error_text(e: &AppError) -> String {
    match e {
        AppError::Validation(v) => v.iter().map(|f| format!("{}: {}", f.key, f.message)).collect::<Vec<_>>().join("; "),
        e => e.to_string(),
    }
}

fn lan_request_from_prefs(p: &config::LanPrefs) -> LanRequest {
    LanRequest {
        mode: p.mode,
        relay_host: p.relay_host.clone(),
        relay_port: p.relay_port,
        room: p.room.clone(),
        net_log: p.net_log,
        bind_any: p.bind_any,
        lan_address: p.lan_address.clone(),
    }
}

/// The second local player shares the PC with the first: in lan mode it takes the loopback address 127.0.0.3 (the
/// documented pair is 127.0.0.2 for the first and 127.0.0.3 for the second); in relay mode the addresses come from the relay.
pub const SECOND_LAN_ADDRESS: &str = "127.0.0.3/8";

/// The environment of a card: `SPLAT3_START` and, for the LAN card, the LAN variables of the saved LAN settings.
fn mode_env(state: &AppState, mode: &str, slot: Slot) -> AppResult<Vec<(String, String)>> {
    let table = startmode::StartTable::load(&state.data_dir);
    let cfg = state.cfg();
    match mode {
        "hub" | "range" | "original" | "lan" => {
            let start = table.env(mode).ok_or_else(|| AppError::FeatureDisabled(mode.into()))?;
            let mut env = vec![start];
            if mode == "lan" {
                if !cfg.lan_enabled {
                    return Err(AppError::FeatureDisabled("lan".into()));
                }
                let mut req = lan_request_from_prefs(&cfg.lan);
                if slot == Slot::Second && req.mode == lan::LanMode::Lan {
                    req.lan_address = SECOND_LAN_ADDRESS.into();
                }
                let name = if slot == Slot::Second { cfg.second_name.clone() } else { profile::load_profile(&state.data_dir).name };
                env.extend(lan::build_lan_env(&req, &name)?);
            }
            Ok(env)
        }
        other => Err(AppError::InvalidInput(format!("unknown mode '{other}'"))),
    }
}

/// Copies what the second instance should not rebuild (translated shaders, pipeline cache) from the first player's
/// user data folder, once: only files the second folder lacks, so a second launch never overwrites its own caches.
fn seed_caches(from: &Path, to: &Path) -> AppResult<()> {
    fn copy_missing(from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            let (src, dst) = (e.path(), to.join(e.file_name()));
            if e.file_type()?.is_dir() {
                copy_missing(&src, &dst)?;
            } else if !dst.exists() {
                std::fs::copy(&src, &dst)?;
            }
        }
        Ok(())
    }
    std::fs::create_dir_all(to.join("logs")).map_err(|e| AppError::io(to, &e))?;
    std::fs::create_dir_all(to.join("profile")).map_err(|e| AppError::io(to, &e))?;
    for file in ["pipeline-cache.bin", "pipeline-keys.bin", "shader-answers.bin", "shader-programs.bin"] {
        let (src, dst) = (from.join(file), to.join(file));
        if src.is_file() && !dst.exists() {
            std::fs::copy(&src, &dst).map_err(|e| AppError::io(&dst, &e))?;
        }
    }
    if from.join("shader-cache").is_dir() {
        copy_missing(&from.join("shader-cache"), &to.join("shader-cache")).map_err(|e| AppError::io(&to.join("shader-cache"), &e))?;
    }
    Ok(())
}

/// "Leave my save alone": no profile at all reaches the game (no `TOTK_SAVE_PROFILE`, no nickname variable), which then
/// runs purely on its own save; on a first launch it plays its own intro and character creation.
fn untouched_step(save_exists: bool) -> ProfileStep {
    ProfileStep { env: vec![], state: "skipped".into(), missing: vec![], detail: None, save_exists }
}

#[allow(clippy::too_many_arguments)]
fn launch_with(state: &AppState, app: AppHandle, slot: Slot, mode: &str, ignore_profile: bool, untouched_save: bool, extra_args: &[String], card_env: Option<Vec<(String, String)>>) -> AppResult<GameStatus> {
    let root = state.root()?;
    let translator = state.translator();
    let missing = launch::missing_for_launch(&root, translator.as_deref());
    if !missing.is_empty() {
        return Err(AppError::NotInstalled(missing));
    }
    // An update may run: its game stays playable, except for the few seconds the new executable is put in place.
    if state.installer.replacing(&root) {
        return Err(AppError::GameUpdating);
    }
    let cfg = state.cfg();
    let first_ud = install::userdata_dir(&root);
    let (userdata, nick, manager) = match slot {
        Slot::First => (first_ud.clone(), None, &state.game),
        Slot::Second => (second_userdata(&root, &cfg)?, Some(cfg.second_name.clone()), &state.game2),
    };
    let mode_env = match card_env {
        Some(e) => e,
        None => mode_env(state, mode, slot)?,
    };
    let mut step = if untouched_save { untouched_step(profile::save_exists(&userdata.join("profile"))) } else { profile_step(state, &userdata, nick.as_deref()) };
    if !ignore_profile {
        match step.state.as_str() {
            "incomplete" => return Err(AppError::ProfileIncomplete(step.missing)),
            "invalid" => return Err(AppError::ProfileInvalid(step.detail.unwrap_or_default())),
            _ => {}
        }
    } else if step.state == "incomplete" || step.state == "invalid" {
        step.state = "skipped".into(); // the user chose to launch without the profile: the game makes its own character
    }
    if slot == Slot::Second {
        seed_caches(&first_ud, &userdata)?;
    }
    let mut env = step.env.clone();
    env.extend(mode_env);
    // The game's language: "like the launcher" / "like Windows" while the settings file has no `lang` of its own.
    let ini_lang = settings::read_value(&install::ini_path(&root), "lang");
    let lang_env = gamelang::launch_env(&cfg.game_lang_follow, &gamelang::effective_launcher_language(&cfg.language), ini_lang.as_deref());
    if let gamelang::LangEnv::Set(code) = &lang_env {
        env.push((gamelang::ENV_LANG.to_string(), (*code).to_string()));
    }
    if slot == Slot::Second {
        env.push(("SPLAT3_WINDOW_TITLE".into(), format!("Kalamar — {}", cfg.second_name)));
        if step.state != "passed" {
            env.push(("SPLAT3_NICKNAME".into(), cfg.second_name.clone()));
        }
    }
    // The game started alone later follows the same Discord choice (a game installed after it was made has no copy yet).
    crate::discord::mirror_to_game(state);
    let mut spec = launch::build_launch_spec(&root, &userdata, translator.as_deref(), extra_args, &env, &launch::log_stamp(), mode);
    if lang_env != gamelang::LangEnv::Leave {
        for n in gamelang::ENV_LANG_NAMES {
            if !spec.env_remove.iter().any(|r| r == n) && !env.iter().any(|(k, _)| k == n) {
                spec.env_remove.push(n.to_string());
            }
        }
    }
    for n in automatch::MatchVars::load(&state.data_dir).names() {
        if !spec.env_remove.contains(&n) && !env.iter().any(|(k, _)| *k == n) {
            spec.env_remove.push(n);
        }
    }
    let exe_dir = spec.exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let var = |name: &str| env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let record = LaunchRecord {
        started_at: util::now_secs(),
        mode: mode.to_string(),
        slot: slot.name().into(),
        profile: step.state.clone(),
        missing: step.missing.clone(),
        detail: step.detail.clone(),
        skip_first_launch: cfg.skip_first_launch,
        unlock_all: cfg.unlock_all,
        save_existed: step.save_exists,
        log_path: spec.stderr_log.display().to_string(),
        restart_log_path: exe_dir.join("logs").join("totk.log").display().to_string(),
        userdata: userdata.display().to_string(),
        profile_dir: userdata.join("profile").display().to_string(),
        scene: var("SPLAT3_START"),
        lan: var("SPLAT3_LAN"),
        room: var("SPLAT3_LAN_ROOM"),
        nickname: var("SPLAT3_NICKNAME"),
        // as the game reads them: the documented names
        env: launch::shown_env(&env),
    };
    let status = manager.launch(&spec, notifier(app.clone(), state.data_dir.clone(), slot))?;
    if follows_game(&cfg.ui, slot, mode) {
        crate::shell::minimize(&app);
    }
    if let Ok(text) = serde_json::to_string_pretty(&record) {
        let _ = util::atomic_write(&last_launch_path(&state.data_dir, slot), text.as_bytes());
    }
    Ok(status)
}

#[derive(Deserialize)]
pub struct PlayRequest {
    /// `hub`, `range`, `lan` or `original`.
    pub mode: String,
    /// Launch without the profile although it is incomplete or invalid (the game then makes its own character).
    #[serde(default)]
    pub ignore_profile: bool,
    /// Launch without any profile: the game runs on its own save (the "full game" tile's second choice).
    #[serde(default)]
    pub untouched_save: bool,
    #[serde(default)]
    pub extra_args: Option<String>,
}

#[tauri::command(async)]
pub fn launch_mode(app: AppHandle, state: St, request: PlayRequest) -> AppResult<GameStatus> {
    let args = launch::parse_extra_args(request.extra_args.as_deref().unwrap_or(""))?;
    launch_with(&state, app, Slot::First, &request.mode, request.ignore_profile, request.untouched_save, &args, None)
}

/// Advanced: the second local player. Another instance with its own user data folder, nickname and profile name.
#[tauri::command(async)]
pub fn launch_second(app: AppHandle, state: St, request: PlayRequest) -> AppResult<GameStatus> {
    let args = launch::parse_extra_args(request.extra_args.as_deref().unwrap_or(""))?;
    launch_with(&state, app, Slot::Second, &request.mode, request.ignore_profile, false, &args, None)
}

#[tauri::command]
pub fn game_status(state: St) -> GameStatus {
    state.game.status()
}

#[tauri::command]
pub fn game2_status(state: St) -> GameStatus {
    state.game2.status()
}

#[tauri::command]
pub fn last_run(state: St) -> Option<GameStatus> {
    util::read_text_limited(&last_run_path(&state.data_dir), 1 << 20).ok().flatten().and_then(|t| serde_json::from_str(&t).ok())
}

#[tauri::command]
pub fn game_stop(state: St) -> AppResult<()> {
    state.game.stop()
}

#[tauri::command]
pub fn game2_stop(state: St) -> AppResult<()> {
    state.game2.stop()
}

#[tauri::command]
pub fn open_logs_folder(state: St) -> AppResult<String> {
    let root = state.root()?;
    let dir = install::logs_dir(&root);
    std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, &e))?;
    #[cfg(windows)]
    {
        std::process::Command::new(crate::winutil::windows_exe("explorer.exe")).arg(&dir).spawn().map_err(|e| AppError::io(&dir, &e))?;
        Ok(dir.display().to_string())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(&dir).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().map_err(|e| AppError::io(&dir, &e))?;
        Ok(dir.display().to_string())
    }
}

#[tauri::command]
pub fn read_log_tail(state: St, lines: Option<usize>) -> AppResult<Option<String>> {
    let root = state.root()?;
    let n = lines.unwrap_or(60).clamp(1, 500);
    match install::latest_log_path(&root) {
        Some(p) => util::tail_lines(&p, n),
        None => Ok(None),
    }
}

/// The first `max` bytes of a log (the runtime prints its `save profile:` lines at start-up).
fn read_head(path: &Path, max: u64) -> String {
    use std::io::Read;
    let Ok(f) = std::fs::File::open(path) else { return String::new() };
    let mut buf = Vec::new();
    let _ = f.take(max).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

#[derive(Serialize)]
pub struct LaunchReport {
    pub record: Option<LaunchRecord>,
    pub feedback: profile::Feedback,
    /// Newest backup the runtime made of the save, when it said so.
    pub backup_exists: bool,
    /// The backups folder of the instance's profile directory, counted now.
    pub backups: Option<profile::BackupInfo>,
}

/// What the runtime did with the profile at the last launch, read from its log (and from the log of the process it
/// restarted into, on a first launch).
#[tauri::command(async)]
pub fn launch_report(state: St, second: Option<bool>) -> LaunchReport {
    let slot = if second == Some(true) { Slot::Second } else { Slot::First };
    let record: Option<LaunchRecord> = util::read_text_limited(&last_launch_path(&state.data_dir, slot), 1 << 20).ok().flatten().and_then(|t| serde_json::from_str(&t).ok());
    let Some(r) = record else {
        return LaunchReport { record: None, feedback: profile::Feedback { state: "none".into(), ..Default::default() }, backup_exists: false, backups: None };
    };
    let mut text = read_head(Path::new(&r.log_path), 4 << 20);
    if text.contains(launch::RESTART_LINE) {
        let second = Path::new(&r.restart_log_path);
        let fresh = util::mtime_secs(second).is_some_and(|m| m + 2 >= r.started_at);
        if fresh {
            text.push('\n');
            text.push_str(&read_head(second, 4 << 20));
        }
    }
    let feedback = profile::parse_log(&text);
    let backup_exists = feedback.backup.as_deref().is_some_and(|b| Path::new(b).exists());
    let backups = Some(profile::backup_info(Path::new(&r.profile_dir)));
    LaunchReport { record: Some(r), feedback, backup_exists, backups }
}

#[derive(Serialize)]
pub struct ModeInfo {
    pub id: String,
    pub enabled: bool,
    pub var: String,
    pub value: String,
}

#[derive(Serialize)]
pub struct LanInfo {
    pub enabled: bool,
    /// The saved LAN settings give a valid environment.
    pub ready: bool,
    pub problem: Option<String>,
    pub mode: lan::LanMode,
    /// The `SPLAT3_LAN` value a launch would set.
    pub value: Option<String>,
    pub room: Option<String>,
}

#[derive(Serialize)]
pub struct SecondInfo {
    pub name: String,
    pub dir: String,
    pub path: Option<String>,
    pub dir_ok: bool,
    pub exists: bool,
    pub save_exists: bool,
    pub lan_address: &'static str,
}

#[derive(Serialize)]
pub struct Preflight {
    /// The profile was checked against the game's catalogue (false for the first screen's light version).
    pub deep: bool,
    /// A save exists in the user data folder: partial profiles are accepted, a new save needs the whole appearance.
    pub save_exists: bool,
    pub userdata: Option<String>,
    pub apply_profile: bool,
    /// `passed`, `disabled`, `incomplete` or `invalid`: what the next launch would do with the profile.
    pub profile: String,
    pub missing: Vec<String>,
    pub detail: Option<String>,
    pub name: String,
    pub name_ok: bool,
    pub lan: LanInfo,
    pub modes: Vec<ModeInfo>,
    pub backups: profile::BackupInfo,
    pub second: SecondInfo,
}

/// Everything the Play screen needs to say what each card will do, before anything is launched. Writes no file.
#[tauri::command(async)]
pub fn play_preflight(state: St, light: Option<bool>) -> Preflight {
    crate::trace::timed("play_preflight", || play_preflight_inner(&state, light.unwrap_or(false)))
}

/// `light`: the first screen's version. The profile is not checked against the game's catalogue unless that is
/// already in memory (loading it reads the 2.5 MB RomFS index and the cached 2 MB catalogue); `deep` in the result
/// tells which one it was. A launch always checks the profile completely, so nothing invalid ever reaches the game.
fn play_preflight_inner(state: &AppState, light: bool) -> Preflight {
    let deep = !light || state.gamedata.current().is_some();
    let cfg = state.cfg();
    let root = state.root().ok();
    let userdata = root.as_ref().map(|r| install::userdata_dir(r));
    let profile_dir = userdata.as_ref().map(|u| u.join("profile"));
    let save_exists = profile_dir.as_ref().is_some_and(|d| profile::save_exists(d));
    let p = profile::load_profile(&state.data_dir);
    let name_ok = profile::validate_name(&p.name).is_ok();
    let (pstate, missing, detail) = if !cfg.apply_profile {
        ("disabled".to_string(), vec![], None)
    } else if !save_exists && !profile::missing_appearance(&p).is_empty() {
        ("incomplete".to_string(), profile::missing_appearance(&p).iter().map(|s| s.to_string()).collect(), None)
    } else {
        // Validate exactly as a launch does, but write nothing.
        if !deep {
            ("passed".to_string(), vec![], None)
        } else {
            let outcome = validated(state, &p).map_err(|e| profile_error_text(&e)).and_then(|c| profile::to_runtime(&c, profile_options(&cfg), save_exists).map_err(|e| e.to_string()));
            match outcome {
                Ok(_) => ("passed".to_string(), vec![], None),
                Err(d) => ("invalid".to_string(), vec![], Some(d)),
            }
        }
    };
    let req = lan_request_from_prefs(&cfg.lan);
    let (ready, problem, value, room) = match lan::build_lan_env(&req, &p.name) {
        Ok(env) => {
            let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
            (true, None, get(lan::ENV_LAN), get(lan::ENV_ROOM))
        }
        Err(AppError::Validation(v)) => (false, Some(v.iter().map(|f| f.message.clone()).collect::<Vec<_>>().join("; ")), None, None),
        Err(e) => (false, Some(e.to_string()), None, None),
    };
    let table = startmode::StartTable::load(&state.data_dir);
    let second_ud = root.as_ref().and_then(|r| second_userdata(r, &cfg).ok());
    Preflight {
        deep,
        save_exists,
        userdata: userdata.map(|u| u.display().to_string()),
        apply_profile: cfg.apply_profile,
        profile: pstate,
        missing,
        detail,
        name: p.name.clone(),
        name_ok,
        lan: LanInfo { enabled: cfg.lan_enabled, ready, problem, mode: cfg.lan.mode, value, room },
        modes: table.modes.iter().map(|m| ModeInfo { id: m.id.clone(), enabled: m.enabled, var: launch::env_name(&m.var), value: m.value.clone() }).collect(),
        backups: profile_dir.as_ref().map(|d| profile::backup_info(d)).unwrap_or_default(),
        second: SecondInfo {
            name: cfg.second_name.clone(),
            dir: cfg.second_dir.clone(),
            dir_ok: valid_second_dir(&cfg.second_dir).is_ok(),
            exists: second_ud.as_ref().is_some_and(|d| d.is_dir()),
            save_exists: second_ud.as_ref().is_some_and(|d| profile::save_exists(&d.join("profile"))),
            path: second_ud.map(|d| d.display().to_string()),
            lan_address: SECOND_LAN_ADDRESS,
        },
    }
}

/// The backups the runtime keeps of the save (folder, count, newest).
#[tauri::command]
pub fn backups_info(state: St) -> profile::BackupInfo {
    state.root().map(|r| profile::backup_info(&install::userdata_dir(&r).join("profile"))).unwrap_or_default()
}

#[derive(Deserialize)]
pub struct LaunchOptions {
    pub apply_profile: bool,
    pub skip_first_launch: bool,
    pub unlock_all: bool,
    #[serde(default)]
    pub unlock_chunks: bool,
}

#[tauri::command]
pub fn set_launch_options(state: St, options: LaunchOptions) -> AppResult<ConfigView> {
    state.update_config(|c| {
        c.apply_profile = options.apply_profile;
        c.skip_first_launch = options.skip_first_launch;
        c.unlock_all = options.unlock_all;
        c.unlock_chunks = options.unlock_chunks;
    })?;
    Ok(config_view(&state))
}

/// Nickname and user data folder of the second local player.
#[tauri::command]
pub fn set_second_player(state: St, name: String, dir: String) -> AppResult<ConfigView> {
    let mut errors = Vec::new();
    let name = match profile::validate_name(&name) {
        Ok(n) => n,
        Err(message) => {
            errors.push(FieldError { key: "second_name".into(), message });
            String::new()
        }
    };
    let dir = dir.trim().to_string();
    if let Err(message) = valid_second_dir(&dir) {
        errors.push(FieldError { key: "second_dir".into(), message });
    }
    if !errors.is_empty() {
        return Err(AppError::Validation(errors));
    }
    state.update_config(|c| {
        c.second_name = name;
        c.second_dir = dir;
    })?;
    Ok(config_view(&state))
}

// ---- LAN -------------------------------------------------------------------------------------------------------

#[tauri::command]
pub fn lan_generate_room() -> String {
    lan::generate_room_code()
}

fn lan_env(state: &AppState, req: &LanRequest) -> AppResult<Vec<(String, String)>> {
    let name = profile::load_profile(&state.data_dir).name;
    lan::build_lan_env(req, &name)
}

/// The variables a LAN launch would set, so the page can show them. Works even when the feature is off.
#[tauri::command]
pub fn lan_preview(state: St, request: LanRequest) -> AppResult<Vec<(String, String)>> {
    lan_env(&state, &request).map(|env| launch::shown_env(&env))
}


/// Remembers the LAN settings (the LAN card of the Play screen launches with them). Nothing is launched.
#[tauri::command]
pub fn set_lan_prefs(state: St, request: LanRequest) -> AppResult<ConfigView> {
    let prefs = config::LanPrefs {
        mode: request.mode,
        relay_host: request.relay_host.trim().to_string(),
        relay_port: request.relay_port,
        room: request.room.trim().to_string(),
        net_log: request.net_log,
        bind_any: request.bind_any,
        lan_address: request.lan_address.trim().to_string(),
    };
    state.update_config(|c| c.lan = prefs)?;
    Ok(config_view(&state))
}

// ---- optional local relay ----------------------------------------------------------------------------------------

fn relay_log(state: &AppState) -> PathBuf {
    state.data_dir.join("relay.log")
}

fn relay_exe(state: &AppState) -> Option<PathBuf> {
    let cfg = state.cfg();
    let mut starts: Vec<PathBuf> = state.root().ok().into_iter().collect();
    starts.extend(std::env::current_exe().ok());
    relay::locate(cfg.relay_exe.as_deref().map(Path::new), &starts)
}

#[tauri::command]
pub fn relay_status(state: St) -> relay::RelayStatus {
    state.relay.status(relay_exe(&state).as_deref(), &relay_log(&state))
}

#[tauri::command]
pub fn relay_start(state: St, port: u16) -> AppResult<relay::RelayStatus> {
    let exe = relay_exe(&state).ok_or_else(|| AppError::NotAvailable("splat3-relay.exe was not found: set its path".into()))?;
    state.relay.start(&exe, port, &relay_log(&state))?;
    state.update_config(|c| c.relay_port = port)?;
    Ok(state.relay.status(Some(&exe), &relay_log(&state)))
}

#[tauri::command]
pub fn relay_stop(state: St) -> AppResult<relay::RelayStatus> {
    state.relay.stop()?;
    Ok(state.relay.status(relay_exe(&state).as_deref(), &relay_log(&state)))
}

#[tauri::command]
pub fn set_relay_exe(state: St, path: Option<String>) -> AppResult<ConfigView> {
    let p = match path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            let pb = PathBuf::from(p.trim_matches('"'));
            if !pb.is_file() {
                return Err(AppError::Validation(vec![FieldError { key: "relay_exe".into(), message: "file does not exist".into() }]));
            }
            Some(pb.display().to_string())
        }
        None => None,
    };
    state.update_config(|c| c.relay_exe = p)?;
    Ok(config_view(&state))
}

// ---- game data (catalogue + icons) ------------------------------------------------------------------------------

#[derive(Serialize)]
pub struct CatalogueView {
    pub catalogue: std::sync::Arc<Catalogue>,
    /// `game` when built from the player's RomFS, `neutral` for the built-in fallback.
    pub origin: &'static str,
    /// Why the neutral catalogue is used (no RomFS found, unreadable image...).
    pub reason: Option<String>,
    pub from_cache: bool,
    pub image: Option<String>,
    pub name_max_chars: usize,
}

/// The catalogue for the configured root: loaded from the cache or built from the RomFS, else the neutral one.
fn catalogue_view(state: &AppState, force: bool) -> CatalogueView {
    let neutral = |reason: Option<String>| CatalogueView {
        catalogue: std::sync::Arc::new(catalogue::neutral()),
        origin: "neutral",
        reason,
        from_cache: false,
        image: None,
        name_max_chars: profile::NAME_MAX_CHARS,
    };
    if !force && let Some(l) = state.gamedata.current() {
        return CatalogueView { catalogue: l.catalogue.clone(), origin: "game", reason: None, from_cache: l.from_cache, image: Some(l.rom.image_path.display().to_string()), name_max_chars: profile::NAME_MAX_CHARS };
    }
    // During a first installation the game data is read from the folder it builds into once its merge is complete.
    let root = match state.data_root() {
        Ok(r) => r,
        Err(_) => return neutral(Some("no game folder is configured".into())),
    };
    if crate::game::romfs::locate(&root).is_none() {
        return neutral(Some(format!("no romfs.bin and index.json found under {}", root.display())));
    }
    match state.gamedata.load(&root, &state.data_dir, force) {
        Ok(l) => CatalogueView { catalogue: l.catalogue.clone(), origin: "game", reason: None, from_cache: l.from_cache, image: Some(l.rom.image_path.display().to_string()), name_max_chars: profile::NAME_MAX_CHARS },
        Err(e) => neutral(Some(e)),
    }
}

/// Loads (or with `force` rebuilds) the catalogue from the player's game data.
#[tauri::command(async)]
pub fn profile_catalogue(state: St, force: Option<bool>) -> CatalogueView {
    crate::trace::timed("profile_catalogue", || catalogue_view(&state, force.unwrap_or(false)))
}

#[derive(Serialize)]
pub struct StagesView {
    pub stages: Vec<catalogue::Stage>,
    /// `game` when read from the player's RomFS, `neutral` without game data (then no stages).
    pub origin: &'static str,
    pub reason: Option<String>,
    /// The widths a stage picture can be asked at through the `gameicon` protocol (`<image>@<width>`).
    pub sizes: Vec<u32>,
}

/// The versus stages of the player's game data (names in every language, picture keys) for the stage cards. Loads or
/// builds the catalogue as the Locker does; nothing of it is in the launcher.
#[tauri::command(async)]
pub fn gamedata_stages(state: St) -> StagesView {
    let v = crate::trace::timed("gamedata_stages", || catalogue_view(&state, false));
    StagesView { stages: v.catalogue.stages.clone(), origin: v.origin, reason: v.reason, sizes: crate::game::icons::SIZES.to_vec() }
}

#[derive(Serialize)]
pub struct IconCacheInfo {
    pub files: usize,
    pub bytes: u64,
    pub warming: bool,
    pub dir: Option<String>,
}

#[tauri::command]
pub fn gamedata_icon_cache(state: St) -> IconCacheInfo {
    let warming = state.gamedata.warming.load(std::sync::atomic::Ordering::SeqCst);
    match state.gamedata.current() {
        Some(l) => {
            let (files, bytes) = l.store.stats();
            IconCacheInfo { files, bytes, warming, dir: Some(l.store.dir().display().to_string()) }
        }
        None => IconCacheInfo { files: 0, bytes: 0, warming, dir: None },
    }
}

/// Decodes every icon of the catalogue in the background; progress arrives as `gamedata-icons` events.
#[tauri::command]
pub fn gamedata_icons_warm(app: AppHandle, state: St) -> AppResult<bool> {
    if state.gamedata.current().is_none() {
        return Err(AppError::NotAvailable("no game data is loaded".into()));
    }
    Ok(state.gamedata.warm_icons(move |p: IconProgress| {
        let _ = app.emit("gamedata-icons", p);
    }))
}

#[tauri::command]
pub fn gamedata_icons_cancel(state: St) {
    state.gamedata.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Deletes the derived data (catalogue and icons) from the app data directory; it is rebuilt on demand.
#[tauri::command]
pub fn gamedata_cache_clear(state: St) -> AppResult<()> {
    state.gamedata.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    *state.gamedata.loaded.lock().unwrap_or_else(|p| p.into_inner()) = None;
    let dir = crate::game::service::cache_root(&state.data_dir);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AppError::io(&dir, &e)),
    }
}

// ---- profile ---------------------------------------------------------------------------------------------------

#[tauri::command]
pub fn profile_load(state: St) -> Profile {
    crate::trace::timed("profile_load", || profile::load_profile(&state.data_dir))
}

fn validated(state: &AppState, p: &Profile) -> AppResult<Profile> {
    let view = catalogue_view(state, false);
    let subs = state.sub_capable();
    profile::validate_profile(p, &view.catalogue, subs.as_deref())
}

/// What the Locker's sub-ability editor needs to know (`gear_abilities_info`).
#[derive(Serialize)]
pub struct GearAbilitiesInfo {
    /// The editor is shown: the installed engine declares `gear-abilities` and the ability list is known.
    pub supported: bool,
    /// The game is installed but its engine does not declare `gear-abilities` (or no ability list can be read): the
    /// Locker asks for a game update.
    pub update_needed: bool,
    /// The abilities a sub slot can hold, in the game's enum order (from the install's keys file, `game::subs`).
    pub abilities: Vec<String>,
}

/// Whether the installed game applies sub abilities, and which abilities a sub slot can hold. The engine's capability
/// list is read from its cached listing (it runs once per executable, with no window).
#[tauri::command(async)]
pub fn gear_abilities_info(state: St) -> GearAbilitiesInfo {
    let abilities = state.sub_capable().unwrap_or_default();
    let engine_ok = match state.root() {
        Ok(root) if install::exe_path(&root).is_file() => {
            let cwd = install::userdata_dir(&root);
            let _ = std::fs::create_dir_all(&cwd);
            let lang = crate::proto::engine::list_lang(&state.cfg().language);
            let info = crate::proto::engine::engine_info_in(&state.data_dir, &install::exe_path(&root), &cwd, Some(lang));
            Some(crate::proto::engine::caps(Some(&info)).gear_abilities)
        }
        _ => None,
    };
    gear_abilities_status(engine_ok, abilities)
}

/// The decision of [`gear_abilities_info`]: `engine_ok` is `None` without an installed game, else whether its engine
/// declares the capability.
fn gear_abilities_status(engine_ok: Option<bool>, abilities: Vec<String>) -> GearAbilitiesInfo {
    let supported = engine_ok == Some(true) && !abilities.is_empty();
    GearAbilitiesInfo { supported, update_needed: engine_ok.is_some() && !supported, abilities: if supported { abilities } else { Vec::new() } }
}

/// Validates and saves `profile.json`. The profile is applied to the game's save at the next launch (the runtime does
/// it, see `profile::prepare_launch`); there is no separate "apply" step.
#[tauri::command(async)]
pub fn profile_save(state: St, profile: Profile) -> AppResult<Profile> {
    let clean = validated(&state, &profile)?;
    profile::save_profile(&state.data_dir, &clean)?;
    Ok(clean)
}

// ---- community servers by name (servers.rs) ----------------------------------------------------------------------

/// Reads a small local file the user named (a server card); never more than `max` bytes.
fn read_user_file(path: &str, max: u64, what: &str) -> AppResult<Vec<u8>> {
    let p = path.trim().trim_matches('"').trim();
    if p.is_empty() {
        return Err(AppError::InvalidInput(format!("{what}: no file chosen")));
    }
    let p = Path::new(p);
    let meta = std::fs::metadata(p).map_err(|e| AppError::io(p, &e))?;
    if !meta.is_file() {
        return Err(AppError::InvalidInput(format!("{what}: not a file")));
    }
    if meta.len() > max {
        return Err(AppError::InvalidInput(format!("{what}: larger than {max} bytes")));
    }
    std::fs::read(p).map_err(|e| AppError::io(p, &e))
}

#[derive(Serialize)]
pub struct CardPreview {
    pub name: String,
    pub relay: Option<String>,
    pub relay_port: Option<u16>,
    pub api: Option<String>,
}

/// Parses a server card of the first format (`servercard.rs`) and shows what it holds. Nothing is saved, nothing is
/// contacted.
#[tauri::command]
pub fn server_card_preview(path: String) -> AppResult<CardPreview> {
    let card = servercard::parse_card(&read_user_file(&path, servercard::MAX_CARD_BYTES, "server card")?)?;
    Ok(CardPreview {
        name: card.name,
        relay: card.relay.as_ref().map(|(h, _)| h.clone()),
        relay_port: card.relay.as_ref().and_then(|(_, p)| *p),
        api: card.api,
    })
}

/// Saves the server of a card the player has seen and confirmed.
#[tauri::command]
pub fn server_card_import(state: St, path: String) -> AppResult<ServersView> {
    let card = servercard::parse_card(&read_user_file(&path, servercard::MAX_CARD_BYTES, "server card")?)?;
    let input = online::ServerInput {
        id: None,
        name: card.name,
        relay: card.relay.as_ref().map(|(h, _)| h.clone()).unwrap_or_default(),
        relay_port: card.relay.as_ref().and_then(|(_, p)| *p),
        api: card.api.unwrap_or_default(),
    };
    online::upsert(&state.data_dir, &input)?;
    Ok(servers_view_of(&state))
}

#[derive(Serialize)]
pub struct ServerSummary {
    pub id: String,
    pub name: String,
    pub api: Option<String>,
    pub relay_host: Option<String>,
    pub relay_port: u16,
    pub last_ip: Option<String>,
    pub selected: bool,
    /// Community service: stay connected in the background.
    pub background: bool,
}

#[derive(Serialize)]
pub struct ServersView {
    pub servers: Vec<ServerSummary>,
    pub selected: Option<String>,
    pub features: crate::servers::Features,
}

fn servers_view_of(state: &AppState) -> ServersView {
    let store = online::load(&state.data_dir);
    let ids: Vec<(String, String)> = store.servers.iter().map(|s| (s.id.clone(), s.name.clone())).collect();
    ServersView {
        features: crate::servers::features(&ids, store.selected.as_deref()),
        servers: store
            .servers
            .iter()
            .map(|s| ServerSummary {
                id: s.id.clone(),
                name: s.name.clone(),
                api: s.api.clone(),
                relay_host: s.relay.clone(),
                relay_port: s.relay_port.unwrap_or(lan::DEFAULT_RELAY_PORT),
                last_ip: s.last_ip.clone(),
                background: s.background,
                selected: store.selected.as_deref() == Some(s.id.as_str()),
            })
            .collect(),
        selected: store.selected.clone(),
    }
}

/// The servers and what they make visible. Reads local files only.
#[tauri::command]
pub fn servers_view(state: St) -> ServersView {
    servers_view_of(&state)
}

/// "Add a server" from one line (host name or IPv4, optional port). Nothing is contacted.
#[tauri::command]
pub fn server_add_line(state: St, line: String, name: Option<String>, api: Option<String>) -> AppResult<ServersView> {
    online::add_line(&state.data_dir, &line, name.as_deref(), api.as_deref())?;
    Ok(servers_view_of(&state))
}

#[tauri::command]
pub fn server_remove(state: St, id: String) -> AppResult<ServersView> {
    if state.game.status().state == RunState::Running {
        return Err(AppError::AlreadyRunning);
    }
    online::remove(&state.data_dir, &id)?;
    Ok(servers_view_of(&state))
}

#[tauri::command]
pub fn server_select(state: St, id: String) -> AppResult<ServersView> {
    online::select(&state.data_dir, Some(&id))?;
    Ok(servers_view_of(&state))
}

#[derive(Serialize)]
pub struct ResolveView {
    pub host: String,
    pub ip: String,
}

/// "Check": looks the server's relay name up now (the one network access, on request, for a server the player added).
#[tauri::command(async)]
pub fn server_resolve(state: St, id: String) -> AppResult<ResolveView> {
    let store = online::load(&state.data_dir);
    let s = store.servers.iter().find(|s| s.id == id).ok_or_else(|| AppError::InvalidInput("unknown server".into()))?;
    let host = s.relay.clone().filter(|r| !r.is_empty()).ok_or_else(|| AppError::NotAvailable("this server has no relay".into()))?;
    let ip = online::resolve_text(&crate::servers::SystemResolver, &host)?;
    online::remember_ip(&state.data_dir, &id, &ip);
    Ok(ResolveView { host, ip })
}

// ---- private match -----------------------------------------------------------------------------------------------

/// What the screens show of a match code: the decoded fields and what the runtime will get.
#[derive(Debug, Clone, Serialize)]
pub struct MatchPlan {
    pub code: String,
    pub spec: matchcode::MatchSpec,
    /// `SPLAT3_MATCH_ID` and the relay room.
    pub match_id: String,
    pub relay: String,
    pub rules: String,
    /// Only `turf` was tried with the runtime.
    pub verified: bool,
    /// The code's host nickname is this launcher's own profile name.
    pub own_code: bool,
    /// The relay's host name resolved by this launcher just now (IPv4), to show before launching; `None` for an IP
    /// relay or when the lookup failed (the match start then reports it).
    pub relay_resolved: Option<String>,
    /// The relay is one this launcher knows (a saved server's, the default match server, or one the player confirmed
    /// during this run). Else the player confirms it before the game starts (`MatchStart::confirm_relay`).
    pub relay_known: bool,
}

/// Relays of match codes the player confirmed during this run (name or address, port).
static CONFIRMED_RELAYS: Mutex<Vec<(String, u16)>> = Mutex::new(Vec::new());

fn relay_key(spec: &matchcode::MatchSpec) -> (String, u16) {
    (spec.relay_host.clone().unwrap_or_else(|| spec.relay_ip.to_string()).to_ascii_lowercase(), spec.relay_port)
}

/// The code's relay is a saved server's (by name or by the address last seen), the default match server, or one the
/// player confirmed during this run.
fn relay_known(state: &AppState, spec: &matchcode::MatchSpec) -> bool {
    let (key, port) = relay_key(spec);
    let ip = spec.relay_ip.to_string();
    if CONFIRMED_RELAYS.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|(k, p)| *k == key && *p == port) {
        return true;
    }
    let saved = online::load(&state.data_dir).servers.iter().any(|s| {
        let p = s.relay_port.unwrap_or(lan::DEFAULT_RELAY_PORT);
        p == port && (s.relay.as_deref().is_some_and(|r| r.eq_ignore_ascii_case(&key)) || (spec.relay_host.is_none() && s.last_ip.as_deref() == Some(ip.as_str())))
    });
    saved || matches!(parse_match_server(&state.cfg().match_server), Ok(Some((mip, mport))) if mip == spec.relay_ip && mport == port && spec.relay_host.is_none())
}

/// A relay address no match code may name: the same rules as the relays' latency measurement (`relayping::allowed`):
/// a port below 1024, 0/8, link-local, multicast, reserved, a broadcast address (this computer's subnets included).
/// Loopback and private addresses stay possible (a relay on this computer or the LAN), asked like any unknown relay.
fn relay_refused(ip: std::net::Ipv4Addr, port: u16) -> Option<String> {
    use crate::proto::relayping;
    let scope = relayping::scope_of(std::net::IpAddr::V4(ip));
    (!relayping::allowed(ip, port, scope, &relayping::local_subnets())).then(|| format!("{ip}:{port}"))
}

fn check_relay_address(spec: &matchcode::MatchSpec) -> AppResult<()> {
    // A code made for a relay name may carry no address (its host could not resolve it): the name is checked once resolved.
    if spec.relay_host.is_some() && spec.relay_ip.is_unspecified() {
        return Ok(());
    }
    match relay_refused(spec.relay_ip, spec.relay_port) {
        Some(addr) => Err(AppError::Validation(vec![FieldError { key: "code".into(), message: format!("relay_address:{addr}") }])),
        None => Ok(()),
    }
}

/// A code error as a validation error on the field `code`; the message is `<kind>` or `<kind>:<detail>`.
fn code_error(e: matchcode::CodeError) -> AppError {
    let message = match &e {
        matchcode::CodeError::UnsupportedVersion(v) => format!("{}:{v}", e.id()),
        matchcode::CodeError::BadCharacter(c) => format!("{}:{c}", e.id()),
        matchcode::CodeError::Field(f) => format!("{}:{f}", e.id()),
        other => other.id().to_string(),
    };
    AppError::Validation(vec![FieldError { key: "code".into(), message }])
}

fn plan_of(state: &AppState, spec: matchcode::MatchSpec) -> AppResult<MatchPlan> {
    let code = matchcode::encode(&spec).map_err(code_error)?;
    let own = profile::load_profile(&state.data_dir).name == spec.host;
    check_relay_address(&spec)?;
    let relay_known = relay_known(state, &spec);
    Ok(MatchPlan {
        relay_known,
        code,
        match_id: spec.match_id(),
        // The relay as the player gave it: its host name when it has one (version 2 codes), else the IPv4.
        relay: { let h = spec.relay_host.clone().unwrap_or_else(|| spec.relay_ip.to_string()); if spec.relay_port == lan::DEFAULT_RELAY_PORT { h } else { format!("{h}:{}", spec.relay_port) } },
        rules: automatch::rules_string(&spec),
        verified: spec.mode == "turf",
        own_code: own,
        relay_resolved: None,
        spec,
    })
}

/// `ip` or `ip:port` of the default match server. Empty clears it.
pub fn parse_match_server(s: &str) -> Result<Option<(std::net::Ipv4Addr, u16)>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let (ip, port) = match s.split_once(':') {
        Some((ip, port)) => (ip, port.parse::<u16>().ok().filter(|p| *p > 0).ok_or("port: 1 to 65535")?),
        None => (s, lan::DEFAULT_RELAY_PORT),
    };
    Ok(Some((lan::parse_ipv4(ip)?, port)))
}

fn show_server(ip: std::net::Ipv4Addr, port: u16) -> String {
    if port == lan::DEFAULT_RELAY_PORT { ip.to_string() } else { format!("{ip}:{port}") }
}

fn match_server(cfg: &LauncherConfig) -> Result<(std::net::Ipv4Addr, u16), AppError> {
    let field = |message: String| AppError::Validation(vec![FieldError { key: "match_server".into(), message }]);
    match parse_match_server(&cfg.match_server) {
        Ok(Some(s)) => Ok(s),
        Ok(None) => Err(field("empty".into())),
        Err(m) => Err(field(m)),
    }
}

#[tauri::command]
pub fn set_match_server(state: St, value: String) -> AppResult<ConfigView> {
    let normalized = match parse_match_server(&value) {
        Ok(None) => String::new(),
        Ok(Some((ip, port))) => show_server(ip, port),
        Err(m) => return Err(AppError::Validation(vec![FieldError { key: "match_server".into(), message: m }])),
    };
    state.update_config(|c| c.match_server = normalized)?;
    Ok(config_view(&state))
}

/// Reads the default match server from a server card (`match_server`, else `relay`, as `ip` or `ip:port`). Returns the
/// text to put in the field; nothing is saved and nothing else of the card is used here.
#[tauri::command]
pub fn match_server_from_card(path: String) -> AppResult<String> {
    let bytes = read_user_file(&path, servercard::MAX_CARD_BYTES, "server card")?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| AppError::InvalidInput(format!("server card: not JSON ({e})")))?;
    let text = ["match_server", "relay"]
        .iter()
        .find_map(|k| v.get(*k).and_then(|x| x.as_str()))
        .ok_or_else(|| AppError::InvalidInput("server card: no relay address (match_server or relay)".into()))?;
    match parse_match_server(text) {
        Ok(Some((ip, port))) => Ok(show_server(ip, port)),
        Ok(None) => Err(AppError::InvalidInput("server card: empty relay address".into())),
        Err(m) => Err(AppError::InvalidInput(format!("server card: {m}"))),
    }
}

#[derive(Deserialize)]
pub struct MatchCreate {
    pub players: u8,
    pub mode: String,
    /// Keep this room (changing the rules of a match already announced); else a fresh one.
    #[serde(default)]
    pub room: Option<String>,
}

/// The relay of private matches: the selected community server's (else the first one's), else the default match
/// server of earlier launchers. A host name is resolved now (the code also carries the address as a fallback).
fn match_relay(state: &AppState) -> AppResult<(Option<String>, std::net::Ipv4Addr, u16)> {
    let store = online::load(&state.data_dir);
    let server = store.selected.as_deref().and_then(|id| store.servers.iter().find(|s| s.id == id && s.relay.is_some())).or_else(|| store.servers.iter().find(|s| s.relay.is_some()));
    if let Some(s) = server {
        let host = s.relay.clone().unwrap_or_default();
        let port = s.relay_port.unwrap_or(lan::DEFAULT_RELAY_PORT);
        let ip: std::net::Ipv4Addr = online::resolve_text(&crate::servers::SystemResolver, &host)?.parse().map_err(|_| AppError::Internal("resolver".into()))?;
        online::remember_ip(&state.data_dir, &s.id, &ip.to_string());
        let named = matches!(crate::servers::parse_host(&host), Ok(crate::servers::Host::Name(_)));
        return Ok((named.then_some(host), ip, port));
    }
    let (ip, port) = match_server(&state.cfg())?;
    Ok((None, ip, port))
}

/// A new private match hosted by this player: fresh room, the community server's relay, the profile name as nickname.
#[tauri::command(async)]
pub fn match_create(state: St, request: MatchCreate) -> AppResult<MatchPlan> {
    let (relay_host, ip, port) = match_relay(&state)?;
    let name = profile::load_profile(&state.data_dir).name.trim().to_string();
    let named = relay_host.is_some();
    let spec = matchcode::MatchSpec { relay_host, relay_ip: ip, relay_port: port, room: request.room.unwrap_or_else(lan::generate_room_code), round: 0, host: name, players: request.players, mode: request.mode };
    let mut plan = plan_of(&state, spec)?;
    plan.relay_resolved = named.then(|| ip.to_string());
    Ok(plan)
}

/// Decodes a pasted code. The error is a validation error on the field `code`.
#[tauri::command]
pub fn match_parse(state: St, code: String) -> AppResult<MatchPlan> {
    with_lookup(plan_of(&state, matchcode::decode(&code).map_err(code_error)?)?)
}

/// Shows the address a relay host name resolves to now (a lookup the match start repeats; nothing is kept).
fn with_lookup(mut plan: MatchPlan) -> AppResult<MatchPlan> {
    if let Some(h) = plan.spec.relay_host.clone() {
        plan.relay_resolved = online::resolve_text(&crate::servers::SystemResolver, &h).ok();
    }
    Ok(plan)
}

/// The same group's next match: the code with the round incremented (the room stays the group's).
#[tauri::command]
pub fn match_next_round(state: St, code: String) -> AppResult<MatchPlan> {
    let spec = matchcode::decode(&code).map_err(code_error)?;
    let next = spec.next_round().ok_or_else(|| AppError::InvalidInput("this group has played the maximum number of rounds: create a new match".into()))?;
    with_lookup(plan_of(&state, next)?)
}

/// What the launcher remembers of the match it started (`last_match.json`), for the screens that follow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchRun {
    pub match_id: String,
    pub role: automatch::Role,
    pub code: String,
    pub started_at: u64,
    pub status_path: String,
    pub result_path: String,
    pub log_path: String,
    pub host: String,
    pub players: u8,
    pub mode: String,
}

fn last_match_path(data_dir: &Path) -> PathBuf {
    data_dir.join("last_match.json")
}

fn load_match_run(data_dir: &Path) -> Option<MatchRun> {
    util::read_text_limited(&last_match_path(data_dir), 1 << 20).ok().flatten().and_then(|t| serde_json::from_str(&t).ok())
}

#[derive(Deserialize)]
pub struct MatchStart {
    pub code: String,
    pub role: automatch::Role,
    /// The player confirmed the code's relay, which this launcher did not know (`MatchPlan::relay_known`).
    #[serde(default)]
    pub confirm_relay: bool,
}

#[tauri::command(async)]
pub fn match_start(app: AppHandle, state: St, request: MatchStart) -> AppResult<MatchRun> {
    let mut spec = matchcode::decode(&request.code).map_err(code_error)?;
    // A code from another player chooses the relay the game talks to: never an address a relay
    // cannot have, and an unknown relay only once the player confirmed it.
    check_relay_address(&spec)?;
    if !relay_known(&state, &spec) {
        if !request.confirm_relay {
            return Err(AppError::Validation(vec![FieldError { key: "relay".into(), message: "unconfirmed".into() }]));
        }
        let mut g = CONFIRMED_RELAYS.lock().unwrap_or_else(|p| p.into_inner());
        if g.len() < 64 {
            g.push(relay_key(&spec));
        }
    }
    // A relay given by name: resolved by this player's launcher now (the game only takes an IPv4). The address the
    // host's launcher saw is the fallback when the lookup fails here.
    if let Some(host) = spec.relay_host.clone() {
        match online::resolve_text(&crate::servers::SystemResolver, &host) {
            Ok(ip) => spec.relay_ip = ip.parse().map_err(|_| AppError::Internal("resolver".into()))?,
            Err(e) if spec.relay_ip.is_unspecified() => return Err(e),
            Err(_) => {}
        }
        if let Some(addr) = relay_refused(spec.relay_ip, spec.relay_port) {
            return Err(AppError::Validation(vec![FieldError { key: "code".into(), message: format!("relay_address:{host} -> {addr}") }]));
        }
    }
    let cfg = state.cfg();
    let root = state.root()?;
    let userdata = install::userdata_dir(&root);
    let name = profile::load_profile(&state.data_dir).name.trim().to_string();
    let vars = automatch::MatchVars::load(&state.data_dir);
    let paths = automatch::match_paths(&userdata, &spec.match_id());
    let mut env = automatch::build_match_env(&vars, request.role, &spec, &paths, &name)?;
    if !cfg.apply_profile {
        env.push(("SPLAT3_NICKNAME".into(), name.clone())); // else the profile step sets it
    }
    // A leftover file from an earlier match with the same id must never be taken for this one.
    std::fs::create_dir_all(&paths.dir).map_err(|e| AppError::io(&paths.dir, &e))?;
    for f in [&paths.status, &paths.result] {
        let _ = std::fs::remove_file(f);
    }
    let status = launch_with(&state, app, Slot::First, launch::MATCH_MODE, false, false, &[], Some(env))?;
    let run = MatchRun {
        match_id: spec.match_id(),
        role: request.role,
        code: matchcode::encode(&spec).map_err(code_error)?,
        started_at: status.started_at.unwrap_or_else(util::now_secs),
        status_path: paths.status.display().to_string(),
        result_path: paths.result.display().to_string(),
        log_path: status.log_path.clone().unwrap_or_default(),
        host: spec.host.clone(),
        players: spec.players,
        mode: spec.mode.clone(),
    };
    if let Ok(text) = serde_json::to_string_pretty(&run) {
        let _ = util::atomic_write(&last_match_path(&state.data_dir), text.as_bytes());
    }
    Ok(run)
}

/// A match of a community server (`proto::runner`): the game starts like a private match, with the environment of
/// the match contract (`proto::contract`), the player's profile and the usual launch checks.
pub(crate) fn launch_protocol_match(app: AppHandle, env: Vec<(String, String)>) -> AppResult<GameStatus> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    // A match found while an update puts the new executable in place (a few seconds) waits for it rather than failing.
    if let Ok(root) = state.root() {
        for _ in 0..60 {
            if !state.installer.replacing(&root) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    launch_with(&state, app.clone(), Slot::First, launch::MATCH_MODE, false, false, &[], Some(env))
}

#[derive(Serialize)]
pub struct MatchProgress {
    pub run: Option<MatchRun>,
    pub game: GameStatus,
    pub status: Option<automatch::MatchStatus>,
    /// A parsable result file exists.
    pub result_ready: bool,
    pub elapsed_s: u64,
}

/// Where the running (or last) match stands: the status file when it exists, else the run log. Cheap enough to poll.
#[tauri::command(async)]
pub fn match_poll(state: St) -> MatchProgress {
    let game = state.game.status();
    let run = load_match_run(&state.data_dir);
    let mut status = None;
    let mut result_ready = false;
    if let Some(r) = &run {
        status = util::read_text_limited(Path::new(&r.status_path), 1 << 20)
            .ok()
            .flatten()
            .and_then(|t| automatch::parse_status(&t))
            .filter(|s| s.match_id.as_deref().is_none_or(|id| id == r.match_id));
        if status.is_none() {
            status = automatch::status_from_log(&read_head(Path::new(&r.log_path), 2 << 20));
        }
        result_ready = util::read_text_limited(Path::new(&r.result_path), 1 << 20).ok().flatten().is_some_and(|t| automatch::parse_result(&t).is_ok());
    }
    let elapsed_s = run.as_ref().map(|r| util::now_secs().saturating_sub(r.started_at)).unwrap_or(0);
    MatchProgress { run, game, status, result_ready, elapsed_s }
}

/// The last match's result file. `None` when the game wrote none; an unreadable file is an error with the reason.
#[tauri::command(async)]
pub fn match_result(state: St) -> AppResult<Option<automatch::ResultView>> {
    let Some(run) = load_match_run(&state.data_dir) else { return Ok(None) };
    let Some(text) = util::read_text_limited(Path::new(&run.result_path), 1 << 20)? else { return Ok(None) };
    automatch::parse_result(&text).map(|r| Some(r.view())).map_err(AppError::InvalidInput)
}

/// The last match, for the "resume / play again" entry points.
#[tauri::command]
pub fn match_last(state: St) -> Option<MatchRun> {
    load_match_run(&state.data_dir)
}

// ---- data of earlier launchers ------------------------------------------------------------------------------------

/// The one-time import notice: only while it has not been shown and something was imported or rejected.
#[tauri::command]
pub fn migration_status(state: St) -> Option<migrate::Migration> {
    migrate::marker(&state.data_dir).filter(|m| !m.shown)
}

/// The last migration (for the details link and the Settings page), shown or not.
#[tauri::command]
pub fn migration_last(state: St) -> Option<migrate::Migration> {
    migrate::marker(&state.data_dir)
}

#[tauri::command]
pub fn migration_ack(state: St) -> AppResult<()> {
    migrate::acknowledge(&state.data_dir)
}

/// Manual import from an earlier launcher's data folder (a path typed by the person). Copies, never moves.
#[tauri::command(async)]
pub fn migration_import(state: St, path: String) -> AppResult<migrate::Migration> {
    let source = existing_dir(&path, "data folder")?;
    let m = migrate::import_from(&state.data_dir, &source, true)?;
    let fresh = config::load(&state.data_dir);
    *state.config.lock().unwrap_or_else(|p| p.into_inner()) = fresh;
    Ok(m)
}

fn sanitized_ui(ui: config::UiPrefs) -> config::UiPrefs {
    let last_mode = if config::QUICK_MODES.contains(&ui.last_mode.as_str()) { ui.last_mode.clone() } else { config::UiPrefs::default().last_mode };
    config::UiPrefs { match_players: ui.match_players.clamp(2, 8), last_mode, sound_volume: ui.sound_volume.min(100), ..ui }
}

#[tauri::command]
pub fn set_ui_prefs(state: St, ui: config::UiPrefs) -> AppResult<ConfigView> {
    let ui = sanitized_ui(ui);
    state.update_config(|c| c.ui = ui)?;
    Ok(config_view(&state))
}

/// Settings > About: automatic check and channel of the launcher's own updates.
#[tauri::command]
pub fn update_set_prefs(app: AppHandle, state: St, prefs: crate::update::UpdatePrefs) -> AppResult<ConfigView> {
    state.update_config(|c| c.update = prefs.clone())?;
    crate::update::prefs_changed(&app, &prefs);
    Ok(config_view(&state))
}

// ---- install wizard ------------------------------------------------------------------------------------------

use crate::installer::{self, InstallRequest, Snapshot};

/// The folder holding the runtime source archive and manifest: a valid `resources` folder, else the copy embedded in the
/// executable, written under the launcher's data folder (`KALAMAR_DATA` respected: it is `AppState::data_dir`).
pub(crate) fn bundle_dir(app: &AppHandle) -> Result<PathBuf, installer::bundle::BundleError> {
    use tauri::Manager;
    let data = app.state::<AppState>().data_dir.clone();
    installer::bundle::resolve(app.path().resource_dir().ok().as_deref(), &data)
}

fn runtime_missing(e: installer::bundle::BundleError) -> AppError {
    AppError::RuntimeMissing(e.to_string())
}

#[derive(Serialize)]
pub struct WizardInfo {
    /// Pre-filled request: the remembered folder and files, else the default folder.
    pub request: InstallRequest,
    /// A run was started earlier and did not finish (closed launcher, failure, restart).
    pub pending: bool,
    pub snapshot: Snapshot,
    pub bundle: Option<installer::bundle::Manifest>,
    /// Why there is no `bundle` (the launcher's own files are incomplete), for a message shown at once.
    pub bundle_error: Option<String>,
    pub default_jobs: u32,
    pub cpus: usize,
    pub ram: Option<u64>,
    /// Outcome recorded in the folder's state file, when there is one.
    pub state_finished: Option<bool>,
    pub needs_reboot: bool,
    pub last_error: Option<installer::InstallError>,
    /// Installs found on this PC (the pre-release installer's default folder, the configured root, where the launcher runs).
    pub existing: Vec<installer::Existing>,
    /// The step an unfinished installation stopped at (launcher closed, crash, reboot, cancel, failure).
    pub interrupted_step: Option<String>,
    /// What was put right at start after an interrupted installation (temporary files removed, previous game restored).
    pub recovered: Vec<String>,
    /// The game version this launcher builds (`installer::gamever`).
    pub game_version: &'static str,
    /// A first install on this PC: free space needed from the start, space taken at the end, estimated seconds, bytes
    /// downloaded (`installer::fresh_estimate`).
    pub fresh: FreshEstimate,
}

#[derive(Serialize)]
pub struct FreshEstimate {
    pub disk_need: u64,
    pub footprint: u64,
    pub eta: f64,
    pub download_bytes: u64,
}

fn default_install_dir() -> String {
    let home = crate::config::user_profile().unwrap_or_else(std::env::temp_dir);
    home.join("Kalamar").display().to_string()
}

#[tauri::command]
pub fn installer_info(app: AppHandle, state: St) -> WizardInfo {
    let cfg = state.cfg();
    let p = &cfg.install;
    let request = InstallRequest {
        install_dir: p.dir.clone().unwrap_or_else(default_install_dir),
        base_nsp: p.base_nsp.clone(),
        update_nsp: p.update_nsp.clone(),
        prod_keys: p.prod_keys.clone(),
        jobs: p.jobs,
        allow_prereqs: false,
        force_rebuild: false,
        free_build_files: false,
        low_priority: false,
    };
    let st = installer::StateFile::load(&installer::Layout::new(&request.install_dir).state());
    let active = state.installer.active();
    let recovered = if !active && st.as_ref().is_some_and(|s| !s.finished) { installer::recover_interrupted(Path::new(&request.install_dir)) } else { Vec::new() };
    let bundle = bundle_dir(&app);
    WizardInfo {
        bundle_error: bundle.as_ref().err().map(ToString::to_string),
        bundle: bundle.ok().as_deref().and_then(installer::bundle::load_manifest),
        interrupted_step: if active { None } else { st.as_ref().filter(|s| !s.finished).and_then(|s| s.current.clone()) },
        recovered,
        game_version: installer::gamever::CURRENT.display,
        pending: p.pending && st.as_ref().is_some_and(|s| !s.finished),
        state_finished: st.as_ref().map(|s| s.finished),
        needs_reboot: st.as_ref().is_some_and(|s| s.needs_reboot),
        last_error: st.and_then(|s| s.last_error),
        request,
        snapshot: state.installer.snapshot(),
        default_jobs: installer::default_jobs(),
        cpus: installer::sys::cpus(),
        ram: installer::sys::total_ram(),
        existing: installer::find_existing(&installer::default_candidates(state.root().ok().as_deref())),
        fresh: {
            let (disk_need, footprint, eta, download_bytes) = installer::fresh_estimate(if p.jobs == 0 { installer::default_jobs() } else { p.jobs });
            FreshEstimate { disk_need, footprint, eta, download_bytes }
        },
    }
}

/// Checks one of the player's files as soon as it is picked: `base`, `update` or `keys`.
#[tauri::command(async)]
pub fn installer_check_file(kind: String, path: String) -> AppResult<installer::nsp::FileCheck> {
    let p = PathBuf::from(path.trim().trim_matches('"'));
    if path.len() > 1024 || path.contains('\0') {
        return Err(AppError::InvalidInput("path".into()));
    }
    Ok(match kind.as_str() {
        "base" => installer::nsp::check_base(&p),
        "update" => installer::nsp::check_update(&p),
        "keys" => installer::nsp::check_keys(&p),
        _ => return Err(AppError::InvalidInput(format!("unknown file kind '{kind}'"))),
    })
}

#[derive(Serialize)]
pub struct DirCheck {
    /// `new`, `wizard`, `kit` or `other` (a folder holding something else: refused).
    pub kind: &'static str,
    pub free: Option<u64>,
    pub need: u64,
    pub absolute: bool,
    /// The player's files are needed (no extracted data in this folder yet).
    pub needs_files: bool,
}

#[tauri::command(async)]
pub fn installer_check_dir(path: String) -> DirCheck {
    let p = PathBuf::from(path.trim().trim_matches('"'));
    let (kind, _) = installer::folder_kind(&p);
    let lay = installer::Layout::new(&p);
    DirCheck {
        kind,
        free: installer::sys::free_bytes(&p),
        need: installer::first_need(&lay),
        absolute: p.is_absolute() && p.parent().is_some(),
        needs_files: installer::needs_player_files(&lay),
    }
}

/// Refuses a request the wizard cannot run: relative or root folders, a folder holding other things, missing files,
/// or a player's file inside the folders the wizard manages (an uninstall would remove it).
fn validate_request(r: &InstallRequest) -> AppResult<InstallRequest> {
    let clean = |s: &str| s.trim().trim_matches('"').to_string();
    let mut r = r.clone();
    r.install_dir = clean(&r.install_dir);
    r.base_nsp = clean(&r.base_nsp);
    r.update_nsp = clean(&r.update_nsp);
    r.prod_keys = clean(&r.prod_keys);
    let dir = PathBuf::from(&r.install_dir);
    let mut bad = Vec::new();
    if !dir.is_absolute() || dir.parent().is_none() || r.install_dir.len() > 200 {
        bad.push(FieldError { key: "install_dir".into(), message: "absolute_folder".into() });
    } else if installer::folder_kind(&dir).1 {
        bad.push(FieldError { key: "install_dir".into(), message: "folder_not_empty".into() });
    }
    // An install that already has the extracted ExeFS and merged RomFS is updated without the player's files.
    let need_files = !dir.is_absolute() || installer::needs_player_files(&installer::Layout::new(&dir));
    for (key, p) in [("base_nsp", &r.base_nsp), ("update_nsp", &r.update_nsp), ("prod_keys", &r.prod_keys)] {
        let pb = PathBuf::from(p);
        if !need_files && p.is_empty() {
            continue;
        }
        if p.is_empty() || !pb.is_file() {
            bad.push(FieldError { key: key.into(), message: "file_missing".into() });
        } else if dir.is_absolute() && ["src", "work", "logs"].iter().any(|d| pb.starts_with(dir.join(d))) {
            bad.push(FieldError { key: key.into(), message: "file_inside_install".into() });
        }
    }
    if !bad.is_empty() {
        return Err(AppError::Validation(bad));
    }
    Ok(r)
}

#[tauri::command(async)]
pub fn installer_plan(app: AppHandle, request: InstallRequest) -> AppResult<installer::Plan> {
    let r = validate_request(&request)?;
    let dir = bundle_dir(&app).map_err(runtime_missing)?;
    Ok(installer::plan(&r, Some(&dir)))
}

/// Can the installed game be updated to the runtime source this launcher carries, and what would that redo (Play
/// page)? The install folder is the wizard's, else the one above the configured game root (`<install>\work\<game>`).
/// `None` while an installation runs, without an install, or when the launcher's own source is missing.
#[tauri::command(async)]
pub fn installer_update_check(app: AppHandle, state: St) -> Option<installer::UpdateCheck> {
    if state.installer.active() {
        return None;
    }
    let cfg = state.cfg();
    let from_root = cfg.root.as_deref().map(PathBuf::from).and_then(|r| r.parent().and_then(Path::parent).map(Path::to_path_buf));
    let bundle = bundle_dir(&app).ok()?;
    let mut c = cfg.install.dir.as_deref().map(PathBuf::from).into_iter().chain(from_root).find_map(|d| installer::update_check(&d, &bundle))?;
    c.at_close = state.at_close.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_some_and(|a| a.request.install_dir.eq_ignore_ascii_case(&c.install_dir));
    Some(c)
}

/// "Update when the launcher closes": the update of `request` starts when the window is closed; the launcher then
/// stays in the notification area for its time (`texts`: the icon's words) and says when it ends. `None` cancels.
#[tauri::command]
pub fn installer_at_close(state: St, request: Option<InstallRequest>, texts: Option<crate::tray::TrayTexts>) -> AppResult<bool> {
    let next = match request {
        Some(r) => Some(AtClose { request: validate_request(&r)?, texts: texts.unwrap_or_default() }),
        None => None,
    };
    let on = next.is_some();
    *state.at_close.lock().unwrap_or_else(|p| p.into_inner()) = next;
    Ok(on)
}

/// The window is being closed with an update chosen for that moment: the window goes, the icon stays in the
/// notification area, the update starts. `false` when it could not start (the launcher then simply closes).
pub fn start_at_close(app: &AppHandle) -> bool {
    use std::sync::atomic::Ordering;
    use tauri::Manager;
    let state = app.state::<AppState>();
    let Some(ac) = state.at_close.lock().unwrap_or_else(|p| p.into_inner()).take() else { return false };
    if state.installer.active() || start_install(app, &state, ac.request).is_err() {
        return false;
    }
    state.closing.store(true, Ordering::SeqCst);
    crate::tray::keep(app, ac.texts, 0.0);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    true
}

/// The end of an update that ran at close: once it is done the launcher quits (after a short while, so its
/// notification can be read and clicked), unless the window was brought back meanwhile; a failure keeps the icon, so the
/// player can open the launcher and see why.
fn finish_at_close(app: &AppHandle, outcome: &str) {
    use std::sync::atomic::Ordering;
    use tauri::Manager;
    let state = app.state::<AppState>();
    if !state.closing.swap(false, Ordering::SeqCst) {
        return;
    }
    if outcome != "done" || crate::tray::reopened() {
        if crate::tray::reopened() {
            crate::tray::release();
        }
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(15));
        let shown = app.get_webview_window("main").is_some_and(|w| w.is_visible().unwrap_or(false));
        if crate::tray::reopened() || shown {
            crate::tray::release();
        } else {
            crate::tray::shutdown();
            app.exit(0);
        }
    });
}

/// The end of an installation, said where the player is: nothing more while the launcher's window is in front (the
/// page shows it, with its sound); otherwise the taskbar button flashes (when the window is in the taskbar) and, unless
/// the player turned them off, a Windows notification says it. Returns whether a notification was shown.
#[tauri::command]
pub fn notify_attention(app: AppHandle, state: St, title: String, body: String) -> bool {
    use tauri::Manager;
    let Some(w) = app.get_webview_window("main") else { return false };
    let visible = w.is_visible().unwrap_or(false);
    let minimized = w.is_minimized().unwrap_or(false);
    if visible && !minimized && crate::winutil::is_foreground(&w) {
        return false;
    }
    if visible {
        let _ = w.request_user_attention(Some(tauri::UserAttentionType::Informational));
    }
    if !state.cfg().ui.notify {
        return false;
    }
    crate::tray::notify(&app, &title, &body)
}

#[tauri::command]
pub fn installer_start(app: AppHandle, state: St, request: InstallRequest) -> AppResult<Snapshot> {
    start_install(&app, &state, request)
}

/// Starts an installation or an update. The game of that install may be running: every step goes on beside it except
/// the link, which waits for it to be closed (`installer::Run::step_compile`); launches wait only the few seconds the
/// new executable is put in place.
pub fn start_install(app: &AppHandle, state: &AppState, request: InstallRequest) -> AppResult<Snapshot> {
    let r = validate_request(&request)?;
    let bundle = bundle_dir(app).map_err(runtime_missing)?;
    state.update_config(|c| {
        c.install = config::InstallPrefs {
            dir: Some(r.install_dir.clone()),
            base_nsp: r.base_nsp.clone(),
            update_nsp: r.update_nsp.clone(),
            prod_keys: r.prod_keys.clone(),
            jobs: r.jobs,
            pending: true,
        }
    })?;
    let app2 = app.clone();
    let emit: installer::Emit = std::sync::Arc::new(move |s: &Snapshot| {
        let _ = app2.emit("installer-progress", s);
        crate::tray::progress(s.overall);
        if s.outcome == Some("done") {
            use tauri::Manager;
            let st = app2.state::<AppState>();
            let _ = st.update_config(|c| c.install.pending = false);
        }
        if let Some(outcome) = s.outcome {
            finish_at_close(&app2, outcome);
        }
    });
    let app3 = app.clone();
    let on_root: installer::OnRoot = std::sync::Arc::new(move |root: &Path| {
        use tauri::Manager;
        let st = app3.state::<AppState>();
        let before = st.cfg();
        let _ = st.update_config(|c| c.root = Some(root.display().to_string()));
        // A new folder replaces a game configured elsewhere: its settings file comes along when the new one has none
        // (what the player set during this installation is already in the new one and stays).
        if let Some(old) = before.root.as_deref().map(PathBuf::from).filter(|o| !o.as_os_str().eq_ignore_ascii_case(root.as_os_str())) {
            let (from, to) = (install::ini_path(&old), install::ini_path(root));
            if from.is_file() && !to.exists() {
                let _ = std::fs::create_dir_all(root.join("build"));
                let _ = std::fs::copy(&from, root.join("build").join(from.file_name().unwrap_or_default()));
            }
        }
        // An install made by the pre-release installer: its launcher kept its data in <install>\launcher-data (Jouer.cmd). Copy it
        // once (never move, never overwrite), so the profile and settings follow the player.
        let old = root.parent().and_then(Path::parent).map(|i| i.join("launcher-data"));
        if let Some(old) = old.filter(|p| p.is_dir())
            && !st.data_dir.join("profile.json").is_file()
        {
            // The wizard has just written this launcher's config.json: the old one replaces it (a backup copy is made
            // first), then the game root and the wizard's own settings are put back, with the language and the
            // interface choices made during this installation (they are newer than the old launcher's).
            let current = st.cfg();
            if migrate::import_from(&st.data_dir, &old, true).is_ok() {
                let mut next = config::load(&st.data_dir);
                next.root = Some(root.display().to_string());
                next.install = current.install;
                if !current.language.is_empty() {
                    next.language = current.language;
                }
                next.ui = current.ui;
                let _ = config::save(&st.data_dir, &next);
                *st.config.lock().unwrap_or_else(|p| p.into_inner()) = next;
            }
        }
    });
    state.installer.start(r, Some(bundle), emit, on_root).map_err(AppError::ModeConflict)
}

/// The window is being closed during an installation (the page asked what to do): `background` keeps it running with
/// the window minimized (hidden again, in the notification area, for an update that runs at close); `stop` cancels it
/// (the state stays resumable), waits for the step to end, and quits.
#[tauri::command(async)]
pub fn installer_close(app: AppHandle, state: St, choice: String) -> AppResult<()> {
    use tauri::Manager;
    match choice.as_str() {
        "background" => {
            if let Some(w) = app.get_webview_window("main") {
                if crate::tray::kept() {
                    crate::tray::rekeep();
                    let _ = w.hide();
                } else {
                    let _ = w.minimize();
                }
            }
            Ok(())
        }
        "stop" => {
            state.installer.cancel();
            for _ in 0..100 {
                if !state.installer.active() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            app.exit(0);
            Ok(())
        }
        other => Err(AppError::InvalidInput(format!("unknown choice '{other}'"))),
    }
}

#[tauri::command]
pub fn installer_status(state: St) -> Snapshot {
    state.installer.snapshot()
}

#[tauri::command]
pub fn installer_cancel(state: St) {
    state.installer.cancel();
}

#[tauri::command]
pub fn installer_open_logs(dir: String) -> AppResult<String> {
    let logs = installer::Layout::new(dir.trim()).logs();
    std::fs::create_dir_all(&logs).map_err(|e| AppError::io(&logs, &e))?;
    #[cfg(windows)]
    {
        std::process::Command::new(crate::winutil::windows_exe("explorer.exe")).arg(&logs).spawn().map_err(|e| AppError::io(&logs, &e))?;
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(&logs).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().map_err(|e| AppError::io(&logs, &e))?;
    }
    Ok(logs.display().to_string())
}

/// "Revert to the previous version": swaps the game executable with the one the last update kept.
#[tauri::command(async)]
pub fn installer_rollback(state: St, dir: String) -> AppResult<installer::RollbackReport> {
    if state.installer.active() {
        return Err(AppError::ModeConflict("an installation is running".into()));
    }
    if state.game.status().state == RunState::Running || state.game2.status().state == RunState::Running {
        return Err(AppError::AlreadyRunning);
    }
    installer::rollback(Path::new(dir.trim())).map_err(AppError::NotAvailable)
}

#[tauri::command(async)]
pub fn installer_uninstall(state: St, dir: String, remove_saves: bool) -> AppResult<installer::UninstallReport> {
    if state.installer.active() {
        return Err(AppError::ModeConflict("an installation is running".into()));
    }
    if state.game.status().state == RunState::Running || state.game2.status().state == RunState::Running {
        return Err(AppError::AlreadyRunning);
    }
    let root = PathBuf::from(dir.trim());
    let lay = installer::Layout::new(&root);
    let report = installer::uninstall(&root, remove_saves).map_err(AppError::NotAvailable)?;
    let game = lay.game().display().to_string();
    state.update_config(|c| {
        if c.root.as_deref() == Some(game.as_str()) {
            c.root = None;
        }
        c.install.pending = false;
    })?;
    Ok(report)
}
#[cfg(test)]
mod tests {
    #[test]
    fn the_sub_ability_editor_needs_the_engine_capability_and_the_ability_list() {
        let list = || vec!["Alpha".to_string(), "Beta".to_string()];
        let g = gear_abilities_status(Some(true), list());
        assert_eq!((g.supported, g.update_needed, g.abilities.len()), (true, false, 2));
        // an engine without the capability: the game update is what the Locker asks for
        let g = gear_abilities_status(Some(false), list());
        assert_eq!((g.supported, g.update_needed, g.abilities.len()), (false, true, 0));
        // the engine is fine but the ability list cannot be read (no keys file): not shown, and an update redoes it
        let g = gear_abilities_status(Some(true), Vec::new());
        assert_eq!((g.supported, g.update_needed), (false, true));
        // no game installed: nothing to say
        let g = gear_abilities_status(None, list());
        assert_eq!((g.supported, g.update_needed), (false, false));
    }

    use super::*;

    fn state(tag: &str, cfg: LauncherConfig) -> AppState {
        let dir = std::env::temp_dir().join(format!("s3l-cmd-{tag}-{}", util::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        AppState::new(dir, cfg)
    }

    #[test]
    fn settings_during_an_install_go_to_the_folder_it_builds() {
        let base = std::env::temp_dir().join(format!("s3l-setroot-{}", util::random_hex(4)));
        let (old, new) = (base.join("old"), base.join("new"));
        std::fs::create_dir_all(&new).unwrap();
        // a first install: nothing configured (or a configured folder without a game), the install's folder, even when a
        // folder with a game is guessed (a developer's tree)
        let guessed = Some(base.join("guessed"));
        assert_eq!(settings_root_of(None, Some(new.clone()), guessed.clone()).unwrap(), new);
        assert_eq!(settings_root_of(Some(old.clone()), Some(new.clone()), None).unwrap(), new);
        assert!(matches!(settings_root_of(None, None, None), Err(AppError::NoRoot)));
        assert_eq!(settings_root_of(None, None, guessed.clone()).unwrap(), base.join("guessed"));
        assert_eq!(settings_root_of(Some(old.clone()), None, guessed).unwrap(), old);
        // a built game is configured (an update of it, or a reinstall elsewhere while it is played): that one, the one
        // the game reads; a reinstall elsewhere takes its file along at its layout step
        std::fs::create_dir_all(old.join("build")).unwrap();
        std::fs::write(install::exe_path(&old), b"x").unwrap();
        assert_eq!(settings_root_of(Some(old.clone()), Some(new.clone()), None).unwrap(), old);
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// A match code from another player could make the game send HELLOs to any IPv4 (a LAN service, a
    /// subnet broadcast). Such addresses are refused; loopback and private relays stay possible (asked like any unknown
    /// relay).
    #[test]
    fn a_match_code_cannot_aim_the_game_at_any_address() {
        use std::net::Ipv4Addr;
        for (ip, port) in [((0, 0, 0, 0), 47313), ((224, 0, 0, 1), 47313), ((255, 255, 255, 255), 47313), ((169, 254, 1, 1), 47313), ((203, 0, 113, 7), 53), ((203, 0, 113, 7), 1023), ((240, 0, 0, 1), 47313)] {
            let ip = Ipv4Addr::new(ip.0, ip.1, ip.2, ip.3);
            assert!(relay_refused(ip, port).is_some(), "{ip}:{port}");
        }
        for (ip, port) in [((203, 0, 113, 7), 47313), ((127, 0, 0, 1), 47399), ((192, 168, 1, 20), 47313), ((100, 64, 3, 9), 5000)] {
            let ip = Ipv4Addr::new(ip.0, ip.1, ip.2, ip.3);
            assert_eq!(relay_refused(ip, port), None, "{ip}:{port}");
        }
        let spec = |ip: Ipv4Addr, port: u16, host: Option<&str>| matchcode::MatchSpec { relay_host: host.map(str::to_string), relay_ip: ip, relay_port: port, room: "A".repeat(24), round: 0, host: "Ana".into(), players: 2, mode: "turf".into() };
        let e = check_relay_address(&spec(Ipv4Addr::new(239, 255, 255, 250), 47313, None)).unwrap_err();
        assert!(matches!(e, AppError::Validation(ref f) if f[0].message.starts_with("relay_address:")));
        // A named relay without an address is checked once resolved, at the start.
        assert!(check_relay_address(&spec(Ipv4Addr::UNSPECIFIED, 47313, Some("relay.example.org"))).is_ok());
        assert_eq!(relay_key(&spec(Ipv4Addr::new(203, 0, 113, 7), 47313, Some("Relay.Example.org"))), ("relay.example.org".to_string(), 47313));
    }

    #[test]
    fn minimise_while_playing_is_opt_in_and_only_for_the_first_players_non_match_launches() {
        let off = config::UiPrefs::default();
        let on = config::UiPrefs { minimize_on_play: true, ..config::UiPrefs::default() };
        assert!(!follows_game(&off, Slot::First, "original"));
        assert!(follows_game(&on, Slot::First, "original") && follows_game(&on, Slot::First, "range") && follows_game(&on, Slot::First, "lan"));
        assert!(!follows_game(&on, Slot::First, "match") && !follows_game(&on, Slot::Second, "hub"));
        let mut st = GameStatus::idle();
        st.state = RunState::Exited;
        assert!(comes_back(&on, &st) && !comes_back(&off, &st));
        st.restarted = true; // the game started itself again: it is still running
        assert!(!comes_back(&on, &st));
        st.restarted = false;
        st.state = RunState::Running;
        assert!(!comes_back(&on, &st));
    }

    #[test]
    fn only_an_offline_mode_can_be_the_quick_play_default() {
        for m in ["original", "range", "hub"] {
            assert_eq!(sanitized_ui(config::UiPrefs { last_mode: m.into(), ..Default::default() }).last_mode, m);
        }
        for bad in ["lan", "match", "", "story", "../x"] {
            assert_eq!(sanitized_ui(config::UiPrefs { last_mode: bad.into(), ..Default::default() }).last_mode, "original", "{bad}");
        }
        assert_eq!(sanitized_ui(config::UiPrefs { match_players: 99, ..Default::default() }).match_players, 8);
        assert_eq!(sanitized_ui(config::UiPrefs { sound_volume: 250, ..Default::default() }).sound_volume, 100);
    }

    fn get<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
        env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn the_full_game_tile_in_both_profile_choices() {
        let s = state("full", LauncherConfig { apply_profile: true, ..Default::default() });
        // The tile is the original start-up: the title scene and nothing else (no match or LAN variable).
        let card = mode_env(&s, "original", Slot::First).unwrap();
        assert_eq!(card, vec![("SPLAT3_START".to_string(), "title".to_string())]);
        // A save exists, so the small launcher profile is accepted.
        let ud = s.data_dir.join("ud");
        std::fs::create_dir_all(ud.join("profile").join("save").join("uid")).unwrap();
        std::fs::write(ud.join("profile").join("save").join("uid").join("save.dat"), b"x").unwrap();
        std::fs::write(s.data_dir.join("profile.json"), r#"{"schema":1,"name":"Kay"}"#).unwrap();
        // (a) "use my launcher profile": the save profile and the nickname are passed.
        let mine = profile_step(&s, &ud, None);
        assert_eq!(mine.state, "passed", "{:?}", mine.detail);
        assert!(get(&mine.env, "TOTK_SAVE_PROFILE").is_some());
        let env: Vec<(String, String)> = mine.env.iter().cloned().chain(card.iter().cloned()).collect();
        assert!(env.iter().all(|(k, _)| !k.starts_with("SPLAT3_MATCH") && !k.starts_with("SPLAT3_LAN")));
        // (b) "leave my save alone": no profile variable at all.
        let none = untouched_step(true);
        assert_eq!(none.state, "skipped");
        assert!(none.env.is_empty() && none.save_exists);
        let env: Vec<(String, String)> = none.env.iter().cloned().chain(card.iter().cloned()).collect();
        assert_eq!(env, vec![("SPLAT3_START".to_string(), "title".to_string())]);
        // Everything the other modes may add is stripped from the inherited environment in both choices.
        for v in ["SPLAT3_MATCH", "SPLAT3_LAN", "SPLAT3_LAN_ROOM", "TOTK_SAVE_PROFILE"] {
            assert!(launch::OWNED_ENV.contains(&v), "{v}");
        }
        std::fs::remove_dir_all(&s.data_dir).unwrap();
    }

    #[test]
    fn each_card_sets_exactly_its_scene() {
        let s = state("cards", LauncherConfig::default());
        for (card, scene) in [("hub", "plaza"), ("range", "range"), ("original", "title")] {
            let env = mode_env(&s, card, Slot::First).unwrap();
            assert_eq!(env, vec![("SPLAT3_START".to_string(), scene.to_string())], "{card}");
        }
        // story and the earlier online card do not exist
        assert!(matches!(mode_env(&s, "story", Slot::First), Err(AppError::InvalidInput(_))));
        assert!(matches!(mode_env(&s, "online", Slot::First), Err(AppError::InvalidInput(_))));
        // the LAN card needs the feature switched on
        assert!(matches!(mode_env(&s, "lan", Slot::First), Err(AppError::FeatureDisabled(_))));
        std::fs::remove_dir_all(&s.data_dir).unwrap();
    }

    #[test]
    fn lan_card_adds_the_lan_environment_and_the_second_player_its_own_address() {
        let mut cfg = LauncherConfig { lan_enabled: true, ..Default::default() };
        cfg.lan.lan_address = "127.0.0.2/8".into();
        cfg.lan.net_log = true;
        let s = state("lan", cfg);
        let env = mode_env(&s, "lan", Slot::First).unwrap();
        assert_eq!(get(&env, "SPLAT3_START"), Some("lobby-lan"));
        assert_eq!(get(&env, "SPLAT3_LAN"), Some("lan:127.0.0.2/8"));
        assert_eq!(get(&env, "SPLAT3_NET_LOG"), Some("1"));
        let env2 = mode_env(&s, "lan", Slot::Second).unwrap();
        assert_eq!(get(&env2, "SPLAT3_LAN"), Some("lan:127.0.0.3/8"));
        // relay mode without a host is a validation error, not a launch
        s.update_config(|c| c.lan.mode = lan::LanMode::Relay).unwrap();
        assert!(matches!(mode_env(&s, "lan", Slot::First), Err(AppError::Validation(_))));
        s.update_config(|c| {
            c.lan.relay_host = "203.0.113.7".into();
            c.lan.room = "ROOM1".into();
        })
        .unwrap();
        let env = mode_env(&s, "lan", Slot::Second).unwrap();
        assert_eq!(get(&env, "SPLAT3_LAN"), Some("relay:203.0.113.7"));
        assert_eq!(get(&env, "SPLAT3_LAN_ROOM"), Some("ROOM1"));
        std::fs::remove_dir_all(&s.data_dir).unwrap();
    }


    #[test]
    fn a_launch_strips_inherited_lan_variables() {
        let root = Path::new(r"C:\g");
        let spec = launch::build_launch_spec(root, &root.join("userdata"), None, &[], &[], "x", "hub");
        for k in ["SPLAT3_LAN", "SPLAT3_LAN_ROOM", "SPLAT3_LAN_NAME", "SPLAT3_LAN_BIND", "SPLAT3_NET_LOG"] {
            assert!(spec.env_remove.iter().any(|r| r == k), "{k}");
        }
    }
    #[test]
    #[cfg(windows)]
    fn the_second_players_folder_can_never_be_the_first_ones() {
        assert!(valid_second_dir("userdata-p2").is_ok());
        for bad in ["", "userdata", "UserData", ".hidden", "a/b", r"a\b", "..", "x y", &"a".repeat(41)] {
            assert!(valid_second_dir(bad).is_err(), "{bad:?}");
        }
        let cfg = LauncherConfig { second_dir: "userdata".into(), ..Default::default() };
        assert!(second_userdata(Path::new(r"C:\g"), &cfg).is_err());
        let cfg = LauncherConfig::default();
        assert_eq!(second_userdata(Path::new(r"C:\g"), &cfg).unwrap(), Path::new(r"C:\g\userdata-p2"));
    }

    #[test]
    fn seeding_copies_caches_once_and_never_the_save() {
        let base = std::env::temp_dir().join(format!("s3l-seed-{}", util::random_hex(4)));
        let (a, b) = (base.join("userdata"), base.join("userdata-p2"));
        std::fs::create_dir_all(a.join("shader-cache").join("in")).unwrap();
        std::fs::create_dir_all(a.join("profile").join("save").join("uid")).unwrap();
        std::fs::write(a.join("profile").join("save").join("uid").join("save.dat"), b"owner").unwrap();
        std::fs::write(a.join("pipeline-cache.bin"), b"p1").unwrap();
        std::fs::write(a.join("shader-cache").join("x.spv"), b"s").unwrap();
        std::fs::write(a.join("shader-cache").join("in").join("y.bin"), b"y").unwrap();
        seed_caches(&a, &b).unwrap();
        assert_eq!(std::fs::read(b.join("pipeline-cache.bin")).unwrap(), b"p1");
        assert!(b.join("shader-cache").join("in").join("y.bin").is_file());
        assert!(!profile::save_exists(&b.join("profile"))); // the first player's save is never copied
        std::fs::write(b.join("pipeline-cache.bin"), b"own").unwrap();
        seed_caches(&a, &b).unwrap();
        assert_eq!(std::fs::read(b.join("pipeline-cache.bin")).unwrap(), b"own"); // its own cache is never overwritten
        std::fs::remove_dir_all(&base).unwrap();
    }


    #[test]
    fn match_server_setting_and_code_errors() {
        assert_eq!(parse_match_server("").unwrap(), None);
        assert_eq!(parse_match_server(" 203.0.113.7 ").unwrap(), Some(("203.0.113.7".parse().unwrap(), 47313)));
        assert_eq!(parse_match_server("203.0.113.7:7000").unwrap().unwrap().1, 7000);
        assert!(parse_match_server("relay.example.org").is_err());
        assert!(parse_match_server("203.0.113.7:0").is_err());
        assert!(parse_match_server("203.0.113.7:99999").is_err());
        let msg = |e: AppError| match e {
            AppError::Validation(v) => format!("{}={}", v[0].key, v[0].message),
            e => panic!("{e:?}"),
        };
        assert_eq!(msg(code_error(matchcode::CodeError::Checksum)), "code=checksum");
        assert_eq!(msg(code_error(matchcode::CodeError::UnsupportedVersion(3))), "code=version:3");
        // A server is required before a match can be created.
        let mut cfg = LauncherConfig::default();
        assert_eq!(msg(match_server(&cfg).unwrap_err()), "match_server=empty");
        cfg.match_server = "10.1.2.3:4000".into();
        assert_eq!(match_server(&cfg).unwrap(), ("10.1.2.3".parse().unwrap(), 4000));
    }

    #[test]
    fn a_match_launch_is_not_the_lan_card_and_strips_inherited_match_variables() {
        // The table's variables are all owned by the launcher: none can leak from the shell into another card.
        for v in automatch::MatchVars::default().names() {
            assert!(launch::OWNED_ENV.contains(&v.as_str()), "{v}");
        }
    }
    #[test]
    fn launch_record_reads_older_files() {
        let r: LaunchRecord = serde_json::from_str(r#"{"started_at":5,"mode":"solo","profile":"passed"}"#).unwrap();
        assert_eq!((r.started_at, r.mode.as_str(), r.slot.as_str()), (5, "solo", ""));
    }
}
