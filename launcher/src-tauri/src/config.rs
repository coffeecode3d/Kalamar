//! The launcher's own configuration (`config.json` in the launcher data directory).

use crate::error::{AppError, AppResult};
use crate::lan::{self, LanMode};
use crate::util;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What the LAN page remembers between launches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LanPrefs {
    pub mode: LanMode,
    pub relay_host: String,
    pub relay_port: u16,
    pub room: String,
    pub net_log: bool,
    pub bind_any: bool,
    pub lan_address: String,
}

impl Default for LanPrefs {
    fn default() -> Self {
        LanPrefs { mode: LanMode::Lan, relay_host: String::new(), relay_port: lan::DEFAULT_RELAY_PORT, room: String::new(), net_log: false, bind_any: false, lan_address: String::new() }
    }
}

/// Remembered interface choices (they used to live in the WebView's localStorage, which does not follow a change of
/// identifier or data folder).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UiPrefs {
    /// "Full game" tile: leave the save alone (no profile passed).
    pub full_untouched: bool,
    /// The Advanced drawer of the Play screen is open.
    pub adv_open: bool,
    pub match_players: u8,
    pub match_mode: String,
    /// The offline mode the main "Play" button starts: `original` (full game), `range` (training) or `hub` (free roam).
    pub last_mode: String,
    /// Minimize the launcher while the game runs and bring it back when the game ends.
    pub minimize_on_play: bool,
    /// Advanced mode: logs, paths, environment, launch reports, LAN and the other tools for tinkerers. Off by default,
    /// also for configs written before the field existed.
    pub advanced: bool,
    /// Interface sounds (short synthesised plops for a few events). On by default, also for older configs.
    pub sounds: bool,
    /// Their volume, 0 to 100 (low by default).
    pub sound_volume: u8,
    /// A Windows notification when an installation or an update ends while the launcher is in the background. On by
    /// default, also for older configs.
    pub notify: bool,
}

/// The offline modes a "Play" button (or a shortcut) can start.
pub const QUICK_MODES: [&str; 3] = ["original", "range", "hub"];

impl Default for UiPrefs {
    fn default() -> Self {
        UiPrefs { full_untouched: false, adv_open: false, match_players: 2, match_mode: "turf".into(), last_mode: "original".into(), minimize_on_play: false, advanced: false, sounds: true, sound_volume: 35, notify: true }
    }
}

/// What the install wizard remembers: the folder and the paths of the player's three files (paths only; the files stay
/// where they are), so a reinstall or a resume after a restart starts pre-filled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct InstallPrefs {
    pub dir: Option<String>,
    pub base_nsp: String,
    pub update_nsp: String,
    pub prod_keys: String,
    pub jobs: u32,
    /// A wizard run started and has not finished yet (offered again at the next start).
    pub pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LauncherConfig {
    /// Game install root (the folder that holds `build\`, `romfs\`, `userdata\`).
    pub root: Option<String>,
    /// Overrides the translator path derived from the root (`<root>\..\xlate\totk_xlate.exe`).
    pub translator: Option<String>,
    /// UI language code (one of `LANGUAGES`); empty until chosen: the interface then follows the system language.
    pub language: String,
    /// What the GAME's language follows while the settings file has no `lang`: `launcher` (the default, also for
    /// configs written before the field existed) or `windows` (see `gamelang`).
    pub game_lang_follow: String,
    /// Feature flag for the experimental LAN page.
    pub lan_enabled: bool,
    pub lan: LanPrefs,
    /// Pass the chosen profile to the game at launch (`TOTK_SAVE_PROFILE`).
    pub apply_profile: bool,
    /// `skip_first_launch`: boot straight to the hub instead of the intro, character creation and tutorial.
    pub skip_first_launch: bool,
    /// `unlock_all`: own every weapon, gear, banner, title word and emote.
    pub unlock_all: bool,
    /// `unlock_chunks`: a full stock of ability chunks (`chunks: "max"`). Off by default.
    pub unlock_chunks: bool,
    /// Path of `splat3-relay.exe` (none = looked for in a `relay\target\release` folder above the game root).
    pub relay_exe: Option<String>,
    pub relay_port: u16,
    /// Advanced "second local player" (a second instance on this PC, for tests): its nickname and the name of its
    /// user data folder, a sibling of the root's `userdata` folder.
    pub second_name: String,
    pub second_dir: String,
    /// Default match server for private matches: a relay `ip` or `ip:port` (empty until the player fills it in).
    pub match_server: String,
    pub ui: UiPrefs,
    pub install: InstallPrefs,
    /// The launcher's own updates (only used by a build that names an update repository, `update::channel`).
    pub update: crate::update::UpdatePrefs,
    /// The Discord presence (only used by a build that names a Discord application, `discord`): off by default.
    pub discord: crate::discord::DiscordPrefs,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        LauncherConfig {
            root: None,
            translator: None,
            language: String::new(),
            game_lang_follow: crate::gamelang::FOLLOW_LAUNCHER.to_string(),
            lan_enabled: false,
            lan: LanPrefs::default(),
            apply_profile: true,
            skip_first_launch: true,
            unlock_all: true,
            unlock_chunks: false,
            relay_exe: None,
            relay_port: lan::DEFAULT_RELAY_PORT,
            second_name: "Player2".into(),
            second_dir: "userdata-p2".into(),
            match_server: String::new(),
            ui: UiPrefs::default(),
            install: InstallPrefs::default(),
            update: crate::update::UpdatePrefs::default(),
            discord: crate::discord::DiscordPrefs::default(),
        }
    }
}

/// Interface languages (keep in step with src/locales/index.ts).
pub const LANGUAGES: [&str; 4] = ["en", "fr", "es", "ja"];

pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("config.json")
}

/// Loads the config. A missing file gives the defaults; a corrupt one also gives the defaults (the broken file is
/// kept as `config.json.bad` so nothing is lost) instead of failing the whole app.
pub fn load(data_dir: &Path) -> LauncherConfig {
    let path = config_path(data_dir);
    let Ok(Some(text)) = util::read_text_limited(&path, 1 << 20) else {
        return LauncherConfig::default();
    };
    match serde_json::from_str::<LauncherConfig>(&text) {
        Ok(c) => c,
        Err(_) => {
            let _ = std::fs::rename(&path, path.with_extension("json.bad"));
            LauncherConfig::default()
        }
    }
}

pub fn save(data_dir: &Path, cfg: &LauncherConfig) -> AppResult<()> {
    let text = serde_json::to_string_pretty(cfg).map_err(|e| AppError::Internal(e.to_string()))?;
    util::atomic_write(&config_path(data_dir), text.as_bytes())
}

/// The game folder named by `KALAMAR_GAME_ROOT` (or its older name `SPLAT3_GAME_ROOT`).
pub fn game_root_env() -> Option<PathBuf> {
    ["KALAMAR_GAME_ROOT", "SPLAT3_GAME_ROOT"].iter().find_map(|k| std::env::var_os(k).filter(|v| !v.is_empty())).map(PathBuf::from)
}

/// The current user's profile folder (`%USERPROFILE%` on Windows, `$HOME` on Linux).
pub fn user_profile() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn same_component(a: std::path::Component, b: std::path::Component) -> bool {
    // Windows paths compare ignoring ASCII case; Linux paths do not.
    if cfg!(windows) {
        a.as_os_str().to_string_lossy().eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    } else {
        a == b
    }
}

/// `path` is `dir` or lies under it (component by component, ignoring ASCII case, as Windows paths compare).
pub fn under(path: &Path, dir: &Path) -> bool {
    let strip = |p: &Path| PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\").to_string());
    let (path, dir) = (strip(path), strip(dir));
    let mut p = path.components();
    dir.components().all(|d| p.next().is_some_and(|x| same_component(x, d))) && dir.components().next().is_some()
}

/// The folders above `start` (itself included) that lie inside the user's profile `profile`, nearest first: a game folder
/// is looked for there only, never in a folder any account of the PC can create (walking up from
/// `C:\Users\<name>\Downloads` would otherwise reach the root of the drive). No profile known: none.
pub fn ancestors_in_profile(start: &Path, profile: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = profile else { return Vec::new() };
    start.ancestors().take_while(|d| under(d, home)).map(Path::to_path_buf).collect()
}

/// First guess of the game folder: `KALAMAR_GAME_ROOT`, else a `work\splatoon3-<version>` folder found by walking up
/// from the launcher executable and the current directory, inside the user's profile only: the current version's, else
/// the previous one's (an install not yet updated: still playable offline, and the page asks for the game update).
pub fn default_root() -> Option<PathBuf> {
    if let Some(v) = game_root_env() {
        return Some(v);
    }
    let starts = [std::env::current_exe().ok(), std::env::current_dir().ok()];
    let profile = user_profile();
    for start in starts.into_iter().flatten() {
        for dir in ancestors_in_profile(&start, profile.as_deref()) {
            for name in [crate::installer::gamever::CURRENT.game_dir, crate::installer::gamever::PREVIOUS.game_dir] {
                let cand = dir.join("work").join(name);
                if cand.is_dir() {
                    return Some(cand);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn game_folders_are_looked_for_inside_the_profile_only() {
        let home = Path::new(r"C:\Users\Someone");
        let got = ancestors_in_profile(Path::new(r"C:\Users\someone\Downloads\Kalamar.exe"), Some(home));
        assert_eq!(got, [PathBuf::from(r"C:\Users\someone\Downloads\Kalamar.exe"), PathBuf::from(r"C:\Users\someone\Downloads"), PathBuf::from(r"C:\Users\someone")]);
        assert!(!got.iter().any(|d| d == Path::new(r"C:\") || d == Path::new(r"C:\Users")), "the root of the drive is never reached");
        assert!(ancestors_in_profile(Path::new(r"D:\Kalamar\Kalamar.exe"), Some(home)).is_empty());
        assert!(ancestors_in_profile(Path::new(r"C:\Users\Someone2\x"), Some(home)).is_empty(), "another profile");
        assert!(ancestors_in_profile(Path::new(r"C:\Users\Someone\x"), None).is_empty());
        assert!(under(Path::new(r"\\?\C:\Users\Someone\a"), home) && !under(Path::new(r"C:\Users"), home));
    }

    #[test]
    #[cfg(not(windows))]
    fn game_folders_are_looked_for_inside_the_profile_only() {
        let home = Path::new("/home/someone");
        let exe = home.join("Downloads").join("Kalamar");
        let got = ancestors_in_profile(&exe, Some(home));
        assert_eq!(got, [exe.clone(), home.join("Downloads"), home.to_path_buf()]);
        assert!(!got.iter().any(|d| d == Path::new("/") || d == Path::new("/home")), "the root is never reached");
        assert!(ancestors_in_profile(Path::new("/opt/Kalamar/Kalamar"), Some(home)).is_empty());
        assert!(ancestors_in_profile(Path::new("/home/someone2/x"), Some(home)).is_empty(), "another profile");
        assert!(ancestors_in_profile(&exe, None).is_empty());
        assert!(under(&home.join("a"), home) && !under(Path::new("/home"), home));
        // Paths are case-sensitive here: another spelling is another folder.
        assert!(!under(Path::new("/home/Someone/a"), home));
    }

    #[test]
    fn quick_play_prefs_default_to_the_full_game_and_a_visible_launcher() {
        let c = load(&tmp("quick"));
        assert_eq!(c.ui.last_mode, "original");
        assert!(!c.ui.minimize_on_play);
        // a config written before these fields existed still loads, with the defaults for them
        let d = tmp("quick-old");
        util::atomic_write(&config_path(&d), br#"{"ui":{"adv_open":true,"match_players":3}}"#).unwrap();
        let old = load(&d);
        assert!(old.ui.adv_open && old.ui.match_players == 3);
        assert_eq!((old.ui.last_mode.as_str(), old.ui.minimize_on_play), ("original", false));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn the_interface_starts_simple_and_remembers_the_advanced_mode() {
        assert!(!load(&tmp("adv-new")).ui.advanced);
        // a config written before the switch existed opens in the simple mode
        let d = tmp("adv-old");
        util::atomic_write(&config_path(&d), br#"{"language":"fr","ui":{"adv_open":true,"minimize_on_play":true}}"#).unwrap();
        let old = load(&d);
        assert!(!old.ui.advanced && old.ui.minimize_on_play);
        let on = LauncherConfig { ui: UiPrefs { advanced: true, ..old.ui.clone() }, ..old };
        save(&d, &on).unwrap();
        assert!(load(&d).ui.advanced);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn interface_sounds_are_on_and_quiet_by_default_even_for_older_configs() {
        let d = tmp("sounds");
        assert!(load(&d).ui.sounds && load(&d).ui.sound_volume == 35);
        util::atomic_write(&config_path(&d), br#"{"ui":{"advanced":true,"minimize_on_play":true}}"#).unwrap();
        let old = load(&d);
        assert!(old.ui.sounds && old.ui.sound_volume == 35 && old.ui.advanced);
        let off = LauncherConfig { ui: UiPrefs { sounds: false, sound_volume: 80, ..old.ui.clone() }, ..old };
        save(&d, &off).unwrap();
        let back = load(&d);
        assert!(!back.ui.sounds && back.ui.sound_volume == 80);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn end_of_install_notifications_are_on_by_default_even_for_older_configs() {
        let d = tmp("notify");
        assert!(load(&d).ui.notify);
        util::atomic_write(&config_path(&d), br#"{"ui":{"sounds":false}}"#).unwrap();
        assert!(load(&d).ui.notify && !load(&d).ui.sounds);
        let off = LauncherConfig { ui: UiPrefs { notify: false, ..load(&d).ui }, ..load(&d) };
        save(&d, &off).unwrap();
        assert!(!load(&d).ui.notify);
        std::fs::remove_dir_all(&d).ok();
    }

    fn tmp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("s3l-cfg-{tag}-{}", util::random_hex(4)))
    }

    #[test]
    fn update_checks_default_to_automatic_and_stable_also_for_older_configs() {
        let d = tmp("upd");
        util::atomic_write(&config_path(&d), br#"{"language":"fr"}"#).unwrap();
        let c = load(&d);
        assert!(c.update.auto_check);
        assert_eq!(c.update.channel, crate::update::manifest::Channel::Stable);
        util::atomic_write(&config_path(&d), br#"{"update":{"auto_check":false,"channel":"beta"}}"#).unwrap();
        let c = load(&d);
        assert!(!c.update.auto_check);
        assert_eq!(c.update.channel, crate::update::manifest::Channel::Beta);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn the_discord_presence_is_off_by_default_also_for_older_configs() {
        assert!(!load(&tmp("dc-new")).discord.enabled);
        let d = tmp("dc-old");
        util::atomic_write(&config_path(&d), br#"{"language":"fr","ui":{"advanced":true}}"#).unwrap();
        let old = load(&d);
        assert_eq!(old.discord, crate::discord::DiscordPrefs::default());
        let on = LauncherConfig { discord: crate::discord::DiscordPrefs { enabled: true, server: true, ..Default::default() }, ..old };
        save(&d, &on).unwrap();
        let back = load(&d);
        assert!(back.discord.enabled && back.discord.server && !back.discord.mode_stage);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn missing_gives_defaults() {
        let c = load(&tmp("missing"));
        assert_eq!(c.language, "");
        assert!(!c.lan_enabled);
    }

    #[test]
    fn roundtrip_and_partial_files() {
        let d = tmp("rt");
        let c = LauncherConfig { root: Some("C:/x".into()), lan_enabled: true, ..Default::default() };
        save(&d, &c).unwrap();
        assert_eq!(load(&d), c);
        util::atomic_write(&config_path(&d), br#"{"language":"en"}"#).unwrap();
        let p = load(&d);
        assert_eq!(p.language, "en");
        assert_eq!(p.root, None);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn corrupt_file_is_set_aside() {
        let d = tmp("bad");
        util::atomic_write(&config_path(&d), b"{ not json").unwrap();
        assert_eq!(load(&d), LauncherConfig::default());
        assert!(d.join("config.json.bad").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
