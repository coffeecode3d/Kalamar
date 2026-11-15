//! The one place that names the game version this launcher builds, and the one it built before.
//!
//! The launcher supports ONE version of the game at a time. When the game is updated, this file is what changes (the
//! constants of [`CURRENT`], with `recomp/targets.py` of the engine's source), plus the version text of the four
//! interface languages (`src/locales/*/install.ts`, checked by `locale_versions_are_the_current_or_the_previous_one`).
//! Everything else (the wizard, the file checks, the build label, the Play page) reads it from here.
//!
//! [`PREVIOUS`] exists only to recognize what a player still has: an install built for it is offered the update to
//! [`CURRENT`], and its update file is named for what it is when the wizard is given it. Nothing builds it any more.

use std::path::{Path, PathBuf};

/// Facts about the version the launcher builds. Sizes, versions and file names that identify the player's own copy
/// (the same kind of facts the file checks have always used); nothing in them comes from the game's contents.
pub struct GameVersion {
    /// As the player knows it (`11.3.0`).
    pub display: &'static str,
    /// The game's folder under `<install>\work`.
    pub game_dir: &'static str,
    /// The engine's build target (`recomp/targets.py`).
    pub target: &'static str,
    /// The update's content-meta version.
    pub update_version: u64,
    /// The file name of the update's Program NCA (the NCA id is a hash of its content).
    pub update_program_nca: &'static str,
    /// Size of the merged RomFS image (the merge step's progress and the space it needs).
    pub romfs_size: u64,
}

/// The version the launcher builds and runs.
pub const CURRENT: GameVersion = GameVersion {
    display: "11.3.0",
    game_dir: "splatoon3-11.3.0",
    target: "splatoon3-v1130",
    update_version: 2_752_512,
    update_program_nca: "b17c38424e33cec90d4407ef33c7848d.nca",
    romfs_size: 10_059_882_848,
};

/// The version the launcher built before [`CURRENT`]: what an older install still holds.
pub struct PreviousVersion {
    pub display: &'static str,
    pub game_dir: &'static str,
    pub update_version: u64,
    pub update_program_nca: &'static str,
}

pub const PREVIOUS: PreviousVersion =
    PreviousVersion { display: "11.2.0", game_dir: "splatoon3-11.2.0", update_version: 2_686_976, update_program_nca: "aada92e014fb1a953bbb735fceb491a9.nca" };

/// Folder names of games are `splatoon3-<version>`.
const DIR_PREFIX: &str = "splatoon3-";

/// The version a game folder is for, from its name (`...\work\splatoon3-11.2.0` is `11.2.0`). `None` for any other
/// name (a folder somebody named by hand: nothing is claimed about it).
pub fn folder_version(root: &Path) -> Option<String> {
    let name = root.file_name()?.to_str()?;
    let v = name.strip_prefix(DIR_PREFIX)?;
    let ok = v.split('.').count() == 3 && v.split('.').all(|p| !p.is_empty() && p.len() <= 4 && p.bytes().all(|b| b.is_ascii_digit()));
    ok.then(|| v.to_string())
}

/// The game in `root` was built for another version than [`CURRENT`] (by its folder name, the only record it has).
/// Online play is not offered with it, and the Play page asks for the update. A folder with another kind of name is
/// taken as current: it is the player's own choice.
pub fn is_outdated_root(root: &Path) -> bool {
    folder_version(root).is_some_and(|v| v != CURRENT.display)
}

/// The previous version's game folder of an install, when it is there as a real folder (a link is never one).
pub fn previous_game(install: &Path) -> Option<PathBuf> {
    let p = install.join("work").join(PREVIOUS.game_dir);
    let m = std::fs::symlink_metadata(&p).ok()?;
    (m.is_dir() && !is_link(&m)).then_some(p)
}

/// A symbolic link, a junction or any other reparse point: something that may lead out of the folder it is in.
#[cfg(windows)]
pub fn is_link(m: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    m.file_type().is_symlink() || m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub fn is_link(m: &std::fs::Metadata) -> bool {
    m.file_type().is_symlink()
}

/// What the game derives from the code and shaders it runs (translated shaders, recorded pipelines): not the player's,
/// rebuilt by playing, and not carried to a game built from another `main`, whose use of them cannot be assumed.
pub const DERIVED_CACHES: [&str; 7] =
    ["shader-cache", "pipeline-cache.bin", "pipeline-keys.bin", "pipeline-use.bin", "shader-answers.bin", "shader-programs.bin", "shader-prepared.bin"];

/// Copies what the player made in the old game folder (`userdata*`: saves, profile, logs, the second player's folder)
/// into the new one, never overwriting a file the new one already has and never following a link; the derived caches
/// ([`DERIVED_CACHES`]) stay behind. Returns the number of files copied and the links skipped.
pub fn carry_user_data(from_game: &Path, to_game: &Path) -> std::io::Result<(u64, u64)> {
    let (mut files, mut skipped) = (0u64, 0u64);
    for e in std::fs::read_dir(from_game)?.flatten() {
        let name = e.file_name();
        if !name.to_string_lossy().to_ascii_lowercase().starts_with("userdata") {
            continue;
        }
        let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
        if is_link(&m) || !m.is_dir() {
            skipped += 1;
            continue;
        }
        copy_tree(&e.path(), &to_game.join(&name), true, &mut files, &mut skipped)?;
    }
    Ok((files, skipped))
}

fn copy_tree(from: &Path, to: &Path, top: bool, files: &mut u64, skipped: &mut u64) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        if top && DERIVED_CACHES.iter().any(|c| e.file_name().to_string_lossy().eq_ignore_ascii_case(c)) {
            continue;
        }
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let Ok(m) = std::fs::symlink_metadata(&src) else { continue };
        if is_link(&m) {
            *skipped += 1;
        } else if m.is_dir() {
            copy_tree(&src, &dst, false, files, skipped)?;
        } else if m.is_file() && !dst.exists() {
            std::fs::copy(&src, &dst)?;
            *files += 1;
        }
    }
    Ok(())
}

/// Removes a folder tree without ever following a link: a link inside is removed itself (its target is left as it
/// is), and anything that is not a plain file or folder is removed as a file. Read-only files are made writable.
pub fn remove_tree_no_follow(dir: &Path) -> std::io::Result<u64> {
    let mut removed = 0u64;
    for e in std::fs::read_dir(dir)?.flatten() {
        let p = e.path();
        let m = std::fs::symlink_metadata(&p)?;
        if is_link(&m) {
            // A directory link (junction) goes with `remove_dir`, a file link with `remove_file`; neither touches the target.
            if std::fs::remove_dir(&p).is_err() {
                std::fs::remove_file(&p)?;
            }
            removed += 1;
        } else if m.is_dir() {
            removed += remove_tree_no_follow(&p)?;
        } else {
            let mut perm = m.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perm.set_readonly(false);
            let _ = std::fs::set_permissions(&p, perm);
            std::fs::remove_file(&p)?;
            removed += 1;
        }
    }
    std::fs::remove_dir(dir)?;
    Ok(removed + 1)
}

/// Removes the previous version's game folder of `install` once the current one is installed and checked. Refuses
/// unless the folder is a real folder inside the install folder (a link, or one that resolves elsewhere, is left
/// alone). `Ok(None)`: nothing to remove.
pub fn remove_previous_game(install: &Path) -> std::io::Result<Option<u64>> {
    let Some(old) = previous_game(install) else { return Ok(None) };
    let (Ok(real_old), Ok(real_install)) = (std::fs::canonicalize(&old), std::fs::canonicalize(install)) else {
        return Err(std::io::Error::other("the old game folder cannot be resolved"));
    };
    if !real_old.starts_with(&real_install) || real_old.file_name() != old.file_name() {
        return Err(std::io::Error::other("the old game folder is not inside the install folder"));
    }
    remove_tree_no_follow(&old).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("s3l-gamever-{tag}-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// A directory junction `link` to `target` (Windows): what a player or a tool can put inside a folder.
    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) -> bool {
        std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(link).arg(target).output().is_ok_and(|o| o.status.success())
    }

    #[test]
    fn the_constants_agree_with_each_other() {
        assert_eq!(CURRENT.game_dir, format!("splatoon3-{}", CURRENT.display));
        assert_eq!(PREVIOUS.game_dir, format!("splatoon3-{}", PREVIOUS.display));
        assert_ne!(CURRENT.update_version, PREVIOUS.update_version);
        assert_ne!(CURRENT.update_program_nca, PREVIOUS.update_program_nca);
        assert_ne!(CURRENT.game_dir, PREVIOUS.game_dir);
        assert_eq!(CURRENT.target, format!("splatoon3-v{}", CURRENT.display.replace('.', "")));
    }

    #[test]
    fn a_folder_name_tells_the_game_version() {
        assert_eq!(folder_version(Path::new("C:/K/work/splatoon3-11.2.0")).as_deref(), Some("11.2.0"));
        assert_eq!(folder_version(Path::new("C:/K/work/splatoon3-11.3.0/")).as_deref(), Some("11.3.0"));
        for odd in [r"C:\K\work\game", r"C:\K\work\splatoon3-", r"C:\K\work\splatoon3-11.3", r"C:\K\work\splatoon3-11.3.x", r"C:\K\work\splatoon3-1.2.3.4"] {
            assert_eq!(folder_version(Path::new(odd)), None, "{odd}");
        }
        assert!(is_outdated_root(Path::new(&format!("C:/K/work/{}", PREVIOUS.game_dir))));
        assert!(!is_outdated_root(Path::new(&format!("C:/K/work/{}", CURRENT.game_dir))));
        assert!(!is_outdated_root(Path::new("D:/my game")), "a folder named by hand is the player's own choice");
    }

    #[test]
    fn locale_versions_are_the_current_or_the_previous_one() {
        // Every 11.y.z in the install texts (and the refusals that name the game) is a version this file names: a new
        // game version is changed here and found there.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("src").join("locales");
        let mut seen = 0;
        for lang in ["en", "fr", "es", "ja"] {
            for file in ["install.ts", "app.ts", "community.ts"] {
                let Ok(text) = std::fs::read_to_string(dir.join(lang).join(file)) else { continue };
                let b = text.as_bytes();
                let mut i = 0;
                while i < b.len() {
                    if b[i].is_ascii_digit() && (i == 0 || !(b[i - 1].is_ascii_digit() || b[i - 1] == b'.')) {
                        let mut j = i;
                        while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
                            j += 1;
                        }
                        let tok = text[i..j].trim_end_matches('.');
                        if tok.starts_with("11.") && tok.split('.').count() == 3 && tok.split('.').all(|p| !p.is_empty() && p.len() <= 2) {
                            assert!(tok == CURRENT.display || tok == PREVIOUS.display, "{lang}/{file}: version {tok} is neither {} nor {}", CURRENT.display, PREVIOUS.display);
                            seen += 1;
                        }
                        i = j;
                    } else {
                        i += 1;
                    }
                }
            }
        }
        assert!(seen >= 8, "the locale files were not read ({seen} versions found)");
    }

    #[test]
    fn the_old_game_is_found_only_as_a_real_folder() {
        let i = tmp("find");
        assert_eq!(previous_game(&i), None);
        touch(&i.join("work").join(PREVIOUS.game_dir).join("exefs").join("main"), "x");
        assert_eq!(previous_game(&i), Some(i.join("work").join(PREVIOUS.game_dir)));
        std::fs::remove_dir_all(&i).unwrap();
    }

    #[test]
    fn user_data_is_copied_and_nothing_the_new_game_has_is_overwritten() {
        let i = tmp("carry");
        let (old, new) = (i.join("old"), i.join("new"));
        touch(&old.join("userdata").join("profile").join("save").join("save.dat"), "SAVE-OLD");
        touch(&old.join("userdata").join("logs").join("a.log"), "log");
        touch(&old.join("userdata").join("shader-cache").join("0001.bin"), "derived");
        touch(&old.join("userdata").join("pipeline-keys.bin"), "derived");
        touch(&old.join("userdata").join("profile").join("shader-cache"), "a save-side file of that name is the player's");
        touch(&old.join("userdata-p2").join("profile").join("save").join("save.dat"), "SAVE-P2");
        touch(&old.join("build").join(crate::installer::exe_name(crate::installer::GAME_STEM)), "old exe");
        touch(&new.join("userdata").join("logs").join("a.log"), "newer log");
        let (n, links) = carry_user_data(&old, &new).unwrap();
        assert_eq!((n, links), (3, 0));
        assert_eq!(std::fs::read_to_string(new.join("userdata/profile/save/save.dat")).unwrap(), "SAVE-OLD");
        assert_eq!(std::fs::read_to_string(new.join("userdata-p2/profile/save/save.dat")).unwrap(), "SAVE-P2");
        assert_eq!(std::fs::read_to_string(new.join("userdata/logs/a.log")).unwrap(), "newer log");
        assert!(!new.join("userdata/shader-cache").exists() && !new.join("userdata/pipeline-keys.bin").exists(), "derived caches are rebuilt by playing");
        assert!(!new.join("build").exists(), "only the player's data is carried");
        std::fs::remove_dir_all(&i).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_inside_user_data_is_not_followed_when_carrying() {
        let i = tmp("carrylink");
        let (old, new, outside) = (i.join("old"), i.join("new"), i.join("outside"));
        touch(&outside.join("secret.txt"), "not yours to copy");
        touch(&old.join("userdata").join("profile").join("save").join("save.dat"), "SAVE");
        if !junction(&old.join("userdata").join("link"), &outside) {
            std::fs::remove_dir_all(&i).unwrap();
            return; // the PC does not let this account make junctions
        }
        let (n, links) = carry_user_data(&old, &new).unwrap();
        assert_eq!((n, links), (1, 1));
        assert!(!new.join("userdata").join("link").exists());
        std::fs::remove_dir_all(old.join("userdata").join("link")).unwrap();
        std::fs::remove_dir_all(&i).unwrap();
    }

    #[test]
    fn removing_the_old_game_removes_the_folder_and_only_it() {
        let i = tmp("remove");
        let old = i.join("work").join(PREVIOUS.game_dir);
        touch(&old.join("romfs").join("romfs.bin"), "big");
        touch(&old.join("userdata").join("profile").join("save").join("save.dat"), "SAVE");
        touch(&i.join("work").join(CURRENT.game_dir).join("build").join(crate::installer::exe_name(crate::installer::GAME_STEM)), "new exe");
        touch(&i.join("work").join("xlate").join("totk_xlate.exe"), "x");
        std::fs::set_permissions(old.join("romfs").join("romfs.bin"), {
            let mut p = std::fs::metadata(old.join("romfs").join("romfs.bin")).unwrap().permissions();
            p.set_readonly(true);
            p
        })
        .unwrap();
        assert!(remove_previous_game(&i).unwrap().is_some());
        assert!(!old.exists());
        assert!(i.join("work").join(CURRENT.game_dir).join("build").join(crate::installer::exe_name(crate::installer::GAME_STEM)).is_file());
        assert!(i.join("work").join("xlate").join("totk_xlate.exe").is_file());
        assert_eq!(remove_previous_game(&i).unwrap(), None, "nothing left to remove");
        std::fs::remove_dir_all(&i).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_is_never_followed_out_of_the_install_folder() {
        let i = tmp("junction");
        let outside = tmp("outside");
        touch(&outside.join("precious.txt"), "keep me");
        let old = i.join("work").join(PREVIOUS.game_dir);
        touch(&old.join("exefs").join("main"), "x");
        // 1. a junction inside the old folder: it is removed, its target stays whole
        if !junction(&old.join("shared"), &outside) {
            std::fs::remove_dir_all(&i).unwrap();
            std::fs::remove_dir_all(&outside).unwrap();
            return;
        }
        assert!(remove_previous_game(&i).unwrap().is_some());
        assert!(!old.exists());
        assert_eq!(std::fs::read_to_string(outside.join("precious.txt")).unwrap(), "keep me");
        // 2. the old folder itself is a junction to somewhere else: nothing is removed
        assert!(junction(&old, &outside));
        assert_eq!(previous_game(&i), None);
        assert_eq!(remove_previous_game(&i).unwrap(), None);
        assert_eq!(std::fs::read_to_string(outside.join("precious.txt")).unwrap(), "keep me");
        std::fs::remove_dir(&old).unwrap();
        std::fs::remove_dir_all(&i).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }
}
