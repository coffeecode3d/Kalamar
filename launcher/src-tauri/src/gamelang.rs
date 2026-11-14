//! The language of the GAME (the `lang` setting of the settings file, section `language`), which is not the launcher's.
//!
//! The runtime reads `lang` at its start (`ov_language`): an explicit code wins, an empty one means the Windows user
//! language. The launcher adds a third meaning, "like the launcher" (the default), which is a launcher preference
//! (`game_lang_follow`), never written into the settings file: when the file has no `lang`, the launch sets
//! `TOTK_LANG` to the launcher's language; with "like Windows" it removes any inherited `TOTK_LANG` instead. An
//! explicit `lang` in the file (set here, or by the player in the game's own menu or by hand) is never touched.
//!
//! Chinese and Korean need the system fonts extracted from the console firmware (`shared_fonts`): without them these
//! three languages are not offered.

use crate::ini::IniDoc;
use std::path::{Path, PathBuf};

pub const FOLLOW_LAUNCHER: &str = "launcher";
pub const FOLLOW_WINDOWS: &str = "windows";
/// The variable the runtime reads for the game's language, and the alias of the same setting.
pub const ENV_LANG: &str = "TOTK_LANG";
pub const ENV_LANG_NAMES: [&str; 2] = ["TOTK_LANG", "KLM_LANG"];

/// The language codes the launcher offers, in the order of the list (the aliases `zh-CN` and `zh-TW` of older files
/// are accepted but not offered).
pub const GAME_LANGS: &[&str] = &[
    "ja", "en-US", "en-GB", "fr", "fr-CA", "de", "it", "es", "es-419", "nl", "pt", "pt-BR", "ru", "ko", "zh-Hans", "zh-Hant",
];

/// The system font files the three CJK languages read (`runtime/ov_system.c`, `font_files`).
pub const CJK_FONT_FILES: &[&str] = &[
    "1-chinese-simplified.bfttf",
    "2-chinese-simplified-extension.bfttf",
    "3-chinese-traditional.bfttf",
    "4-korean.bfttf",
];

/// The game language that matches a launcher language ("like the launcher"); English for any other.
pub fn from_launcher(launcher: &str) -> &'static str {
    match launcher {
        "fr" => "fr",
        "es" => "es",
        "ja" => "ja",
        _ => "en-US",
    }
}

/// `zh-CN` and `zh-TW`, which older files wrote, are the same languages as `zh-Hans` and `zh-Hant`.
pub fn canonical(code: &str) -> &str {
    match code {
        "zh-CN" => "zh-Hans",
        "zh-TW" => "zh-Hant",
        c => c,
    }
}

/// Whether a game language reads the extracted system fonts.
pub fn needs_fonts(code: &str) -> bool {
    matches!(canonical(code), "zh-Hans" | "zh-Hant" | "ko")
}

/// A valid choice of the setting: "launcher", "windows" or a code of the list (or an alias).
#[cfg(test)]
fn is_choice(choice: &str) -> bool {
    choice == FOLLOW_LAUNCHER || choice == FOLLOW_WINDOWS || GAME_LANGS.contains(&canonical(choice))
}

/// The check of a `lang` written to the settings file: a language of the list (or an alias), and for Chinese and
/// Korean the extracted system fonts must be there (the game would close before its first image). `None` or an empty
/// value removes the setting and is always fine.
pub fn check_lang_change(value: Option<&str>, fonts_ok: bool) -> Result<(), String> {
    let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else { return Ok(()) };
    if !GAME_LANGS.iter().any(|l| l.eq_ignore_ascii_case(canonical(v))) {
        return Err(format!("'{v}' is not a game language"));
    }
    if needs_fonts(v) && !fonts_ok {
        return Err("needs the console's system fonts (shared-fonts), which are not in this installation".into());
    }
    Ok(())
}

/// What the launch does to the game's environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LangEnv {
    /// Set `TOTK_LANG` to this code.
    Set(&'static str),
    /// Remove an inherited `TOTK_LANG` (and its alias): the game follows Windows.
    Windows,
    /// Leave everything as it is: the settings file chooses.
    Leave,
}

/// `ini_lang` is the `lang` value of the settings file (None or empty: not chosen); `follow` the launcher preference;
/// `launcher` the launcher's effective language.
pub fn launch_env(follow: &str, launcher: &str, ini_lang: Option<&str>) -> LangEnv {
    if ini_lang.is_some_and(|v| !v.trim().is_empty()) {
        return LangEnv::Leave;
    }
    if follow == FOLLOW_WINDOWS { LangEnv::Windows } else { LangEnv::Set(from_launcher(launcher)) }
}

fn resolve(exe_dir: &Path, v: &str) -> PathBuf {
    let p = PathBuf::from(v.trim().trim_matches('"'));
    if p.is_absolute() { p } else { exe_dir.join(p) }
}

/// The folder the runtime reads the system fonts from: `TOTK_SHARED_FONTS` (or `KLM_SHARED_FONTS`), the settings
/// file's `shared_fonts`, `shared-fonts` beside the executable, else `<romfs>\..\..\firmware\shared-fonts`.
pub fn fonts_dir(root: &Path, ini: &IniDoc, env: impl Fn(&str) -> Option<String>) -> PathBuf {
    let set = env("TOTK_SHARED_FONTS")
        .or_else(|| env("KLM_SHARED_FONTS"))
        .or_else(|| ini.get("shared_fonts").map(str::to_string))
        .filter(|v| !v.trim().is_empty());
    let exe_dir = crate::install::exe_path(root).parent().map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf());
    if let Some(v) = set {
        return resolve(&exe_dir, &v);
    }
    let beside = exe_dir.join("shared-fonts");
    if beside.join("0-standard.bfttf").is_file() {
        return beside;
    }
    let romfs = ini
        .get("romfs")
        .filter(|v| !v.trim().is_empty())
        .map(|v| resolve(&exe_dir, v))
        .unwrap_or_else(|| root.join("romfs"));
    romfs.join("..").join("..").join("firmware").join("shared-fonts")
}

/// The four system fonts of the CJK languages are all present (and not empty).
pub fn fonts_present(dir: &Path) -> bool {
    CJK_FONT_FILES.iter().all(|f| std::fs::metadata(dir.join(f)).map(|m| m.is_file() && m.len() > 12).unwrap_or(false))
}

/// The first of the system's preferred languages that the launcher has (same rule as the page's `detectLang`), else
/// English.
pub fn pick_supported(preferred: &[String]) -> &'static str {
    for p in preferred {
        let low = p.to_ascii_lowercase();
        let base = low.split(['-', '_']).next().unwrap_or("");
        if let Some(l) = crate::config::LANGUAGES.iter().find(|l| **l == base) {
            return l;
        }
    }
    "en"
}

#[cfg(windows)]
pub fn system_languages() -> Vec<String> {
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let mut num = 0u32;
    let mut len = 0u32;
    // SAFETY: the first call only asks for the size of the buffer; the second fills a buffer of that size.
    unsafe {
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut num, std::ptr::null_mut(), &mut len) == 0 || len == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u16; len as usize];
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut num, buf.as_mut_ptr(), &mut len) == 0 {
            return Vec::new();
        }
        buf.split(|c| *c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
    }
}

#[cfg(not(windows))]
pub fn system_languages() -> Vec<String> {
    std::env::var("LANG").ok().into_iter().collect()
}

/// The launcher's effective language: the chosen one, else the system's.
pub fn effective_launcher_language(configured: &str) -> String {
    if crate::config::LANGUAGES.contains(&configured) {
        configured.to_string()
    } else {
        pick_supported(&system_languages()).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launcher_language_maps_to_the_game_language() {
        assert_eq!(from_launcher("fr"), "fr");
        assert_eq!(from_launcher("en"), "en-US");
        assert_eq!(from_launcher("es"), "es");
        assert_eq!(from_launcher("ja"), "ja");
        assert_eq!(from_launcher("xx"), "en-US");
        for l in crate::config::LANGUAGES {
            assert!(GAME_LANGS.contains(&from_launcher(l)), "{l}");
            assert!(!needs_fonts(from_launcher(l)), "{l}: no extracted fonts needed");
        }
    }

    #[test]
    fn like_the_launcher_is_applied_at_launch_without_overriding_an_explicit_choice() {
        assert_eq!(launch_env("launcher", "fr", None), LangEnv::Set("fr"));
        assert_eq!(launch_env("launcher", "en", Some("")), LangEnv::Set("en-US"));
        assert_eq!(launch_env("launcher", "ja", Some("  ")), LangEnv::Set("ja"));
        assert_eq!(launch_env("", "es", None), LangEnv::Set("es")); // an older config: the default
        assert_eq!(launch_env("windows", "fr", None), LangEnv::Windows);
        // a language the player set (here or in the game): left alone, whatever the preference
        assert_eq!(launch_env("launcher", "fr", Some("de")), LangEnv::Leave);
        assert_eq!(launch_env("windows", "fr", Some("ko")), LangEnv::Leave);
        assert_eq!(launch_env("launcher", "ja", Some("zh-CN")), LangEnv::Leave);
    }

    #[test]
    fn a_language_set_in_the_settings_file_is_read_back_and_kept() {
        let d = std::env::temp_dir().join(format!("s3l-lang-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        let ini = d.join("kalamar.ini");
        assert_eq!(crate::settings::read_value(&ini, "lang"), None); // no file
        std::fs::write(&ini, "[display]
fullscreen = 1
[language]
menu_lang = fr
").unwrap();
        let none = crate::settings::read_value(&ini, "lang");
        assert_eq!(launch_env("launcher", "ja", none.as_deref()), LangEnv::Set("ja"));
        // the player picks a language in the game's own settings: the launcher no longer sets one
        std::fs::write(&ini, "[language]
menu_lang = fr
lang = pt-BR
").unwrap();
        let some = crate::settings::read_value(&ini, "lang");
        assert_eq!(some.as_deref(), Some("pt-BR"));
        assert_eq!(launch_env("launcher", "ja", some.as_deref()), LangEnv::Leave);
        // an empty value (the player cleared it): back to following
        std::fs::write(&ini, "[language]
lang =
").unwrap();
        assert_eq!(launch_env("windows", "ja", crate::settings::read_value(&ini, "lang").as_deref()), LangEnv::Windows);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_language_needing_missing_fonts_is_refused_when_written() {
        assert!(check_lang_change(Some("ko"), false).is_err());
        assert!(check_lang_change(Some("zh-TW"), false).is_err());
        assert!(check_lang_change(Some("zh-Hans"), true).is_ok());
        assert!(check_lang_change(Some("fr"), false).is_ok());
        assert!(check_lang_change(Some("pt-br"), false).is_ok());
        assert!(check_lang_change(Some("klingon"), true).is_err());
        assert!(check_lang_change(None, false).is_ok());
        assert!(check_lang_change(Some(""), false).is_ok());
    }

    #[test]
    fn the_choices_and_the_fonts_rule() {
        assert!(is_choice("launcher") && is_choice("windows") && is_choice("pt-BR") && is_choice("zh-CN"));
        assert!(!is_choice("") && !is_choice("klingon") && !is_choice("en"));
        assert!(needs_fonts("ko") && needs_fonts("zh-Hans") && needs_fonts("zh-Hant") && needs_fonts("zh-CN") && needs_fonts("zh-TW"));
        assert!(!needs_fonts("ja") && !needs_fonts("en-US") && !needs_fonts("pt-BR"));
        // every offered code is a code of the settings schema
        let schema = crate::settings::Schema::embedded();
        let lang = schema.fields.iter().find(|f| f.key == "lang").expect("lang is in the schema");
        for c in GAME_LANGS {
            assert!(lang.options.iter().any(|o| o == c), "{c}");
        }
        assert_eq!(canonical("zh-TW"), "zh-Hant");
    }

    #[test]
    fn system_language_picks_the_first_supported() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_supported(&l(&["ja-JP", "en-US"])), "ja");
        assert_eq!(pick_supported(&l(&["de-DE", "es-MX", "en"])), "es");
        assert_eq!(pick_supported(&l(&["de-DE"])), "en");
        assert_eq!(pick_supported(&[]), "en");
        assert_eq!(effective_launcher_language("fr"), "fr");
    }

    #[test]
    fn fonts_are_found_where_the_runtime_looks() {
        let d = std::env::temp_dir().join(format!("s3l-fonts-{}", crate::util::random_hex(4)));
        let root = d.join("w");
        let exe_dir = crate::install::exe_path(&root).parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&exe_dir).unwrap();
        let no_env = |_: &str| None;
        let ini = IniDoc::empty();
        // nothing: the firmware folder two levels above the romfs folder
        let dir = fonts_dir(&root, &ini, no_env);
        assert!(dir.ends_with("firmware/shared-fonts") || dir.ends_with(r"firmware\shared-fonts"));
        assert!(!fonts_present(&dir));
        // shared-fonts beside the executable, announced by its standard font
        let beside = exe_dir.join("shared-fonts");
        std::fs::create_dir_all(&beside).unwrap();
        std::fs::write(beside.join("0-standard.bfttf"), vec![1u8; 64]).unwrap();
        assert_eq!(fonts_dir(&root, &ini, no_env), beside);
        assert!(!fonts_present(&beside)); // only the standard one
        for f in CJK_FONT_FILES {
            std::fs::write(beside.join(f), vec![1u8; 64]).unwrap();
        }
        assert!(fonts_present(&beside));
        std::fs::write(beside.join("4-korean.bfttf"), b"").unwrap();
        assert!(!fonts_present(&beside)); // an empty file does not count
        // the settings file and the environment name a folder
        let other = d.join("fonts");
        let ini2 = IniDoc::parse(&format!("[paths]\nshared_fonts = {}\n", other.display()));
        assert_eq!(fonts_dir(&root, &ini2, no_env), other);
        let from_env = |k: &str| (k == "TOTK_SHARED_FONTS").then(|| std::env::temp_dir().join("fonts-env").display().to_string());
        assert_eq!(fonts_dir(&root, &ini2, from_env), std::env::temp_dir().join("fonts-env"));
        std::fs::remove_dir_all(&d).ok();
    }
}
