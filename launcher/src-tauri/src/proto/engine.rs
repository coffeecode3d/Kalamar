//! What the installed game engine can run (`match-contract-2.md` 2): the engine's own capability list
//! (`splat3.exe --list-match-options`, a JSON document of the engine's format), cached per executable, and the
//! `PUT /v1/me/game` body built from it. The list is relayed to the server as the engine wrote it (`options`); this
//! launcher only reads what it must act on: the mode keys, the stage ids, the team limits, the rule keys and rounds.
//!
//! The listing runs only when a community server needs it (after a login or a new event stream), never at start-up,
//! with no window, and at most once per executable (its size and date) and language of its labels. An engine without
//! the command (it prints its usage and exits) is remembered as such: the launcher then declares contract 1 and the
//! five mode keys of the automatic match, as before. Nothing of the list is kept in the repository: it is the player's
//! own game data.
//!
//! **Stage identifiers** are the game's scene names (`match-contract.md` 2: the strings the runtime uses in its result
//! file, `stage_scene`, which the server compares with `rules.stage`). The engine's list numbers its stages (`stage`)
//! and gives each one's `scene`; its rule key `stage=` takes either. The launcher declares scenes, and maps the
//! numbered room list of a status file to them.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const FORMAT: &str = "splat3-match-options";
/// What the server accepts in `options` (`match-contract-2.md`: at most 48 KiB).
pub const MAX_OPTIONS_BYTES: usize = 48 * 1024;
const MAX_OUTPUT: usize = 1 << 20;
const LIST_TIMEOUT: Duration = Duration::from_secs(90);
/// The capability an engine names in the `capabilities` array of its list when its save profile applies the sub
/// abilities of the equipped gear (`gear_abilities`) and the ability chunk stock (`chunks`).
pub const CAP_GEAR_ABILITIES: &str = "gear-abilities";
/// The automatic match's mode keys before the engine listed its own (contract 1).
pub const DEFAULT_MODES: &[&str] = crate::proto::contract::MODES;

/// The engine as last seen, kept in `<data>\engine\match-options.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EngineInfo {
    /// Size and modification time of the executable the list was read from.
    pub fingerprint: String,
    /// The engine's list, verbatim; `None` when the engine has no such command.
    pub options: Option<Value>,
    /// The stages of this player's room list, from the last status file that carried them (`stages`).
    #[serde(default)]
    pub room_stages: Option<Vec<Value>>,
    /// Why the list is missing (the command's failure), for the details line.
    #[serde(default)]
    pub note: Option<String>,
    /// The language of the list's labels (`SPLAT3_LIST_LANG`), when one was asked for.
    #[serde(default)]
    pub lang: Option<String>,
}

/// The engine's label language (the game's message prefixes) for the launcher's language.
pub fn list_lang(ui: &str) -> &'static str {
    match ui {
        "fr" => "EUfr",
        "es" => "USes",
        "ja" => "JPja",
        _ => "USen",
    }
}

/// The game version the runtime targets (`installer::gamever`, the one place that names it).
pub const GAME_VERSION: &str = crate::installer::gamever::CURRENT.display;

/// `build` of `PUT /v1/me/game` (protocol 1.1.12: a build, never an installation): the launcher's version, the engine
/// snapshot the game was compiled from (the first 12 hex digits of its identifier; `kit` for builds made by the
/// pre-release installer, `unknown` when no record says) and the game version: the same text for every player with the
/// same build, nothing of this computer. [`fingerprint`] (the size and modification time of the locally compiled
/// executable) would tell one installation from every other and let servers link accounts: it is never sent.
pub fn build_label(runtime_commit: Option<&str>) -> String {
    let commit = match runtime_commit.map(str::trim) {
        Some("kit") => "kit".to_string(),
        Some(c) if (7..=64).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_hexdigit()) => c[..12.min(c.len())].to_ascii_lowercase(),
        _ => "unknown".to_string(),
    };
    format!("kalamar/{} runtime/{commit} game/{GAME_VERSION}", env!("CARGO_PKG_VERSION"))
}

/// The executable's size and modification time: the key of the local listing cache only, never sent anywhere.
pub fn fingerprint(exe: &Path) -> Option<String> {
    let m = std::fs::metadata(exe).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(format!("exe-{:x}-{:x}", m.len(), t))
}

fn cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join("engine").join("match-options.json")
}

pub fn load_cache(data_dir: &Path) -> Option<EngineInfo> {
    crate::util::read_text_limited(&cache_path(data_dir), 4 << 20).ok().flatten().and_then(|t| serde_json::from_str(&t).ok())
}

pub fn save_cache(data_dir: &Path, info: &EngineInfo) {
    if let Ok(t) = serde_json::to_string(info) {
        let _ = crate::util::atomic_write(&cache_path(data_dir), t.as_bytes());
    }
}

/// Checks the engine's output: a JSON object of its format, version 1 or later.
pub fn parse_options(stdout: &[u8]) -> Result<Value, String> {
    if stdout.len() > MAX_OUTPUT {
        return Err("the list is larger than 1 MiB".into());
    }
    let text = String::from_utf8_lossy(stdout);
    let start = text.find('{').ok_or("not a JSON list (the engine probably has no --list-match-options)")?;
    let v: Value = serde_json::from_str(text[start..].trim()).map_err(|e| format!("not JSON: {e}"))?;
    if v.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(format!("not a {FORMAT} document"));
    }
    if !v.get("version").and_then(Value::as_u64).is_some_and(|n| n >= 1) {
        return Err("unknown version of the list".into());
    }
    Ok(v)
}

/// Runs `<exe> --list-match-options` (no window, the user data folder as working directory, a time limit), its labels
/// in `lang` when given.
pub fn run_listing(exe: &Path, cwd: &Path, lang: Option<&str>) -> Result<Value, String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--list-match-options").current_dir(cwd).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    // Under the neutral name and the older one, like a launch (`launch::runtime_env`).
    let env: Vec<(String, String)> = lang.map(|l| ("SPLAT3_LIST_LANG".to_string(), l.to_string())).into_iter().collect();
    crate::launch::apply_env(&mut cmd, &["SPLAT3_LIST_LANG".to_string()], &env);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", exe.display()))?;
    let mut out = child.stdout.take().ok_or("no output")?;
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = (&mut out).take(MAX_OUTPUT as u64 + 1).read_to_end(&mut buf);
        buf
    });
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        if started.elapsed() > LIST_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("no answer within {} s", LIST_TIMEOUT.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let buf = reader.join().unwrap_or_default();
    let parsed = parse_options(&buf);
    if !status.success() && parsed.is_err() {
        return Err(format!("the engine has no --list-match-options (exit code {:?})", status.code()));
    }
    parsed
}

/// The engine of `exe`: from the cache when the executable did not change, else listed now (and cached).
pub fn engine_info(data_dir: &Path, exe: &Path, cwd: &Path) -> EngineInfo {
    engine_info_in(data_dir, exe, cwd, None)
}

/// [`engine_info`] with the labels in `lang` (listed again when the cached list has another language; the room list
/// seen in status files is kept).
pub fn engine_info_in(data_dir: &Path, exe: &Path, cwd: &Path, lang: Option<&str>) -> EngineInfo {
    let Some(fp) = fingerprint(exe) else {
        return EngineInfo { fingerprint: "unknown".into(), options: None, room_stages: None, note: Some("no game executable".into()), lang: None };
    };
    let cached = load_cache(data_dir).filter(|c| c.fingerprint == fp);
    if let Some(c) = cached.as_ref().filter(|c| lang.is_none() || c.lang.as_deref() == lang || c.options.is_none()) {
        return c.clone();
    }
    let (options, note) = match run_listing(exe, cwd, lang) {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e)),
    };
    let info = EngineInfo { fingerprint: fp, options, room_stages: cached.and_then(|c| c.room_stages), note, lang: lang.map(str::to_string) };
    save_cache(data_dir, &info);
    info
}

/// The local names of the list (`match-contract.md` 2: a launcher names game identifiers from the player's own game
/// data): `{"stages": {scene: label}, "modes": {key: label}}`.
pub fn names(info: Option<&EngineInfo>) -> Value {
    let mut stages = serde_json::Map::new();
    let mut modes = serde_json::Map::new();
    if let Some(o) = info.and_then(|i| i.options.as_ref()) {
        for s in o.get("stages").and_then(Value::as_array).into_iter().flatten().take(256) {
            if let (Some(scene), Some(label)) = (s.get("scene").and_then(id_text), s.get("label").and_then(Value::as_str).filter(|l| !l.is_empty() && l.len() <= 120)) {
                stages.insert(scene, json!(label));
            }
        }
        for m in o.get("modes").and_then(Value::as_array).into_iter().flatten().take(64) {
            if let (Some(key), Some(label)) = (m.get("key").and_then(id_text), m.get("label").and_then(Value::as_str).filter(|l| !l.is_empty() && l.len() <= 120)) {
                modes.insert(key, json!(label));
            }
        }
    }
    json!({"stages": stages, "modes": modes})
}

/// Keeps the stages of the player's room list a status file carried (`stages`), for the next declaration.
pub fn remember_room_stages(data_dir: &Path, stages: &[Value]) {
    let Some(mut c) = load_cache(data_dir) else { return };
    let list: Vec<Value> = stages.iter().filter(|s| s.is_string() || s.is_number()).take(256).cloned().collect();
    if list.is_empty() || c.room_stages.as_ref() == Some(&list) {
        return;
    }
    c.room_stages = Some(list);
    save_cache(data_dir, &c);
}

/// What this launcher reads of the engine's list.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Caps {
    /// Contract versions this launcher can run with this engine.
    pub contract_versions: Vec<u32>,
    pub modes: Vec<String>,
    /// Stage identifiers: the scenes (see the module's notes).
    pub stages: Vec<String>,
    pub rule_keys: Vec<String>,
    pub rounds: bool,
    pub max_players: u64,
    pub max_per_team: u64,
    pub uneven_teams: bool,
    pub spectators: bool,
    /// 1.1.7: the engine's places for spectators (its `spectators` member; `max` 0: none): declared as
    /// `engine.spectator_seats` with `launcher.spectate`, so the server may seat this player in the stands.
    pub spectator_max: u64,
    /// The modes a room can have spectators in (empty with `spectator_max` > 0: every mode).
    pub spectator_modes: Vec<String>,
    /// The modes the engine ran with spectators.
    pub spectator_verified: Vec<String>,
    /// A host in the stands was run.
    pub spectator_host: bool,
    /// The list was read from the engine (else: the contract 1 defaults).
    pub listed: bool,
    /// The engine applies a loadout for one match to a copy of the save (`match_loadout`, `SPLAT3_MATCH_LOADOUT`): the
    /// constraint kinds the launcher can then apply (`weapon`, `gear`).
    pub loadout: Vec<String>,
    /// A round in the open game reaches its intro sooner than a new game (`rounds.faster_than_restart`).
    pub rounds_faster: bool,
    /// The engine's `stage_list`: `all` (every listed stage, whatever the save) or `host_save` (older engines).
    pub stage_list: String,
    /// The engine applies `gear_abilities` and `chunks` of a save profile: its list's `capabilities` names
    /// [`CAP_GEAR_ABILITIES`]. An engine that does not name it may ignore both fields without a word.
    pub gear_abilities: bool,
}

fn id_text(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string).or_else(|| v.as_i64().map(|n| n.to_string())).filter(|s| (1..=64).contains(&s.len()))
}

/// Capabilities from the engine's list. Contract 2 needs the engine's list (rules for every player, a match id other
/// than the room, result files of version 2); without it: contract 1 and the default mode keys.
pub fn caps(info: Option<&EngineInfo>) -> Caps {
    let Some(o) = info.and_then(|i| i.options.as_ref()) else {
        return Caps { contract_versions: vec![1], modes: DEFAULT_MODES.iter().map(|s| s.to_string()).collect(), max_players: 8, max_per_team: 4, ..Default::default() };
    };
    let modes: Vec<String> = o.get("modes").and_then(Value::as_array).map(|a| a.iter().filter_map(|m| m.get("key").and_then(id_text)).take(64).collect()).unwrap_or_default();
    // The room list when a status file gave it (numbered: mapped to the scenes), else every stage the game's stage
    // lists show. A stage without a scene in the list cannot be named in the protocol: left out.
    let listed: Vec<&Value> = o.get("stages").and_then(Value::as_array).map(|a| a.iter().take(256).collect()).unwrap_or_default();
    let scene = |s: &Value| s.get("scene").and_then(id_text);
    // `stage_list: "all"`: a room offers every stage the stage lists show, whatever the save.
    let all_stages = o.get("stage_list").and_then(Value::as_str) == Some("all");
    let stages: Vec<String> = match info.and_then(|i| i.room_stages.as_ref()).filter(|_| !all_stages) {
        Some(room) => room
            .iter()
            .filter_map(|id| {
                let id = id_text(id)?;
                listed.iter().find(|s| s.get("stage").and_then(id_text).as_deref() == Some(id.as_str()) || scene(s).as_deref() == Some(id.as_str())).and_then(|s| scene(s))
            })
            .collect(),
        None => listed.iter().filter(|s| s.get("in_stage_lists").and_then(Value::as_bool) != Some(false)).filter_map(|s| scene(s)).collect(),
    };
    let teams = o.get("teams").cloned().unwrap_or(Value::Null);
    let mut rule_keys: Vec<String> = ["players", "teams", "mode"].iter().map(|s| s.to_string()).collect();
    if !stages.is_empty() || o.get("stage_random").and_then(Value::as_bool) == Some(true) {
        rule_keys.push("stage".into());
    }
    if let Some(r) = o.get("room_options").and_then(Value::as_object) {
        rule_keys.extend(r.keys().filter(|k| k.len() <= 64).cloned());
    }
    if o.get("results_hold").is_some() {
        rule_keys.push("results_hold".into());
    }
    // 1.1.7: the engine's spectators (its `spectators` member, from engines of dc39dfe on); the wait is a rule key.
    let sp = o.get("spectators").filter(|s| s.is_object());
    let spectator_max = sp.and_then(|s| s.get("max")).and_then(Value::as_u64).unwrap_or(0).min(2);
    let ids = |k: &str| -> Vec<String> { sp.and_then(|s| s.get(k)).and_then(Value::as_array).map(|a| a.iter().filter_map(id_text).take(64).collect()).unwrap_or_default() };
    if let Some(k) = sp.and_then(|s| s.pointer("/wait/key")).and_then(Value::as_str).filter(|k| (1..=64).contains(&k.len())) {
        rule_keys.push(k.to_string());
    }
    let result_v2 = o.pointer("/result/version").and_then(Value::as_u64).is_some_and(|v| v >= 2);
    Caps {
        contract_versions: if result_v2 { vec![1, 2] } else { vec![1] },
        modes: if modes.is_empty() { DEFAULT_MODES.iter().map(|s| s.to_string()).collect() } else { modes },
        stages,
        rule_keys,
        rounds: result_v2 && o.pointer("/rounds/command").and_then(Value::as_str) == Some("next"),
        max_players: teams.get("players_max").and_then(Value::as_u64).unwrap_or(8).clamp(2, 8),
        max_per_team: teams.get("places_per_team").and_then(Value::as_u64).unwrap_or(4).clamp(1, 4),
        uneven_teams: teams.get("uneven").and_then(Value::as_bool).unwrap_or(false),
        spectators: teams.get("spectators").and_then(Value::as_bool).unwrap_or(false),
        // Contract 2 only: the rules (with the letter S) go to every participant.
        spectator_max: if result_v2 { spectator_max } else { 0 },
        spectator_modes: ids("modes"),
        spectator_verified: ids("verified_modes"),
        spectator_host: sp.and_then(|s| s.get("host_verified")).and_then(Value::as_bool) == Some(true),
        listed: true,
        loadout: match o.get("match_loadout") {
            Some(ml) if ml.is_object() && result_v2 => {
                let fields: Vec<String> = ml.get("fields").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).filter(|f| matches!(*f, "weapon" | "gear")).map(str::to_string).collect()).unwrap_or_default();
                if fields.is_empty() { vec!["weapon".into(), "gear".into()] } else { fields }
            }
            _ => Vec::new(),
        },
        rounds_faster: o.pointer("/rounds/faster_than_restart").and_then(Value::as_bool) == Some(true),
        stage_list: if all_stages { "all".into() } else { "host_save".into() },
        gear_abilities: o.get("capabilities").and_then(Value::as_array).is_some_and(|a| a.iter().any(|c| c.as_str() == Some(CAP_GEAR_ABILITIES))),
    }
}

/// `PUT /v1/me/game` (1.1, a superset of the 1.0 body). `engine.loadout`: the constraint kinds this launcher applies,
/// those of the engine's `match_loadout` (a copy of the save for the match). Rounds in one game session are announced
/// when the engine says a round there is faster than a new game (`rounds.faster_than_restart`), or on request
/// (`declare_rounds`); engines before that took about 120 s a round against about 50 s for a fresh launch.
pub fn game_body(build: &str, info: Option<&EngineInfo>, declare_rounds: bool) -> Value {
    let c = caps(info);
    let mut body = json!({
        "contract": {"id": crate::proto::contract::CONTRACT_ID, "version": 1},
        "contracts": [{"id": crate::proto::contract::CONTRACT_ID, "versions": c.contract_versions}],
        "build": build,
        "modes": c.modes,
        "stages": c.stages,
        "can_select_stage": c.listed && !c.stages.is_empty(),
        "max_players": c.max_players,
        // 1.1.6: this launcher shows the stage draft (`draft.md`), version 1 of its object.
        "launcher": {"draft": [crate::proto::DRAFT_VERSION]},
    });
    // 1.1.7: this launcher runs the spectator seat (`spectators.md`), when the engine has places for spectators.
    if c.listed && c.spectator_max > 0 {
        body["launcher"]["spectate"] = json!([crate::proto::SPECTATE_VERSION]);
    }
    if c.listed {
        body["engine"] = json!({
            "rule_keys": c.rule_keys,
            "rounds": c.rounds && (declare_rounds || c.rounds_faster),
            "max_players": c.max_players,
            "max_per_team": c.max_per_team,
            "uneven_teams": c.uneven_teams,
            "spectators": c.spectators,
            "loadout": c.loadout,
            // 1.1.3: the engine's own capabilities, as its list names them.
            "match_loadout": !c.loadout.is_empty(),
            "rounds_faster": c.rounds && c.rounds_faster,
            "stage_list": c.stage_list,
        });
        if c.spectator_max > 0 {
            let mut seats = json!({"max": c.spectator_max, "verified_modes": c.spectator_verified, "host": c.spectator_host});
            if !c.spectator_modes.is_empty() {
                seats["modes"] = json!(c.spectator_modes);
            }
            body["engine"]["spectator_seats"] = seats;
        }
        if let Some(o) = info.and_then(|i| i.options.as_ref())
            && serde_json::to_vec(o).map(|b| b.len() <= MAX_OPTIONS_BYTES).unwrap_or(false)
        {
            body["options"] = o.clone();
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine's format with neutral ids and labels (none of the game's).
    pub fn sample() -> Value {
        json!({"format": "splat3-match-options", "version": 1, "language": "USen",
            "modes": [{"key": "turf", "rule": 0, "name": "R0", "label": "Mode A"}, {"key": "zones", "rule": 1, "name": "R1", "label": "Mode B"}],
            "not_in_private_rooms": [],
            "stages": [{"stage": 1, "scene": "S1", "label": "Stage one", "in_stage_lists": true}, {"stage": 2, "scene": "S2", "label": "Hidden", "in_stage_lists": false}, {"stage": 3, "scene": "S3", "label": "Stage three", "in_stage_lists": true}],
            "stage_random": true, "stage_list": "host_save",
            "teams": {"places_per_team": 4, "players_min": 2, "players_max": 8, "uneven": true, "any_team": true, "by_name": "relay", "spectators": false, "verified_players_max": 2},
            "room_options": {"option_a": {"values": [0, 1], "default": 0}},
            "recon": false, "time_limit": false,
            "rounds": {"command": "next", "per_round": ["mode", "stage"]},
            "results_hold": {"min": 0, "max": 600, "default": 10},
            "loadout": "profile", "result": {"version": 2, "outcomes": ["completed", "aborted"]}})
    }

    #[test]
    fn the_gear_abilities_capability_is_read_from_the_lists_capabilities() {
        let info = |o: Value| EngineInfo { fingerprint: "f".into(), options: Some(o), room_stages: None, note: None, lang: None };
        assert!(!caps(Some(&info(sample()))).gear_abilities, "a list without the member: an older engine");
        let mut o = sample();
        o["capabilities"] = json!(["something-else"]);
        assert!(!caps(Some(&info(o.clone()))).gear_abilities);
        o["capabilities"] = json!(["something-else", CAP_GEAR_ABILITIES]);
        assert!(caps(Some(&info(o.clone()))).gear_abilities);
        o["capabilities"] = json!("gear-abilities");
        assert!(!caps(Some(&info(o))).gear_abilities, "an array is expected");
        assert!(!caps(None).gear_abilities);
        assert_eq!(CAP_GEAR_ABILITIES, "gear-abilities");
    }

    #[test]
    fn the_engines_list_gives_contract_2_and_its_limits() {
        let v = parse_options(format!("log line before\n{}", sample()).as_bytes()).unwrap();
        let info = EngineInfo { fingerprint: "f".into(), options: Some(v), room_stages: None, note: None, lang: None };
        let c = caps(Some(&info));
        assert_eq!(c.contract_versions, [1, 2]);
        assert_eq!(c.modes, ["turf", "zones"]);
        assert_eq!(c.stages, ["S1", "S3"], "the scenes of the stages the stage lists show");
        assert!(c.rounds && c.uneven_teams && !c.spectators);
        assert_eq!((c.max_players, c.max_per_team), (8, 4));
        assert_eq!(c.rule_keys, ["players", "teams", "mode", "stage", "option_a", "results_hold"]);
        let b = game_body("exe-1", Some(&info), false);
        assert_eq!(b["contracts"][0]["versions"], json!([1, 2]));
        assert_eq!(b["engine"]["rounds"], false, "relaunch between games unless asked");
        assert_eq!(game_body("exe-1", Some(&info), true)["engine"]["rounds"], true);
        assert_eq!(b["can_select_stage"], true);
        assert_eq!(b["options"]["format"], FORMAT, "relayed verbatim");
        assert_eq!(b["stages"], json!(["S1", "S3"]));
        assert_eq!(b["engine"]["loadout"], json!([]), "no match_loadout: no constraint applied");
        // A match's copy of the save takes a loadout, every listed stage is playable, a round is faster.
        let mut o2 = sample();
        o2["match_loadout"] = json!({"setting": "SPLAT3_MATCH_LOADOUT", "format": "save_profile", "fields": ["weapon", "gear"], "player_save_written": false});
        o2["stage_list"] = json!("all");
        o2["rounds"]["faster_than_restart"] = json!(true);
        let info3 = EngineInfo { options: Some(o2), room_stages: Some(vec![json!(3)]), ..info.clone() };
        let c3 = caps(Some(&info3));
        assert_eq!((c3.loadout.clone(), c3.stages.clone(), c3.rounds_faster), (vec!["weapon".to_string(), "gear".to_string()], vec!["S1".to_string(), "S3".to_string()], true), "stage_list all: the whole list, not the room's");
        let b3 = game_body("exe-1", Some(&info3), false);
        assert_eq!((b3["engine"]["loadout"].clone(), b3["engine"]["rounds"].clone()), (json!(["weapon", "gear"]), json!(true)), "a faster round is announced");
        assert_eq!((b3["engine"]["match_loadout"].clone(), b3["engine"]["rounds_faster"].clone(), b3["engine"]["stage_list"].clone()), (json!(true), json!(true), json!("all")), "1.1.3 names");
        assert_eq!((b["engine"]["match_loadout"].clone(), b["engine"]["rounds_faster"].clone(), b["engine"]["stage_list"].clone()), (json!(false), json!(false), json!("host_save")));
        // a room list seen in a status file wins (numbered there: mapped to the scenes; an unknown number is dropped)
        let info2 = EngineInfo { room_stages: Some(vec![json!(3), json!(2), json!(99)]), ..info.clone() };
        assert_eq!(caps(Some(&info2)).stages, ["S3", "S2"]);
        let n = names(Some(&info));
        assert_eq!((n["stages"]["S3"].as_str(), n["modes"]["zones"].as_str()), (Some("Stage three"), Some("Mode B")));
        assert_eq!(names(None), json!({"stages": {}, "modes": {}}));
    }

    #[test]
    fn an_engine_with_spectator_places_declares_the_seat() {
        let mut o = sample();
        o["teams"]["spectators"] = json!(true);
        let info = EngineInfo { fingerprint: "f".into(), options: Some(o.clone()), room_stages: None, note: None, lang: None };
        let b = game_body("exe-1", Some(&info), false);
        assert!(b["engine"].get("spectator_seats").is_none() && b["launcher"].get("spectate").is_none(), "the 1.1 boolean alone seats nobody");
        assert_eq!(b["engine"]["spectators"], true);
        // The engine's `spectators` member (dc39dfe): two places, every mode, two verified, the host verified, the wait.
        o["spectators"] = json!({"max": 2, "team": "S", "in_players": true, "session_max": 10, "modes": ["turf", "zones"], "verified_max": 2,
            "verified_modes": ["turf"], "host_verified": true, "seat": ["status", "result"], "result_outcome": "spectated",
            "wait": {"key": "spectators_wait", "min": 0, "max": 600, "default": 60, "needs": "teams_by_name"}});
        let info = EngineInfo { options: Some(o.clone()), ..info };
        let c = caps(Some(&info));
        assert_eq!((c.spectator_max, c.spectator_modes.len(), c.spectator_verified.clone(), c.spectator_host), (2, 2, vec!["turf".to_string()], true));
        assert!(c.rule_keys.contains(&"spectators_wait".to_string()));
        let b = game_body("exe-1", Some(&info), false);
        assert_eq!(b["engine"]["spectator_seats"], json!({"max": 2, "modes": ["turf", "zones"], "verified_modes": ["turf"], "host": true}));
        assert_eq!(b["launcher"]["spectate"], json!([1]));
        // An engine whose results are of version 1 (contract 1 only) cannot be told the rules with S: no seat.
        o["result"] = json!({"version": 1});
        let info = EngineInfo { options: Some(o), ..info };
        assert_eq!(caps(Some(&info)).spectator_max, 0);
        assert!(game_body("exe-1", Some(&info), false)["launcher"].get("spectate").is_none());
    }

    #[test]
    fn without_the_list_contract_1_as_before() {
        let c = caps(None);
        assert_eq!((c.contract_versions.clone(), c.modes.len(), c.listed), (vec![1], 5, false));
        let b = game_body(&build_label(None), None, true);
        assert_eq!(b["build"], format!("kalamar/{} runtime/unknown game/{GAME_VERSION}", env!("CARGO_PKG_VERSION")));
        assert_eq!(b["contract"]["version"], 1);
        assert!(b.get("engine").is_none() && b.get("options").is_none());
        assert_eq!(b["launcher"]["draft"], json!([1]), "the draft screen does not depend on the engine");
        assert_eq!(b["can_select_stage"], false);
        assert!(parse_options(b"usage: splat3.exe [--selftest | ...]").is_err());
        assert!(parse_options(br#"{"format": "other"}"#).is_err());
        let info = EngineInfo { fingerprint: "f".into(), options: None, room_stages: None, note: Some("x".into()), lang: None };
        assert_eq!(caps(Some(&info)).contract_versions, [1]);
    }

    #[test]
    fn a_list_too_big_is_not_relayed() {
        let mut v = sample();
        v["x-padding"] = json!("x".repeat(MAX_OPTIONS_BYTES));
        let info = EngineInfo { fingerprint: "f".into(), options: Some(v), room_stages: None, note: None, lang: None };
        let b = game_body("b", Some(&info), true);
        assert!(b.get("options").is_none());
        assert_eq!(b["engine"]["rounds"], true);
    }

    #[test]
    fn the_cache_follows_the_executable() {
        let d = std::env::temp_dir().join(format!("s3l-eng-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        let exe = d.join("game.exe");
        std::fs::write(&exe, b"not a program").unwrap();
        let fp = fingerprint(&exe).unwrap();
        save_cache(&d, &EngineInfo { fingerprint: fp.clone(), options: Some(sample()), room_stages: None, note: None, lang: Some("EUfr".into()) });
        // cached: the executable is not run (it could not be)
        let i = engine_info(&d, &exe, &d);
        assert!(i.options.is_some());
        assert!(engine_info_in(&d, &exe, &d, Some("EUfr")).options.is_some(), "same language: the cache");
        remember_room_stages(&d, &[json!(1), json!("x"), json!({"bad": 1})]);
        assert_eq!(load_cache(&d).unwrap().room_stages, Some(vec![json!(1), json!("x")]));
        // another language: listed again (this executable cannot run: no list), the room list kept
        let i = engine_info_in(&d, &exe, &d, Some("USen"));
        assert!(i.options.is_none() && i.room_stages.is_some());
        assert_eq!(list_lang("fr"), "EUfr");
        assert_eq!(list_lang("en"), "USen");
        assert_eq!((list_lang("es"), list_lang("ja")), ("USes", "JPja"));
        // another executable: listed again; this one cannot run, so: no list, remembered with the reason
        std::fs::write(&exe, b"not a program either").unwrap();
        let i = engine_info(&d, &exe, &d);
        assert!(i.options.is_none() && i.note.is_some());
        assert_ne!(i.fingerprint, fp);
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// The review's M1: `build` named one installation (`exe-<size>-<mtime>`). It is now the same for every copy of a
    /// build and carries nothing of the computer.
    #[test]
    fn the_declared_build_names_a_build_never_an_installation() {
        let v = env!("CARGO_PKG_VERSION");
        assert_eq!(build_label(Some("4cb0304a1b2c3d4e5f60718293a4b5c6d7e8f901")), format!("kalamar/{v} runtime/4cb0304a1b2c game/11.3.0"));
        assert_eq!(GAME_VERSION, "11.3.0", "the build label names the game version the launcher builds");
        assert_eq!(build_label(Some("4CB0304")), format!("kalamar/{v} runtime/4cb0304 game/{GAME_VERSION}"));
        assert_eq!(build_label(Some("kit")), format!("kalamar/{v} runtime/kit game/{GAME_VERSION}"));
        for odd in [None, Some(""), Some("C:\\Users\\someone\\splat3.exe"), Some("exe-1a2b-6543210f"), Some("abc")] {
            assert_eq!(build_label(odd), format!("kalamar/{v} runtime/unknown game/{GAME_VERSION}"), "{odd:?}");
        }
        // Two installations of the same build (other sizes and dates on disk) declare the same thing.
        let d = std::env::temp_dir().join(format!("s3l-build-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        let (a, b) = (d.join("a.exe"), d.join("b.exe"));
        std::fs::write(&a, b"one").unwrap();
        std::fs::write(&b, b"another size").unwrap();
        assert_ne!(fingerprint(&a), fingerprint(&b));
        let body = |_: &std::path::Path| game_body(&build_label(Some("4cb0304a1b2c")), None, false);
        assert_eq!(body(&a)["build"], body(&b)["build"]);
        assert!(!body(&a)["build"].as_str().unwrap().contains("exe-"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
