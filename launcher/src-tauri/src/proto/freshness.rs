//! Is the installed game recent enough for the automatic match, and is it the build of the runtime this launcher
//! carries? Two questions, both answered from what the launcher already has:
//!
//! * **Too old** (blocking): the engine does not know `--list-match-options` (its listing failed the way an engine
//!   without the command does: usage text, no document of the engine's format). That game cannot be driven to join or
//!   host a session by itself: it starts, connects to the relay and does nothing. The page refuses "ready", "start"
//!   and "accept" for it and points to the recompilation; the runner also refuses the match.
//! * **Recompilation available** (a warning): the game was compiled from another runtime commit than the one embedded
//!   in this launcher (the bundled `runtime-src.json`, from `resources\` or the copy embedded in the executable, against the compile step's record in the install state file), or
//!   its listing is older than the contract-2 one.
//!
//! * **Another game version** (blocking, the same state as "too old"): the installed game is the previous version
//!   (`installer::gamever`), built before the game was updated. It still starts for offline play, but its build differs
//!   from every current one: it is not declared to a server, no match is accepted with it, and the page asks for the
//!   game update. `old_version` names it.
//!
//! A listing that failed for another reason (no answer in time, the executable cannot be run, no executable) proves
//! nothing about the build: the level is `unknown` and nothing is blocked.

use crate::proto::engine::EngineInfo;
use serde::Serialize;
use std::path::Path;

/// The failure notes of `engine::run_listing` / `parse_options` that mean "this engine has no such command".
pub fn lacks_listing(note: &str) -> bool {
    note.starts_with("the engine has no --list-match-options")
        || note.starts_with("not a JSON list")
        || note.starts_with("not a splat3-match-options")
        || note.starts_with("not JSON")
        || note.starts_with("unknown version of the list")
}

/// The engine's listing is known to be missing: its game is too old for the automatic match.
pub fn engine_too_old(info: Option<&EngineInfo>) -> bool {
    info.is_some_and(|i| i.options.is_none() && i.note.as_deref().is_some_and(lacks_listing))
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GameCheck {
    /// `ok`, `too_old` (blocking), `unknown` (nothing proven: not blocking), `missing` (no game installed).
    pub level: &'static str,
    /// A recompilation is available (warning, not blocking); always true when `level` is `too_old`.
    pub outdated: bool,
    /// Runtime commit the installed game was compiled from (`kit`: by the pre-release installer), when the install state has it.
    pub installed_commit: Option<String>,
    /// Runtime commit embedded in this launcher.
    pub bundled_commit: Option<String>,
    pub note: Option<String>,
    /// The installed game is for this other version (`level` is then `too_old`): the game update to
    /// `needed_version` is what is asked of the player.
    pub old_version: Option<String>,
    pub needed_version: &'static str,
}

/// The assessment. `has_exe`: the game executable exists.
pub fn assess(info: Option<&EngineInfo>, has_exe: bool, installed: Option<&str>, bundled: Option<&str>, old_version: Option<&str>) -> GameCheck {
    let installed_commit = installed.filter(|c| !c.is_empty()).map(str::to_string);
    let bundled_commit = bundled.filter(|c| !c.is_empty()).map(str::to_string);
    let note = info.and_then(|i| i.note.clone());
    let base = |level: &'static str, outdated: bool| GameCheck {
        level,
        outdated,
        installed_commit: installed_commit.clone(),
        bundled_commit: bundled_commit.clone(),
        note: note.clone(),
        old_version: old_version.map(str::to_string),
        needed_version: crate::installer::gamever::CURRENT.display,
    };
    if !has_exe {
        return base("missing", false);
    }
    if old_version.is_some() {
        return base("too_old", true);
    }
    if engine_too_old(info) {
        return base("too_old", true);
    }
    // A different commit than the embedded one. The pre-release installer's builds (`kit`) and any other recorded commit count: the
    // launcher only knows that the game was not compiled from the source it carries now.
    let other_commit = matches!((&installed_commit, &bundled_commit), (Some(i), Some(b)) if i != b);
    let older_listing = info.is_some_and(|i| i.options.is_some() && !crate::proto::engine::caps(Some(i)).contract_versions.contains(&2));
    let unknown = info.is_none_or(|i| i.options.is_none());
    base(if unknown { "unknown" } else { "ok" }, other_commit || older_listing)
}

/// The commit the compile step of the install state at `state_file` recorded (`done` only).
pub fn installed_commit(state_file: &Path) -> Option<String> {
    let st = crate::installer::StateFile::load(state_file)?;
    st.steps.get(crate::installer::StepId::Compile.name()).filter(|r| r.status == "done").map(|r| r.commit.clone()).filter(|c| !c.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn info(options: Option<serde_json::Value>, note: Option<&str>) -> EngineInfo {
        EngineInfo { fingerprint: "fp".into(), options, room_stages: None, note: note.map(str::to_string), lang: None }
    }
    fn listing(result_version: u64) -> serde_json::Value {
        json!({"format": "splat3-match-options", "version": 1, "modes": [{"key": "turf"}], "result": {"version": result_version}, "rounds": {"command": "next"}})
    }

    #[test]
    fn a_missing_command_is_too_old_but_a_timeout_is_not() {
        for note in ["the engine has no --list-match-options (exit code Some(1))", "not a JSON list (the engine probably has no --list-match-options)", "not JSON: expected value", "not a splat3-match-options document", "unknown version of the list"] {
            assert!(engine_too_old(Some(&info(None, Some(note)))), "{note}");
        }
        for note in ["no answer within 90 s", "C:\\game\\splat3.exe: Access is denied. (os error 5)", "no game executable", "the list is larger than 1 MiB"] {
            assert!(!engine_too_old(Some(&info(None, Some(note)))), "{note}");
        }
        assert!(!engine_too_old(None));
        assert!(!engine_too_old(Some(&info(Some(listing(2)), None))));
    }

    #[test]
    fn levels() {
        let old = info(None, Some("the engine has no --list-match-options (exit code Some(1))"));
        let c = assess(Some(&old), true, Some("aaa"), Some("aaa"), None);
        assert_eq!((c.level, c.outdated), ("too_old", true));
        let c = assess(Some(&info(None, Some("no answer within 90 s"))), true, None, Some("bbb"), None);
        assert_eq!((c.level, c.outdated), ("unknown", false));
        let c = assess(Some(&old), false, None, None, None);
        assert_eq!((c.level, c.outdated), ("missing", false));
        let ok = info(Some(listing(2)), None);
        assert!(!assess(Some(&ok), true, Some("aaa"), Some("aaa"), None).outdated);
        let c = assess(Some(&ok), true, Some("aaa"), Some("bbb"), None);
        assert_eq!((c.level, c.outdated, c.installed_commit.as_deref(), c.bundled_commit.as_deref()), ("ok", true, Some("aaa"), Some("bbb")));
        // The pre-release installer's builds are older than whatever this launcher carries.
        assert!(assess(Some(&ok), true, Some("kit"), Some("bbb"), None).outdated);
        // No record of the compiled commit: nothing to compare.
        assert!(!assess(Some(&ok), true, None, Some("bbb"), None).outdated);
        // A listing without the contract-2 result: a recompilation brings the newer one.
        let v1 = info(Some(listing(1)), None);
        let c = assess(Some(&v1), true, Some("bbb"), Some("bbb"), None);
        assert_eq!((c.level, c.outdated), ("ok", true));
    }

    #[test]
    fn a_game_of_another_version_is_blocked_like_a_game_too_old() {
        let ok = info(Some(listing(2)), None);
        // Whatever the engine's listing says, the previous version's game does not take part in online play.
        let c = assess(Some(&ok), true, Some("aaa"), Some("aaa"), Some("11.2.0"));
        assert_eq!((c.level, c.outdated, c.old_version.as_deref(), c.needed_version), ("too_old", true, Some("11.2.0"), crate::installer::gamever::CURRENT.display));
        let c = assess(None, true, None, None, Some("11.2.0"));
        assert_eq!(c.level, "too_old");
        // No executable: nothing to say about its version.
        assert_eq!(assess(None, false, None, None, Some("11.2.0")).level, "missing");
        // The current version has no old version to name.
        assert_eq!(assess(Some(&ok), true, Some("aaa"), Some("aaa"), None).old_version, None);
    }

    #[test]
    fn reads_the_compile_step_of_the_state_file() {
        let d = std::env::temp_dir().join(format!("s3l-fresh-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("state.json");
        assert_eq!(installed_commit(&p), None);
        let mut st = crate::installer::StateFile::default();
        st.steps.insert("compile".into(), crate::installer::StepRecord { status: "done".into(), commit: "abc123".into(), ..Default::default() });
        st.save(&p).unwrap();
        assert_eq!(installed_commit(&p).as_deref(), Some("abc123"));
        st.steps.insert("compile".into(), crate::installer::StepRecord { status: "failed".into(), commit: "abc123".into(), ..Default::default() });
        st.save(&p).unwrap();
        assert_eq!(installed_commit(&p), None);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
