//! The runtime's automatic match mode (`engine/docs/auto-match.md`): the variables it reads, the status
//! file it writes, the result file it writes, and the log lines the launcher falls back on.
//!
//! Every runtime-facing name lives in [`MatchVars`] (one table). `match-vars.json` in the launcher data directory
//! overrides entries by `id` (name, value, enabled) without a rebuild, so the names can be aligned with the runtime
//! later. Names and values follow the runtime's document (HEAD 075a879): `SPLAT3_MATCH_REVEAL=intro|loading|never`
//! (the window stays hidden until the stage intro), `SPLAT3_MATCH_ON_ERROR=exit|show`, `SPLAT3_MATCH_TIMEOUTS`
//! (off: the runtime's defaults are the document's).
//!
//! | id | variable (default) | value | when |
//! |---|---|---|---|
//! | `start` | `SPLAT3_START` | `lobby-lan` | always (the match needs the LAN lobby) |
//! | `role` | `SPLAT3_MATCH` | `host` or `join` | always |
//! | `id` | `SPLAT3_MATCH_ID` | match id (= relay room) | always |
//! | `rules` | `SPLAT3_MATCH_RULES` | `players=<n>,teams=<AB...>,mode=<mode>` | host |
//! | `host` | `SPLAT3_MATCH_HOST` | host nickname | guest |
//! | `result` | `SPLAT3_MATCH_RESULT` | path of the result JSON | always |
//! | `exit` | `SPLAT3_MATCH_EXIT` | `1` | always |
//! | `status` | `SPLAT3_MATCH_STATUS` | path of the status JSON | always |
//! | `reveal` | `SPLAT3_MATCH_REVEAL` | `intro` | always |
//! | `on_error` | `SPLAT3_MATCH_ON_ERROR` | `exit` (the game closes with code 3 and the launcher says why) | always |
//! | `timeouts` | `SPLAT3_MATCH_TIMEOUTS` | `boot=300,list=90,...` | off (runtime defaults) |
//! | `wait` | `SPLAT3_MATCH_WAIT` | seconds (older form of `players=`/`join=`) | off |
//! | `lan` / `lan_room` / `lan_name` | `SPLAT3_LAN` / `SPLAT3_LAN_ROOM` / `SPLAT3_LAN_NAME` | `relay:<ip>[:<port>]`, match id, nickname | always (built by `lan.rs`) |

use crate::error::{AppError, AppResult, FieldError};
use crate::lan::{self, LanMode, LanRequest};
use crate::matchcode::MatchSpec;
use crate::util;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VarDef {
    pub id: String,
    pub name: String,
    /// Fixed value (the others are computed per match).
    #[serde(default)]
    pub value: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MatchVars {
    pub vars: Vec<VarDef>,
}

fn def(id: &str, name: &str, value: Option<&str>, enabled: bool) -> VarDef {
    VarDef { id: id.into(), name: name.into(), value: value.map(str::to_string), enabled }
}

impl Default for MatchVars {
    fn default() -> Self {
        MatchVars {
            vars: vec![
                def("start", "SPLAT3_START", Some("lobby-lan"), true),
                def("role", "SPLAT3_MATCH", None, true),
                def("id", "SPLAT3_MATCH_ID", None, true),
                def("rules", "SPLAT3_MATCH_RULES", None, true),
                def("host", "SPLAT3_MATCH_HOST", None, true),
                def("result", "SPLAT3_MATCH_RESULT", None, true),
                def("exit", "SPLAT3_MATCH_EXIT", Some("1"), true),
                def("status", "SPLAT3_MATCH_STATUS", None, true),
                def("reveal", "SPLAT3_MATCH_REVEAL", Some("intro"), true),
                def("on_error", "SPLAT3_MATCH_ON_ERROR", Some("exit"), true),
                def("timeouts", "SPLAT3_MATCH_TIMEOUTS", None, false),
                def("wait", "SPLAT3_MATCH_WAIT", None, false),
            ],
        }
    }
}

fn valid_var(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') && !n.starts_with(|c: char| c.is_ascii_digit())
}

impl MatchVars {
    /// Defaults overlaid with `match-vars.json` (entries with a known id and a sane name; the rest is ignored).
    pub fn load(data_dir: &Path) -> MatchVars {
        let mut t = MatchVars::default();
        let Some(over) = util::read_text_limited(&data_dir.join("match-vars.json"), 64 * 1024)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        else {
            return t;
        };
        if let Some(list) = over.get("vars").and_then(|m| serde_json::from_value::<Vec<VarDef>>(m.clone()).ok()) {
            for v in list.into_iter().filter(|v| valid_var(&v.name) && v.value.as_deref().is_none_or(|s| s.len() <= 128 && !s.chars().any(char::is_control))) {
                if let Some(x) = t.vars.iter_mut().find(|x| x.id == v.id) {
                    *x = v;
                }
            }
        }
        t
    }

    fn get(&self, id: &str) -> Option<&VarDef> {
        self.vars.iter().find(|v| v.id == id && v.enabled)
    }

    fn push(&self, env: &mut Vec<(String, String)>, id: &str, computed: Option<String>) {
        if let Some(d) = self.get(id)
            && let Some(v) = computed.or_else(|| d.value.clone())
        {
            env.push((d.name.clone(), v));
        }
    }

    /// Every variable name the table can set, for stripping the inherited environment.
    pub fn names(&self) -> Vec<String> {
        self.vars.iter().map(|v| v.name.clone()).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Host,
    Join,
}

impl Role {
    pub fn word(self) -> &'static str {
        match self {
            Role::Host => "host",
            Role::Join => "join",
        }
    }
}

/// Where the runtime writes for one match: under `<userdata>\match\<id>\`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MatchPaths {
    pub dir: PathBuf,
    pub status: PathBuf,
    pub result: PathBuf,
}

pub fn match_paths(userdata: &Path, match_id: &str) -> MatchPaths {
    let dir = userdata.join("match").join(match_id);
    MatchPaths { status: dir.join("status.json"), result: dir.join("result.json"), dir }
}

/// `teams=` for `n` players: Alpha, Bravo, Alpha... in arrival order (the host first).
pub fn alternating_teams(players: u8) -> String {
    (0..players).map(|i| if i % 2 == 0 { 'A' } else { 'B' }).collect()
}

pub fn rules_string(spec: &MatchSpec) -> String {
    format!("players={},teams={},mode={}", spec.players, alternating_teams(spec.players), spec.mode)
}

/// The environment of a match launch, in a stable order: the LAN relay variables (through `lan.rs`), then the table.
pub fn build_match_env(vars: &MatchVars, role: Role, spec: &MatchSpec, paths: &MatchPaths, nickname: &str) -> AppResult<Vec<(String, String)>> {
    spec.validate().map_err(|e| AppError::Validation(vec![FieldError { key: "code".into(), message: e.id().into() }]))?;
    let nick = nickname.trim();
    if nick.is_empty() {
        return Err(AppError::Validation(vec![FieldError { key: "name".into(), message: "empty".into() }]));
    }
    let id = spec.match_id();
    let req = LanRequest {
        mode: LanMode::Relay,
        relay_host: spec.relay_ip.to_string(),
        relay_port: spec.relay_port,
        room: id.clone(),
        net_log: false,
        bind_any: false,
        lan_address: String::new(),
    };
    let mut env = lan::build_lan_env(&req, nick)?;
    vars.push(&mut env, "start", None);
    vars.push(&mut env, "role", Some(role.word().to_string()));
    vars.push(&mut env, "id", Some(id));
    match role {
        Role::Host => vars.push(&mut env, "rules", Some(rules_string(spec))),
        Role::Join => vars.push(&mut env, "host", Some(spec.host.clone())),
    }
    vars.push(&mut env, "result", Some(paths.result.display().to_string()));
    vars.push(&mut env, "exit", None);
    vars.push(&mut env, "status", Some(paths.status.display().to_string()));
    vars.push(&mut env, "reveal", None);
    vars.push(&mut env, "on_error", None);
    vars.push(&mut env, "timeouts", None);
    vars.push(&mut env, "wait", None);
    Ok(env)
}

// ---- status ---------------------------------------------------------------------------------------------------

/// States of the planned status file, in order of progress. Anything else is reported as `unknown`.
pub const STATES: [&str; 11] = [
    "booting", "lobby", "creating-room", "waiting-for-players", "joining", "in-room", "loading-stage", "in-match", "results", "finished", "error",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MatchStatus {
    pub match_id: Option<String>,
    pub role: Option<String>,
    /// One of [`STATES`], or `unknown`.
    pub state: String,
    pub since_unix: Option<u64>,
    pub detail: Option<String>,
    /// `waiting-for-players`: players in the room (the host included) and wanted.
    pub present: Option<u32>,
    pub expected: Option<u32>,
    /// `error`: the runtime's stable code (see [`ERROR_CODES`]) and its English sentence.
    pub error: Option<String>,
    pub reason: Option<String>,
    /// The game window is shown.
    pub visible: Option<bool>,
    pub pid: Option<u32>,
    /// `file` (the status file) or `log` (derived from the run log).
    pub source: &'static str,
}

/// The error codes of the runtime's status and result files; the UI has a message and advice for each.
#[cfg(test)]
pub const ERROR_CODES: [&str; 18] = [
    "bad-settings", "boot-timeout", "network-unavailable", "list-timeout", "create-failed", "players-timeout", "room-not-found", "join-failed",
    "host-left", "player-left", "start-timeout", "stage-timeout", "match-aborted", "record-timeout",
    // 1.1.13: why the relay refused the player
    "relay-full", "relay-room-full", "relay-source-limit", "relay-source-full",
];

fn normalise_state(raw: &str) -> String {
    let s = raw.trim().to_ascii_lowercase().replace(['_', ' '], "-");
    if STATES.contains(&s.as_str()) { s } else { "unknown".into() }
}

/// `{match_id, role, state, since_unix, detail}`; tolerant of missing and extra keys. `None` when it is not JSON or has
/// no `state` (a half-written file must not break the screen).
pub fn parse_status(text: &str) -> Option<MatchStatus> {
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let state = v.get("state")?.as_str()?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).filter(|f| *f >= 0.0).map(|f| f as u32);
    Some(MatchStatus {
        match_id: s("match_id"),
        role: s("role"),
        state: normalise_state(state),
        since_unix: v.get("since_unix").and_then(|x| x.as_f64()).filter(|f| *f >= 0.0).map(|f| f as u64),
        detail: s("detail").filter(|d| !d.is_empty()),
        present: num("present"),
        expected: num("expected"),
        error: s("error").filter(|d| !d.is_empty()),
        reason: s("reason").filter(|d| !d.is_empty()),
        visible: v.get("visible").and_then(|x| x.as_bool()),
        pid: num("pid"),
        source: "file",
    })
}

/// The runtime logs its progress as `automatch: +<s> s <what>` (and `automatch: ... stopped: <why>` on failure).
/// Maps the newest recognised line to a state: the fallback when the status file does not exist.
pub fn status_from_log(log: &str) -> Option<MatchStatus> {
    let mut best: Option<(String, Option<String>)> = None;
    for line in log.lines() {
        let Some(at) = line.find("automatch:") else { continue };
        let msg = line[at + "automatch:".len()..].trim();
        // Drop the "+12.3 s " time stamp.
        let msg = match msg.strip_prefix('+') {
            Some(r) => r.split_once(" s ").map(|(_, m)| m).unwrap_or(r).trim(),
            None => msg,
        };
        let guest = best.as_ref().is_some_and(|(s, _)| s == "joining");
        let mut found: Option<(&str, Option<String>)> = None;
        let mut hit = |s: &'static str, d: Option<&str>| found = Some((s, d.map(str::to_string)));
        if let Some(why) = msg.strip_prefix("stopped:") {
            hit("error", Some(why.trim()));
        } else if msg.ends_with("; off") || msg.contains("; off") || msg.starts_with("cannot start") {
            hit("error", Some(msg));
        } else if msg.starts_with("result written") {
            hit("results", None);
        } else if msg.starts_with("closing the game") {
            hit("finished", None);
        } else if msg.starts_with("match record") {
            hit("results", None);
        } else if msg.starts_with("in stage") {
            hit("in-match", None);
        } else if msg.starts_with("stage loading") {
            hit("loading-stage", msg.split_once('(').map(|(_, r)| r.trim_end_matches(')')));
        } else if msg.starts_with("all players in") || msg.starts_with("teams ") || msg.starts_with("session joined") {
            hit("in-room", None);
        } else if msg.starts_with("a session is listed") {
            hit("joining", None);
        } else if msg.starts_with("session created") {
            hit("waiting-for-players", None);
        } else if msg.starts_with("session list open") || msg.starts_with("search ended") {
            if !guest {
                hit("creating-room", None);
            }
        } else if msg.starts_with("lobby") {
            hit("lobby", None);
        } else if msg.starts_with("on (") || msg.starts_with("host,") || msg.starts_with("join,") {
            hit("booting", None);
        }
        if let Some((s, d)) = found {
            best = Some((s.to_string(), d));
        }
    }
    best.map(|(state, detail)| MatchStatus { match_id: None, role: None, state, since_unix: None, detail, present: None, expected: None, error: None, reason: None, visible: None, pid: None, source: "log" })
}

// ---- result ---------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TeamResult {
    pub team: String,
    pub paint_points: u32,
    pub turf_permille: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlayerResult {
    pub name: String,
    #[serde(rename = "self")]
    pub is_self: bool,
    pub team: String,
    pub paint: u32,
    pub kills: u32,
    pub assists: u32,
    pub deaths: u32,
    pub specials: u32,
    pub disconnected: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MatchResult {
    pub version: u32,
    pub match_id: String,
    pub role: String,
    /// Set when the flow stopped without a match record: a code of [`ERROR_CODES`], with its sentence in `reason`.
    pub error: Option<String>,
    pub reason: Option<String>,
    pub mode: String,
    pub stage_id: Option<u32>,
    pub stage_scene: String,
    pub end_utc: u64,
    pub duration_s: f64,
    pub win_team: Option<u32>,
    pub winner: String,
    pub teams: Vec<TeamResult>,
    pub players: Vec<PlayerResult>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ResultView {
    #[serde(flatten)]
    pub result: MatchResult,
    /// `victory`, `defeat` or `unknown`, from the point of view of the local player (`self`).
    pub outcome: &'static str,
    pub my_team: Option<String>,
}

impl MatchResult {
    pub fn view(self) -> ResultView {
        let my_team = self.players.iter().find(|p| p.is_self).map(|p| p.team.clone()).filter(|t| !t.is_empty());
        let outcome = match (&my_team, self.winner.as_str()) {
            (Some(_), "") | (None, _) => "unknown",
            (Some(m), w) if m.eq_ignore_ascii_case(w) => "victory",
            _ => "defeat",
        };
        ResultView { result: self, outcome, my_team }
    }
}

/// Reads a result file. `Err` is a message for the screen; a result with an `error` field is still `Ok`.
pub fn parse_result(text: &str) -> Result<MatchResult, String> {
    let r: MatchResult = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("not a match result: {e}"))?;
    if r.match_id.is_empty() {
        return Err("not a match result: no match_id".into());
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn spec() -> MatchSpec {
        MatchSpec {
            relay_host: None,
            relay_ip: Ipv4Addr::new(203, 0, 113, 7),
            relay_port: 47313,
            room: "ABCDEFGHJKLMNPQRSTUVWXYZ".into(),
            round: 0,
            host: "Kay".into(),
            players: 4,
            mode: "turf".into(),
        }
    }

    fn paths() -> MatchPaths {
        match_paths(Path::new(r"C:\g\userdata"), "ABCDEFGHJKLMNPQRSTUVWXYZ")
    }

    fn get<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
        env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    #[test]
    #[cfg(windows)]
    fn host_environment_is_exactly_the_documented_one() {
        let env = build_match_env(&MatchVars::default(), Role::Host, &spec(), &paths(), "Kay").unwrap();
        let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names,
            ["SPLAT3_LAN", "SPLAT3_LAN_ROOM", "SPLAT3_LAN_NAME", "SPLAT3_START", "SPLAT3_MATCH", "SPLAT3_MATCH_ID", "SPLAT3_MATCH_RULES", "SPLAT3_MATCH_RESULT", "SPLAT3_MATCH_EXIT", "SPLAT3_MATCH_STATUS", "SPLAT3_MATCH_REVEAL", "SPLAT3_MATCH_ON_ERROR"]
        );
        assert_eq!(get(&env, "SPLAT3_LAN"), Some("relay:203.0.113.7")); // default port is implicit
        assert_eq!(get(&env, "SPLAT3_LAN_ROOM"), get(&env, "SPLAT3_MATCH_ID")); // the doc: same value in relay mode
        assert_eq!(get(&env, "SPLAT3_START"), Some("lobby-lan"));
        assert_eq!(get(&env, "SPLAT3_MATCH"), Some("host"));
        assert_eq!(get(&env, "SPLAT3_MATCH_RULES"), Some("players=4,teams=ABAB,mode=turf"));
        assert_eq!(get(&env, "SPLAT3_MATCH_EXIT"), Some("1"));
        assert_eq!(get(&env, "SPLAT3_MATCH_RESULT"), Some(r"C:\g\userdata\match\ABCDEFGHJKLMNPQRSTUVWXYZ\result.json"));
        assert_eq!(get(&env, "SPLAT3_MATCH_STATUS"), Some(r"C:\g\userdata\match\ABCDEFGHJKLMNPQRSTUVWXYZ\status.json"));
        assert_eq!(get(&env, "SPLAT3_MATCH_HOST"), None); // guest only
        assert_eq!(get(&env, "SPLAT3_MATCH_REVEAL"), Some("intro")); // hidden until the stage intro
        assert_eq!(get(&env, "SPLAT3_MATCH_ON_ERROR"), Some("exit"));
        assert_eq!(get(&env, "SPLAT3_MATCH_TIMEOUTS"), None); // the runtime's defaults
    }

    #[test]
    fn guest_names_the_host_and_sends_no_rules() {
        let s = MatchSpec { relay_port: 7000, round: 2, ..spec() };
        let env = build_match_env(&MatchVars::default(), Role::Join, &s, &paths(), "Zoe").unwrap();
        assert_eq!(get(&env, "SPLAT3_MATCH"), Some("join"));
        assert_eq!(get(&env, "SPLAT3_MATCH_HOST"), Some("Kay"));
        assert_eq!(get(&env, "SPLAT3_MATCH_RULES"), None);
        assert_eq!(get(&env, "SPLAT3_LAN"), Some("relay:203.0.113.7:7000"));
        assert_eq!(get(&env, "SPLAT3_LAN_ROOM"), Some("ABCDEFGHJKLMNPQRSTUVWXYZ-2"));
        assert_eq!(get(&env, "SPLAT3_MATCH_ID"), Some("ABCDEFGHJKLMNPQRSTUVWXYZ-2"));
        assert_eq!(get(&env, "SPLAT3_LAN_NAME"), Some("Zoe"));
    }

    #[test]
    fn team_strings_alternate_for_two_to_eight_players() {
        assert_eq!(alternating_teams(2), "AB");
        assert_eq!(alternating_teams(8), "ABABABAB");
        assert_eq!(rules_string(&MatchSpec { mode: "zones".into(), players: 3, ..spec() }), "players=3,teams=ABA,mode=zones");
    }

    #[test]
    fn bad_input_is_a_validation_error() {
        assert!(matches!(build_match_env(&MatchVars::default(), Role::Host, &spec(), &paths(), "  "), Err(AppError::Validation(_))));
        assert!(matches!(build_match_env(&MatchVars::default(), Role::Host, &MatchSpec { players: 12, ..spec() }, &paths(), "Kay"), Err(AppError::Validation(_))));
    }

    #[test]
    fn the_table_can_be_realigned_from_a_file() {
        let d = std::env::temp_dir().join(format!("s3l-mvars-{}", util::random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("match-vars.json"),
            r#"{"vars":[{"id":"status","name":"SPLAT3_MATCH_STATE_FILE","enabled":true},
                        {"id":"reveal","name":"SPLAT3_MATCH_REVEAL","value":"loading","enabled":true},
                        {"id":"exit","name":"lower","value":"1","enabled":true},
                        {"id":"nonsense","name":"X","enabled":true}]}"#,
        )
        .unwrap();
        let t = MatchVars::load(&d);
        let env = build_match_env(&t, Role::Host, &spec(), &paths(), "Kay").unwrap();
        assert!(get(&env, "SPLAT3_MATCH_STATE_FILE").is_some() && get(&env, "SPLAT3_MATCH_STATUS").is_none());
        assert_eq!(get(&env, "SPLAT3_MATCH_REVEAL"), Some("loading"));
        assert_eq!(get(&env, "SPLAT3_MATCH_EXIT"), Some("1")); // the bad name was refused, the default kept
        assert_eq!(t.vars.len(), MatchVars::default().vars.len());
        std::fs::write(d.join("match-vars.json"), "{ nope").unwrap();
        assert_eq!(MatchVars::load(&d), MatchVars::default());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn status_file_is_parsed_tolerantly() {
        let s = parse_status(r#"{"match_id":"M1","role":"host","state":"waiting_for_players","since_unix":1794342500,"detail":"1/2","extra":true}"#).unwrap();
        assert_eq!((s.state.as_str(), s.since_unix, s.detail.as_deref(), s.source), ("waiting-for-players", Some(1794342500), Some("1/2"), "file"));
        assert_eq!(parse_status(r#"{"state":"In-Match"}"#).unwrap().state, "in-match");
        assert_eq!(parse_status(r#"{"state":"something-new"}"#).unwrap().state, "unknown");
        assert_eq!(parse_status("\u{feff}{\"state\":\"error\",\"detail\":\"no lobby\"}").unwrap().detail.as_deref(), Some("no lobby"));
        assert!(parse_status("{\"state\":").is_none()); // half-written
        assert!(parse_status("{}").is_none());
        assert!(parse_status("[1]").is_none());
        for st in STATES {
            assert_eq!(parse_status(&format!("{{\"state\":\"{st}\"}}")).unwrap().state, st);
        }
    }

    #[test]
    fn the_runtime_status_examples_parse() {
        // A finished host's status file and the document's waiting example.
        let done = r#"{"version": 1, "match_id": "R3", "role": "host", "state": "finished", "since_unix": 1794346837.881, "detail": "closing the game", "visible": false, "pid": 38396}"#;
        let s = parse_status(done).unwrap();
        assert_eq!((s.state.as_str(), s.since_unix, s.visible, s.pid), ("finished", Some(1794346837), Some(false), Some(38396)));
        let wait = r#"{"version": 1, "match_id": "R3", "role": "host", "state": "waiting-for-players", "since_unix": 1794346577.120, "detail": "waiting for the players", "present": 1, "expected": 2, "visible": false, "pid": 38396}"#;
        let w = parse_status(wait).unwrap();
        assert_eq!((w.present, w.expected), (Some(1), Some(2)));
        let err = r#"{"version": 1, "match_id": "R4", "role": "join", "state": "error", "since_unix": 1.0, "detail": "no session answered", "error": "room-not-found", "reason": "no session answered the search", "visible": false, "pid": 5}"#;
        let e = parse_status(err).unwrap();
        assert_eq!((e.state.as_str(), e.error.as_deref(), e.reason.as_deref()), ("error", Some("room-not-found"), Some("no session answered the search")));
        assert!(ERROR_CODES.contains(&"room-not-found") && ERROR_CODES.len() == 18);
    }

    #[test]
    fn log_lines_give_a_state_when_there_is_no_status_file() {
        let log = "boot...\nautomatch: on (host, match M1, mode turf, result x, exit after the match)\nautomatch: host, match M1, 2 players, teams AB\n";
        assert_eq!(status_from_log(log).unwrap().state, "booting");
        let host = format!("{log}automatch: +7.9 s lobby loading\nautomatch: +12.1 s lobby network up\nautomatch: +18.3 s session list open\nautomatch: +21.3 s search ended\n");
        assert_eq!(status_from_log(&host).unwrap().state, "creating-room");
        let created = format!("{host}automatch: +23.6 s session created\n");
        assert_eq!(status_from_log(&created).unwrap().state, "waiting-for-players");
        let inroom = format!("{created}automatch: +29.1 s all players in\nautomatch: +42.2 s teams confirmed\n");
        assert_eq!(status_from_log(&inroom).unwrap().state, "in-room");
        let stage = format!("{inroom}automatch: +43.8 s stage loading (Stage_A)\n");
        let st = status_from_log(&stage).unwrap();
        assert_eq!((st.state.as_str(), st.detail.as_deref(), st.source), ("loading-stage", Some("Stage_A"), "log"));
        let play = format!("{stage}automatch: +55.8 s in stage (stage load + 12 s)\n");
        assert_eq!(status_from_log(&play).unwrap().state, "in-match");
        let rec = format!("{play}automatch: +244.8 s match record\nautomatch: result written to C:\\x\\result.json\n");
        assert_eq!(status_from_log(&rec).unwrap().state, "results");
        assert_eq!(status_from_log(&format!("{rec}automatch: closing the game (SPLAT3_MATCH_EXIT)\n")).unwrap().state, "finished");
        // A guest's search must not read as "creating a room".
        let guest = "automatch: join, match M1, 2 players\nautomatch: +8.3 s lobby loading\nautomatch: +21.7 s a session is listed\nautomatch: +21.7 s search ended\n";
        assert_eq!(status_from_log(guest).unwrap().state, "joining");
        assert_eq!(status_from_log("automatch: +30.0 s stopped: the local lobby never loaded\n").unwrap().state, "error");
        assert_eq!(status_from_log("automatch: SPLAT3_MATCH needs SPLAT3_START=lobby-lan; off\n").unwrap().state, "error");
        assert!(status_from_log("some other line\nanother\n").is_none());
        assert!(status_from_log("").is_none());
    }

    const EXAMPLE: &str = r#"{"match_id": "M42", "role": "host", "rule": 0, "mode": "turf", "stage_id": 11, "stage_scene": "Stage_A",
 "end_utc": 1794342745, "battle_frames": 10800, "duration_s": 180.00, "win_team": 0, "winner": "alpha",
 "deemed_result": -1,
 "teams": [{"team": "alpha", "paint_points": 43, "turf_permille": 19},
           {"team": "bravo", "paint_points": 17, "turf_permille": 7}, {"team": "charlie", "paint_points": 0, "turf_permille": 0}],
 "players": [{"name": "Alpha", "self": true, "team": "alpha", "paint": 43, "kills": 0, "assists": 0, "deaths": 1, "specials": 0, "disconnected": false},
             {"name": "Bravo", "self": false, "team": "bravo", "paint": 16, "kills": 1, "assists": 0, "deaths": 0, "specials": 0, "disconnected": false}],
 "t0_unix": 1794342500.623, "timeline_s": [["lobby loading", 7.9]]}"#;

    #[test]
    fn the_documented_result_example_parses() {
        let r = parse_result(EXAMPLE).unwrap();
        assert_eq!((r.match_id.as_str(), r.role.as_str(), r.winner.as_str(), r.win_team), ("M42", "host", "alpha", Some(0)));
        assert_eq!(r.teams.len(), 3);
        assert_eq!(r.teams[0], TeamResult { team: "alpha".into(), paint_points: 43, turf_permille: 19 });
        assert_eq!(r.players.len(), 2);
        assert!(r.players[0].is_self && !r.players[1].is_self);
        assert_eq!(r.players[1].kills, 1);
        assert!((r.duration_s - 180.0).abs() < 1e-9);
        let v = r.clone().view();
        assert_eq!((v.outcome, v.my_team.as_deref()), ("victory", Some("alpha")));
        // The guest of that match lost.
        let mut guest = r.clone();
        guest.players[0].is_self = false;
        guest.players[1].is_self = true;
        assert_eq!(guest.view().outcome, "defeat");
        // No local player flagged: no verdict.
        let mut none = r;
        none.players.iter_mut().for_each(|p| p.is_self = false);
        assert_eq!(none.view().outcome, "unknown");
    }

    #[test]
    fn an_error_result_and_junk_are_handled() {
        let r = parse_result(r#"{"version":1,"match_id":"M1","role":"join","error":"boot-timeout","reason":"the local lobby never loaded","t0_unix":1.5,"timeline_s":[]}"#).unwrap();
        assert_eq!((r.error.as_deref(), r.reason.as_deref()), (Some("boot-timeout"), Some("the local lobby never loaded")));
        assert!(r.players.is_empty() && r.view().outcome == "unknown");
        assert!(parse_result("").is_err());
        assert!(parse_result("{\"players\":[]}").is_err());
        assert!(parse_result("{\"match_id\":\"M\",\"players\":[{\"paint\":\"many\"}]}").is_err());
    }
}
