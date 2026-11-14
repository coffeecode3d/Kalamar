//! Detection of the game install under a root folder. Layout:
//!
//! ```text
//! <root>\build\splat3.exe          the recompiled game
//! <root>\build\totk.ini            optional runtime settings (next to the exe)
//! <root>\romfs\romfs.bin + index.json   the RomFS image
//! <root>\userdata\                 working dir: logs\, profile\ (saves), caches
//! <root>\..\xlate\totk_xlate.exe   the shader translator (TOTK_XLATE)
//! ```

use crate::util;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const EXE_REL: [&str; 2] = ["build", "splat3"];
pub const ROMFS_BIN_REL: [&str; 2] = ["romfs", "romfs.bin"];
pub const ROMFS_INDEX_REL: [&str; 2] = ["romfs", "index.json"];

pub fn exe_path(root: &Path) -> PathBuf {
    root.join(EXE_REL[0]).join(crate::installer::exe_name(EXE_REL[1]))
}
/// The runtime's settings file, in the game's build folder. The runtime reads `kalamar.ini` first, falls back to
/// `totk.ini` when only the old one exists, and writes back to whichever it read (it never deletes the old file); the
/// launcher follows the same rule here, and only here. With neither file, the new name is used.
pub fn ini_path(root: &Path) -> PathBuf {
    let build = root.join("build");
    let (new, old) = (build.join("kalamar.ini"), build.join("totk.ini"));
    if !new.exists() && old.exists() { old } else { new }
}
pub fn userdata_dir(root: &Path) -> PathBuf {
    root.join("userdata")
}
pub fn logs_dir(root: &Path) -> PathBuf {
    userdata_dir(root).join("logs")
}
pub fn save_dir(root: &Path) -> PathBuf {
    userdata_dir(root).join("profile").join("save")
}
/// Default translator location: `<parent of root>\xlate\totk_xlate.exe`, unless overridden.
pub fn translator_path(root: &Path, override_path: Option<&Path>) -> PathBuf {
    match override_path {
        Some(p) => p.to_path_buf(),
        None => root.parent().unwrap_or(root).join("xlate").join(crate::installer::exe_name(crate::installer::XLATE_STEM)),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Check {
    /// Stable id the UI translates: `exe`, `romfs`, `translator`, `save`, `settings`.
    pub id: &'static str,
    pub path: String,
    pub present: bool,
    pub required: bool,
    pub size: Option<u64>,
    pub modified: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LastRunLog {
    pub path: String,
    pub name: String,
    pub modified: Option<u64>,
    pub size: u64,
    /// `fatal` (a line says so), `completed` (the runtime's exit line is there), `unknown`, `empty`.
    pub status: &'static str,
    pub problem_lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallStatus {
    pub root: Option<String>,
    pub root_exists: bool,
    pub checks: Vec<Check>,
    /// Ids of required checks that failed.
    pub missing: Vec<&'static str>,
    pub ready: bool,
    pub build_date: Option<u64>,
    pub userdata: Option<String>,
    pub logs_dir: Option<String>,
    pub last_log: Option<LastRunLog>,
}

fn check(id: &'static str, path: PathBuf, required: bool, dir_with_entries: bool) -> Check {
    let meta = std::fs::metadata(&path).ok();
    let present = match &meta {
        None => false,
        Some(m) if dir_with_entries => m.is_dir() && std::fs::read_dir(&path).map(|mut r| r.next().is_some()).unwrap_or(false),
        Some(m) => m.is_file(),
    };
    Check {
        id,
        size: meta.as_ref().filter(|m| m.is_file()).map(|m| m.len()),
        modified: if present { util::mtime_secs(&path) } else { None },
        path: path.display().to_string(),
        present,
        required,
    }
}

/// Full detection, with the last run's log summary (Home page).
pub fn detect(root: Option<&Path>, translator_override: Option<&Path>) -> InstallStatus {
    detect_with(root, translator_override, true)
}

/// What is needed to enable "Play": a handful of `stat` calls on small paths (the exe, the RomFS image's size, the
/// index, the translator, the save folder and the settings file) and nothing else. In particular the 9.6 GB RomFS
/// image is never opened or read here, and the logs folder is not listed (`last_log` stays empty: Home asks for the
/// full status).
pub fn detect_light(root: Option<&Path>, translator_override: Option<&Path>) -> InstallStatus {
    detect_with(root, translator_override, false)
}

fn detect_with(root: Option<&Path>, translator_override: Option<&Path>, with_log: bool) -> InstallStatus {
    let Some(root) = root else {
        return InstallStatus {
            root: None,
            root_exists: false,
            checks: Vec::new(),
            missing: vec!["root"],
            ready: false,
            build_date: None,
            userdata: None,
            logs_dir: None,
            last_log: None,
        };
    };
    let root_exists = root.is_dir();
    let exe = check("exe", exe_path(root), true, false);
    let bin = check("romfs", root.join(ROMFS_BIN_REL[0]).join(ROMFS_BIN_REL[1]), true, false);
    let idx_ok = root.join(ROMFS_INDEX_REL[0]).join(ROMFS_INDEX_REL[1]).is_file();
    let mut romfs = bin;
    romfs.present = romfs.present && idx_ok;
    let translator = check("translator", translator_path(root, translator_override), true, false);
    let save = check("save", save_dir(root), false, true);
    let settings = check("settings", ini_path(root), false, false);
    let checks = vec![exe, romfs, translator, save, settings];
    let missing: Vec<&'static str> = checks.iter().filter(|c| c.required && !c.present).map(|c| c.id).collect();
    let mut missing_all = missing;
    if !root_exists {
        missing_all.insert(0, "root");
    }
    let build_date = checks[0].modified;
    InstallStatus {
        root: Some(root.display().to_string()),
        root_exists,
        ready: root_exists && missing_all.is_empty(),
        missing: missing_all,
        build_date,
        userdata: Some(userdata_dir(root).display().to_string()),
        logs_dir: Some(logs_dir(root).display().to_string()),
        last_log: if with_log { latest_log(root) } else { None },
        checks,
    }
}

/// Newest `run-*.log` (the runtime's stderr; the `.log.out` companion is stdout).
pub fn latest_log_path(root: &Path) -> Option<PathBuf> {
    let rd = std::fs::read_dir(logs_dir(root)).ok()?;
    rd.filter_map(|e| e.ok())
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.starts_with("run-") && n.ends_with(".log")
        })
        .max_by_key(|e| (util::mtime_secs(&e.path()).unwrap_or(0), e.file_name()))
        .map(|e| e.path())
}

pub fn latest_log(root: &Path) -> Option<LastRunLog> {
    let path = latest_log_path(root)?;
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let tail = util::tail_lines(&path, 400).ok().flatten().unwrap_or_default();
    let (status, problem_lines) = classify_log(&tail, size);
    Some(LastRunLog {
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        modified: util::mtime_secs(&path),
        path: path.display().to_string(),
        size,
        status,
        problem_lines,
    })
}

/// Heuristic over the end of a runtime log: a line with "fatal" or "crash" means a bad end, the runtime's
/// `frametime: exit` summary line means a normal one.
pub fn classify_log(tail: &str, size: u64) -> (&'static str, Vec<String>) {
    if size == 0 {
        return ("empty", Vec::new());
    }
    let problems: Vec<String> = tail
        .lines()
        .filter(|l| {
            let l = l.to_ascii_lowercase();
            l.contains("fatal") || l.contains("crash") || l.contains("exception") || l.contains("abort")
        })
        .map(|l| l.chars().take(300).collect())
        .collect();
    let keep: Vec<String> = problems.iter().rev().take(5).rev().cloned().collect();
    if !keep.is_empty() {
        ("fatal", keep)
    } else if tail.contains("frametime: exit") {
        ("completed", Vec::new())
    } else {
        ("unknown", Vec::new())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_settings_file_follows_the_runtimes_rule() {
        use super::*;
        let d = std::env::temp_dir().join(format!("s3l-ini-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(d.join("build")).unwrap();
        // neither: the new name
        assert_eq!(ini_path(&d), d.join("build").join("kalamar.ini"));
        // only the old one: it is used (and stays the one written back to)
        std::fs::write(d.join("build").join("totk.ini"), "[display]\n").unwrap();
        assert_eq!(ini_path(&d), d.join("build").join("totk.ini"));
        // both: the new one wins; the old file is left alone
        std::fs::write(d.join("build").join("kalamar.ini"), "[display]\n").unwrap();
        assert_eq!(ini_path(&d), d.join("build").join("kalamar.ini"));
        assert!(d.join("build").join("totk.ini").is_file());
        std::fs::remove_dir_all(&d).unwrap();
    }

    use super::*;

    fn fixture(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("s3l-inst-{tag}-{}", util::random_hex(4)));
        std::fs::create_dir_all(base.join("root")).unwrap();
        base
    }
    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn no_root_configured() {
        let s = detect(None, None);
        assert!(!s.ready);
        assert_eq!(s.missing, vec!["root"]);
    }

    #[test]
    fn missing_everything_is_reported_not_panicked() {
        let base = fixture("empty");
        let s = detect(Some(&base.join("root")), None);
        assert!(!s.ready);
        assert_eq!(s.missing, vec!["exe", "romfs", "translator"]);
        assert!(s.last_log.is_none());
        let s2 = detect(Some(&base.join("nonexistent")), None);
        assert_eq!(s2.missing[0], "root");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn full_install_is_ready() {
        let base = fixture("full");
        let root = base.join("root");
        touch(&exe_path(&root));
        touch(&root.join("romfs").join("romfs.bin"));
        touch(&root.join("romfs").join("index.json"));
        touch(&base.join("xlate").join(crate::installer::exe_name(crate::installer::XLATE_STEM)));
        let s = detect(Some(&root), None);
        assert!(s.ready, "{:?}", s.missing);
        assert!(s.build_date.is_some());
        assert!(!s.checks.iter().find(|c| c.id == "save").unwrap().present);
        touch(&save_dir(&root).join("abc").join("data"));
        assert!(detect(Some(&root), None).checks.iter().find(|c| c.id == "save").unwrap().present);
        // romfs without its index is not complete
        std::fs::remove_file(root.join("romfs").join("index.json")).unwrap();
        assert_eq!(detect(Some(&root), None).missing, vec!["romfs"]);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn light_detection_agrees_with_the_full_one_and_skips_the_logs() {
        let base = fixture("light");
        let root = base.join("root");
        touch(&exe_path(&root));
        touch(&root.join("romfs").join("romfs.bin"));
        touch(&root.join("romfs").join("index.json"));
        touch(&base.join("xlate").join(crate::installer::exe_name(crate::installer::XLATE_STEM)));
        touch(&logs_dir(&root).join("run-20260101-000000.log"));
        let (full, light) = (detect(Some(&root), None), detect_light(Some(&root), None));
        assert!(full.last_log.is_some() && light.last_log.is_none());
        assert_eq!((full.ready, &full.missing, &full.checks, full.build_date), (light.ready, &light.missing, &light.checks, light.build_date));
        assert!(light.ready);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn translator_override_wins() {
        let r = Path::new("C:/w/root");
        assert_eq!(translator_path(r, None), Path::new("C:/w").join("xlate").join(crate::installer::exe_name(crate::installer::XLATE_STEM)));
        assert_eq!(translator_path(r, Some(Path::new("D:/t.exe"))), Path::new("D:/t.exe"));
    }

    #[test]
    fn log_classification() {
        assert_eq!(classify_log("", 0).0, "empty");
        assert_eq!(classify_log("[host] ok\n[host] frametime: exit: 112 frames", 50).0, "completed");
        let (s, p) = classify_log("[host] a\n[host] rt_fatal: boom\n", 30);
        assert_eq!(s, "fatal");
        assert_eq!(p.len(), 1);
        assert_eq!(classify_log("[host] hi", 9).0, "unknown");
    }
}
