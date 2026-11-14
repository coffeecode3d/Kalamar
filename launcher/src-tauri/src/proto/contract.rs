//! Match contract `lan-auto-match`, versions 1 (`match-contract.md`) and 2 (`match-contract-2.md`): from a match object
//! to the game's start settings, and what the launcher reports back. The server's values are applied as given; `game.rules` in particular is passed
//! to the game verbatim (its keys are the runtime's business and evolve with it), only checked to be a safe
//! environment value. A match this launcher cannot apply exactly is refused, never adapted.
//!
//! | variable | value | for |
//! |---|---|---|
//! | `SPLAT3_START` | `lobby-lan` | everyone |
//! | `SPLAT3_LAN` | `relay:<IPv4 resolved by the launcher>[:<port>]` | everyone |
//! | `SPLAT3_LAN_ROOM` | `game.relay.room` | everyone |
//! | `SPLAT3_LAN_NAME`, `SPLAT3_NICKNAME` | `you.member_name` | everyone |
//! | `SPLAT3_MATCH` | `host` / `join` (`you.role`) | everyone |
//! | `SPLAT3_MATCH_ID` | `game.match_id` (= `relay.room` in version 1) | everyone |
//! | `SPLAT3_MATCH_RULES` | `game.rules`, verbatim | host |
//! | `SPLAT3_MATCH_HOST` | `game.host_name` | guests |
//! | `SPLAT3_MATCH_STATUS`, `SPLAT3_MATCH_RESULT` | files in the launcher's data folder | everyone |
//! | `SPLAT3_MATCH_EXIT=1`, `SPLAT3_MATCH_ON_ERROR=exit`, `SPLAT3_MATCH_REVEAL=intro` | | everyone |
//! | `SPLAT3_MATCH_TIMEOUTS` | `game.timeouts`, when not `null` | everyone |
//!
//! Version 2 differs: `SPLAT3_MATCH_RULES` goes to every participant, `SPLAT3_MATCH_ID` is `game.match_id` (it may
//! differ from the room, which stays the same across the rounds of one game session), the status and result paths
//! carry `{id}` (the engine puts the round's match id there), and with `game.keep_open` (and an engine that declared
//! rounds) `SPLAT3_MATCH_CMD` names a command file instead of `SPLAT3_MATCH_EXIT=1`. Which versions can be run comes
//! from the engine's capability list (`engine.rs`): version 2 only with an engine that lists them.
//!
//! Variable names come from the auto-match table (`automatch::MatchVars`, overridable by `match-vars.json`), so the
//! private match by code and the protocol's matches always speak the same names to the runtime.

use crate::automatch::{MatchVars, Role};
use crate::error::{AppError, AppResult, FieldError};
use crate::lan::{self, LanMode, LanRequest};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

pub const CONTRACT_ID: &str = "lan-auto-match";
/// Every version this launcher implements; the ones it can run depend on the engine (`engine::Caps`).
pub const CONTRACT_VERSIONS: &[u32] = &[1, 2];
/// The mode keys the runtime's automatic match takes (`SPLAT3_MATCH_RULES` `mode=`, auto-match.md): the private room's
/// mode selector, in its order. Only `turf` has been measured end to end.
pub const MODES: &[&str] = &["turf", "zones", "tower", "rainmaker", "clams"];
pub const MAX_PLAYERS: u64 = 8;
pub const MAX_RULES_BYTES: usize = 1024;

/// Launcher error codes (`match-contract.md` 4).
pub const ERR_FAILED: &str = "launcher-failed";
pub const ERR_CRASHED: &str = "launcher-crashed";
pub const ERR_CANCELLED: &str = "launcher-cancelled";
pub const ERR_RELAY: &str = "launcher-relay-unresolved";
pub const ERR_RULES: &str = "launcher-unsupported-rules";
/// Contract 2: the engine lacks a capability the match needs (another contract version, a rule key...).
pub const ERR_INCOMPATIBLE: &str = "launcher-incompatible";
/// Contract 2: a loadout constraint this launcher cannot apply.
pub const ERR_LOADOUT: &str = "launcher-loadout-unavailable";
/// 1.1.7: a spectator's game was not started: the match had started (the engine cannot let a late spectator in).
pub const ERR_SPECTATOR_LATE: &str = "launcher-spectator-late";
/// The engine's error code for a player who left the match (`auto-match.md`, "Leave match"); servers count it as
/// leaving a queue match. The launcher sends it too when the game was closed during the match.
pub const LEFT_BY_PLAYER: &str = "left-by-player";
/// 1.1.13: the engine's codes for a relay that refused the player (`auto-match.md`, "Status file"): they say why the
/// match never began, so they go to the server as they are whatever the contract (never as `launcher-crashed`, which
/// counts as an error in the match).
pub const RELAY_REFUSALS: &[&str] = &["relay-full", "relay-room-full", "relay-source-limit", "relay-source-full"];

/// Whether the engine's error `code` goes to the server as it is under contract `version`: every code under contract
/// 2; under contract 1 a departure (`left-by-player`) and a relay's refusal.
pub fn engine_code_passes(version: u32, code: &str) -> bool {
    version >= 2 || code == LEFT_BY_PLAYER || RELAY_REFUSALS.contains(&code)
}

/// 1.1.7: game states of a player that say the match has started (its stage loads, or later).
pub const STARTED_STATES: &[&str] = &["loading-stage", "in-match", "results", "finished", "waiting-command"];

/// Everything the launcher takes from a match object to start the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GamePlan {
    /// The service's match id (`id`), for the endpoints.
    pub id: String,
    /// `game.match_id`: the game's match id, the relay room in version 1.
    pub match_id: String,
    pub role: Role,
    pub member_name: String,
    pub relay_host: String,
    pub relay_port: u16,
    pub room: String,
    pub players: u64,
    pub rules: String,
    pub host_name: String,
    pub timeouts: Option<String>,
    pub mode: String,
    /// The contract version of this match (1 or 2).
    pub version: u32,
    /// Version 2: keep the game open after the result, for the next round (`SPLAT3_MATCH_CMD`).
    pub keep_open: bool,
    /// Version 2: the match whose game session this one continues.
    pub continue_from: Option<String>,
    /// This player's loadout constraints (`rules.loadout`), slot by slot.
    pub loadout: Vec<(String, Constraint)>,
    /// 1.1.7: this player watches from the stands (`you.team: spectator`): no loadout, the launch is reported before
    /// the game starts.
    pub spectator: bool,
}

/// One slot's loadout constraint (`match-contract-2.md` 5); ids are game identifiers, compared as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Constraint {
    Fixed(String),
    Allowed(Vec<String>),
    Denied(Vec<String>),
}

impl Constraint {
    pub fn admits(&self, id: &str) -> bool {
        match self {
            Constraint::Fixed(f) => f == id,
            Constraint::Allowed(a) => a.iter().any(|x| x == id),
            Constraint::Denied(d) => !d.iter().any(|x| x == id),
        }
    }
}

/// The loadout slots, in the protocol's names.
pub const SLOTS: [&str; 4] = ["weapon", "gear.head", "gear.clothes", "gear.shoes"];

fn id_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if (1..=64).contains(&s.len()) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn constraint(v: &Value) -> Option<Constraint> {
    let list = |k: &str| v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(id_text).take(512).collect::<Vec<_>>());
    if let Some(f) = v.get("fixed").and_then(id_text) {
        return Some(Constraint::Fixed(f));
    }
    if let Some(a) = list("allowed") {
        return Some(Constraint::Allowed(a));
    }
    list("denied").map(Constraint::Denied)
}

/// This player's constraints: `players` (by member name) over `teams` over `all`, slot by slot.
pub fn loadout_for(m: &Value, member: &str, team: &str) -> Vec<(String, Constraint)> {
    let Some(l) = m.pointer("/rules/loadout").filter(|l| l.is_object()) else { return Vec::new() };
    let blocks = [l.pointer(&format!("/players/{}", member.replace('~', "~0").replace('/', "~1"))), l.pointer(&format!("/teams/{team}")), l.get("all")];
    SLOTS
        .iter()
        .filter_map(|slot| {
            let path: Vec<&str> = slot.split('.').collect();
            blocks.iter().flatten().find_map(|b| path.iter().try_fold(*b, |v, k| v.get(*k)).and_then(constraint)).map(|c| (slot.to_string(), c))
        })
        .collect()
}

/// The ids to impose, slot by slot: a fixed id; for a choice, the player's pick when it complies, else the current
/// item (the launcher's profile) when it complies, else the first allowed id (`last_call`, the launch can wait no more).
/// Returns the chosen ids and the slots still waiting for the player's pick.
pub fn resolve_loadout(cons: &[(String, Constraint)], picks: &HashMap<String, String>, current: &HashMap<String, String>, last_call: bool) -> (Vec<(String, String)>, Vec<String>) {
    let (mut chosen, mut open) = (Vec::new(), Vec::new());
    for (slot, c) in cons {
        let ok = |id: Option<&String>| id.filter(|i| c.admits(i)).cloned();
        let pick = match c {
            Constraint::Fixed(f) => Some(f.clone()),
            _ => ok(picks.get(slot)).or_else(|| ok(current.get(slot))).or_else(|| match c {
                Constraint::Allowed(a) if last_call => a.first().cloned(),
                _ => None,
            }),
        };
        match pick {
            Some(id) => chosen.push((slot.clone(), id)),
            None if !last_call => open.push(slot.clone()),
            None => {}
        }
    }
    (chosen, open)
}

/// The engine's `SPLAT3_MATCH_LOADOUT` file (a save profile naming only `weapon` and `gear`; numeric ids as numbers).
pub fn loadout_profile(chosen: &[(String, String)]) -> Value {
    let id = |s: &str| s.parse::<i64>().map(Value::from).unwrap_or_else(|_| Value::from(s));
    let mut v = json!({"version": 1});
    for (slot, x) in chosen {
        match slot.split_once('.') {
            Some(("gear", part)) => {
                if v.get("gear").is_none() {
                    v["gear"] = json!({});
                }
                v["gear"][part] = id(x);
            }
            _ => v[slot.as_str()] = id(x),
        }
    }
    v
}

/// Why a match is refused: the report's `reason` (English, for the server's log) and the code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub code: &'static str,
    pub reason: String,
}

fn refuse(reason: impl Into<String>) -> Refusal {
    Refusal { code: ERR_RULES, reason: reason.into() }
}

fn incompatible(reason: impl Into<String>) -> Refusal {
    Refusal { code: ERR_INCOMPATIBLE, reason: reason.into() }
}

/// A member name as the contract guarantees it: 1 to 10 Unicode scalar values, at most 32 bytes, no control character,
/// none of `, : = "`, no leading or trailing space.
pub fn valid_member_name(s: &str) -> bool {
    let n = s.chars().count();
    (1..=10).contains(&n)
        && s.len() <= 32
        && !s.chars().any(|c| crate::util::unsafe_char(c) || matches!(c, ',' | ':' | '=' | '"'))
        && !s.starts_with(' ')
        && !s.ends_with(' ')
}

fn upper_code(s: &str, extra: &[u8]) -> bool {
    (1..=32).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || extra.contains(&b))
}

fn any_id(s: &str, max: usize) -> bool {
    (1..=max).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The checks of `match-contract.md` 3, step 1 (and `match-contract-2.md` 3), plus the shape of every value the game
/// receives. `caps`: what this launcher and its engine can run (`caps.loadout`: the constraint kinds it applies).
pub fn plan(m: &Value, caps: &crate::proto::engine::Caps) -> Result<GamePlan, Refusal> {
    let s = |p: &str| m.pointer(p).and_then(Value::as_str);
    let id = s("/id").filter(|x| crate::proto::meta::valid_id(x)).ok_or_else(|| refuse("the match has no valid id"))?.to_string();
    let g = m.get("game").filter(|g| g.is_object()).ok_or_else(|| refuse("the match has no game block"))?;
    let gid = g.pointer("/contract/id").and_then(Value::as_str).unwrap_or("");
    let gver = g.pointer("/contract/version").and_then(Value::as_u64).unwrap_or(0);
    if gid != CONTRACT_ID || !CONTRACT_VERSIONS.contains(&(gver as u32)) || !caps.contract_versions.contains(&(gver as u32)) {
        let have: Vec<String> = caps.contract_versions.iter().map(u32::to_string).collect();
        return Err(incompatible(format!("match contract {gid} version {gver}: this launcher and its engine run {CONTRACT_ID} {}", have.join(", "))));
    }
    let version = gver as u32;
    let gs = |k: &str| g.get(k).and_then(Value::as_str);
    let match_id = if version >= 2 {
        gs("match_id").filter(|x| any_id(x, 64)).ok_or_else(|| refuse("game.match_id: 1 to 64 of A-Z a-z 0-9 - _"))?
    } else {
        gs("match_id").filter(|x| upper_code(x, b"")).ok_or_else(|| refuse("game.match_id: 1 to 32 of A-Z 0-9"))?
    }
    .to_string();
    let relay = g.get("relay").ok_or_else(|| refuse("game.relay missing"))?;
    let relay_host = relay.get("host").and_then(Value::as_str).ok_or_else(|| refuse("game.relay.host missing"))?;
    crate::servers::parse_host(relay_host).map_err(|e| refuse(format!("game.relay.host: {}", e.message())))?;
    let relay_port = relay.get("port").and_then(Value::as_u64).filter(|p| (1..=65535).contains(p)).ok_or_else(|| refuse("game.relay.port: 1 to 65535"))? as u16;
    let room = relay.get("room").and_then(Value::as_str).filter(|x| upper_code(x, b"-_")).ok_or_else(|| refuse("game.relay.room: 1 to 32 of A-Z 0-9 - _"))?.to_string();
    // 1.1.7: `players` counts the spectators too (at most the engine's places).
    let spectator = s("/you/team") == Some("spectator") || s("/you/seat") == Some("spectator");
    let watchers = m.get("spectators").and_then(Value::as_array).map(|a| a.iter().filter(|w| w.get("left").and_then(Value::as_bool) != Some(true)).count() as u64).unwrap_or(0).max(u64::from(spectator));
    if watchers > caps.spectator_max {
        return Err(incompatible(format!("the match has {watchers} spectator(s): this engine has {} place(s) for spectators", caps.spectator_max)));
    }
    let max = caps.max_players.min(MAX_PLAYERS) + watchers;
    let players = g.get("players").and_then(Value::as_u64).filter(|n| (2..=max).contains(n)).ok_or_else(|| incompatible(format!("game.players: 2 to {max} (this engine's limit)")))?;
    let rules = gs("rules").ok_or_else(|| refuse("game.rules missing"))?;
    if rules.is_empty() || rules.len() > MAX_RULES_BYTES || rules.chars().any(crate::util::unsafe_char) {
        return Err(refuse("game.rules: 1 to 1024 bytes without control characters"));
    }
    let host_name = gs("host_name").filter(|x| valid_member_name(x)).ok_or_else(|| refuse("game.host_name is not a valid member name"))?.to_string();
    let timeouts = match g.get("timeouts") {
        None | Some(Value::Null) => None,
        Some(Value::String(t)) if !t.is_empty() && t.len() <= 256 && !t.chars().any(crate::util::unsafe_char) => Some(t.clone()),
        _ => return Err(refuse("game.timeouts: null or a short string")),
    };
    let role = match s("/you/role") {
        Some("host") => Role::Host,
        Some("join") => Role::Join,
        other => return Err(refuse(format!("you.role {other:?} is neither host nor join"))),
    };
    let member_name = s("/you/member_name").filter(|x| valid_member_name(x)).ok_or_else(|| refuse("you.member_name is not a valid member name"))?.to_string();
    let mode = s("/rules/mode").unwrap_or("").to_string();
    if !caps.modes.contains(&mode) {
        return Err(incompatible(format!("mode {mode:?} is not one this engine can play ({})", caps.modes.join(", "))));
    }
    let stage_set = !m.pointer("/rules/stage").is_none_or(Value::is_null);
    if stage_set && version < 2 {
        return Err(refuse("this game build cannot be told a stage (contract version 1): rules.stage must be null"));
    }
    if stage_set && role == Role::Host && !caps.rule_keys.iter().any(|k| k == "stage") {
        return Err(incompatible("this engine cannot be told a stage (rule key stage)"));
    }
    let team = s("/you/team").unwrap_or("");
    // A spectator plays nothing: no loadout is ever imposed on it.
    let constraints = if spectator { Vec::new() } else { loadout_for(m, &member_name, team) };
    if let Some((slot, _)) = constraints.iter().find(|(slot, _)| !caps.loadout.iter().any(|k| slot == k || slot.starts_with(&format!("{k}.")))) {
        return Err(Refusal { code: ERR_LOADOUT, reason: format!("this launcher and its engine cannot apply a {slot} constraint (rules.loadout)") });
    }
    if role == Role::Join && host_name == member_name {
        return Err(refuse("a guest cannot be the host"));
    }
    let keep_open = version >= 2 && g.get("keep_open").and_then(Value::as_bool) == Some(true) && caps.rounds;
    let continue_from = if version >= 2 { gs("continue_from").filter(|x| any_id(x, 64)).map(str::to_string) } else { None };
    Ok(GamePlan { id, match_id, role, member_name, relay_host: relay_host.to_ascii_lowercase(), relay_port, room, players, rules: rules.to_string(), host_name, timeouts, mode, version, keep_open, continue_from, loadout: constraints, spectator })
}

/// Where the game writes: a folder of the launcher's own, one per game session (one match in version 1). In version 2
/// the paths carry `{id}`, which the engine replaces with each round's match id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Paths {
    pub dir: PathBuf,
    pub status: PathBuf,
    pub result: PathBuf,
    /// Version 2 with `keep_open`: the command file (`next` / `quit` lines).
    pub cmd: Option<PathBuf>,
}

impl Paths {
    /// A path of the session with the round's id put in.
    pub fn of(path: &Path, match_id: &str) -> PathBuf {
        PathBuf::from(path.display().to_string().replace("{id}", match_id))
    }
    pub fn status_of(&self, match_id: &str) -> PathBuf {
        Paths::of(&self.status, match_id)
    }
    pub fn result_of(&self, match_id: &str) -> PathBuf {
        Paths::of(&self.result, match_id)
    }
}

pub fn paths(server_dir: &Path, plan: &GamePlan) -> Paths {
    if plan.version >= 2 {
        let dir = server_dir.join("matches").join(&plan.room);
        return Paths { status: dir.join("status-{id}.json"), result: dir.join("result-{id}.json"), cmd: plan.keep_open.then(|| dir.join("commands.txt")), dir };
    }
    let dir = server_dir.join("matches").join(&plan.match_id);
    Paths { status: dir.join("status.json"), result: dir.join("result.json"), cmd: None, dir }
}

/// A line of the command file: the next round (`next id=<id> <rules>` for the host, `next id=<id>` for a guest).
pub fn next_command(plan: &GamePlan) -> String {
    match plan.role {
        Role::Host => format!("next id={} {}", plan.match_id, plan.rules),
        Role::Join => format!("next id={}", plan.match_id),
    }
}

fn var_name(vars: &MatchVars, id: &str, fallback: &str) -> String {
    vars.vars.iter().find(|v| v.id == id).map(|v| v.name.clone()).unwrap_or_else(|| fallback.to_string())
}

/// The game's environment for this match, in a stable order. `relay_ip` is `game.relay.host` resolved by the launcher
/// (the game never resolves names). Every other `SPLAT3_MATCH*`/`SPLAT3_LAN*` variable is removed by the launch itself.
pub fn build_env(vars: &MatchVars, plan: &GamePlan, relay_ip: Ipv4Addr, p: &Paths) -> AppResult<Vec<(String, String)>> {
    let req = LanRequest {
        mode: LanMode::Relay,
        relay_host: relay_ip.to_string(),
        relay_port: plan.relay_port,
        room: plan.room.clone(),
        net_log: false,
        bind_any: false,
        lan_address: String::new(),
    };
    let mut env = lan::build_lan_env(&req, &plan.member_name)?;
    // Fixed values of the contract, whatever a local override of the table says.
    let mut put = |id: &str, fallback: &str, v: String| env.push((var_name(vars, id, fallback), v));
    put("start", "SPLAT3_START", "lobby-lan".into());
    put("role", "SPLAT3_MATCH", plan.role.word().into());
    put("id", "SPLAT3_MATCH_ID", plan.match_id.clone());
    // Version 1: the rules to the host only; version 2: to every participant (room keys act on the host only).
    if plan.role == Role::Host || plan.version >= 2 {
        put("rules", "SPLAT3_MATCH_RULES", plan.rules.clone());
    }
    if plan.role == Role::Join {
        put("host", "SPLAT3_MATCH_HOST", plan.host_name.clone());
    }
    put("status", "SPLAT3_MATCH_STATUS", p.status.display().to_string());
    put("result", "SPLAT3_MATCH_RESULT", p.result.display().to_string());
    match &p.cmd {
        Some(cmd) => put("cmd", "SPLAT3_MATCH_CMD", cmd.display().to_string()),
        None => put("exit", "SPLAT3_MATCH_EXIT", "1".into()),
    }
    put("on_error", "SPLAT3_MATCH_ON_ERROR", "exit".into());
    put("reveal", "SPLAT3_MATCH_REVEAL", vars.vars.iter().find(|v| v.id == "reveal").and_then(|v| v.value.clone()).filter(|v| v == "intro" || v == "loading").unwrap_or_else(|| "intro".into()));
    if let Some(t) = &plan.timeouts {
        put("timeouts", "SPLAT3_MATCH_TIMEOUTS", t.clone());
    }
    env.push(("SPLAT3_NICKNAME".into(), plan.member_name.clone()));
    if env.iter().any(|(k, v)| k.is_empty() || v.contains('\0')) {
        return Err(AppError::Validation(vec![FieldError { key: "match".into(), message: "unsafe environment value".into() }]));
    }
    Ok(env)
}

/// A launcher error report (`match-contract.md` 4, `match-contract-2.md` 7): uploaded when the game wrote no result
/// file. `code` is a `launcher-*` code, or (version 2) the status file's error code.
pub fn error_report(version: u32, match_id: &str, code: &str, reason: &str) -> Value {
    json!({"version": version.max(1), "match_id": match_id, "error": code, "reason": reason.chars().take(500).collect::<String>()})
}

/// Protocol 1.1.11 (`leaving.md`): the game's `in_play` from a status file whose error is `left-by-player`: `true` when
/// the player left the match while it was being played (the game heard every other player until then), `false` before
/// the match or with another player already gone. The only proof of a departure a server's `leave` policy accepts:
/// the report carries it as the game wrote it, never a guess of the launcher's.
pub fn in_play(status: &Value) -> Option<bool> {
    if status.get("error").and_then(Value::as_str) != Some(LEFT_BY_PLAYER) {
        return None;
    }
    status.get("in_play").and_then(Value::as_bool)
}

/// [`error_report`] with the game's `in_play` for a departure (`left-by-player` only; other codes never carry it).
pub fn error_report_in_play(version: u32, match_id: &str, code: &str, reason: &str, in_play: Option<bool>) -> Value {
    let mut r = error_report(version, match_id, code, reason);
    if let (Some(b), true) = (in_play, code == LEFT_BY_PLAYER) {
        r["in_play"] = json!(b);
    }
    r
}

/// The game's status file, as uploaded: a JSON object with a string `state` (else it is not sent).
pub fn status_state(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    v.as_object()?.get("state")?.as_str().map(str::to_string)
}

/// The game's result file is ready to upload: a JSON object (a record or an error) of at most `max` bytes.
pub fn result_ready(text: &str, max: usize) -> bool {
    text.len() <= max && serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).is_ok_and(|v| v.is_object())
}

/// This player's own entry of a match object: its line in `participants` (a player) or `spectators` (1.1.7), by
/// `you.user_id`.
pub fn own_entry(m: &Value) -> Option<&Value> {
    let me = m.pointer("/you/user_id")?;
    ["participants", "spectators"]
        .iter()
        .filter_map(|k| m.get(*k).and_then(Value::as_array))
        .flatten()
        .find(|p| p.pointer("/user/id") == Some(me))
}

/// What a launcher that starts (again) finds of its own part in a running match: nothing done yet (the game is
/// started as usual), a game that already reached the stage (`Started`: started again, the engine could not join a
/// match in progress), or a report already sent (`Reported`: nothing left to do but wait for the decision).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnPart {
    Fresh,
    Started,
    Reported,
}

pub fn own_part(m: &Value) -> OwnPart {
    let Some(e) = own_entry(m) else { return OwnPart::Fresh };
    if e.get("reported").and_then(Value::as_bool) == Some(true) {
        OwnPart::Reported
    } else if e.pointer("/status/state").and_then(Value::as_str).is_some_and(|s| STARTED_STATES.contains(&s)) {
        OwnPart::Started
    } else {
        OwnPart::Fresh
    }
}

/// The report for a game that ended without a result file, when the status file holds no error: the player left
/// (the game's own clean exit, code 0, or the launcher's stop, in the match: the window's close button or the menu's
/// "Quit to launcher" leave the match as its "Leave match" does, measured: exit code 0, no error in the status file,
/// the other game stopping 45 s later with `connection-lost`), cancelled (stopped before the lobby), or crashed.
pub fn ended_without_result(state: Option<&str>, exit_code: Option<i32>, stopped_by_user: bool) -> (&'static str, String) {
    let before_lobby = matches!(state, None | Some("booting"));
    let in_match = matches!(state, Some("loading-stage") | Some("in-match"));
    if stopped_by_user && before_lobby {
        (ERR_CANCELLED, "the player stopped the game before it reached its lobby".into())
    } else if in_match && (exit_code == Some(0) || stopped_by_user) {
        (LEFT_BY_PLAYER, "the player closed the game during the match (its window, Quit to launcher or the launcher's stop)".into())
    } else {
        let how = exit_code.map(|c| format!("with code {c}")).unwrap_or_else(|| "unexpectedly".into());
        (ERR_CRASHED, format!("the game exited {how} and wrote no result"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The participant view of the API's example (`api.md`, "Match object").
    fn example(role: &str) -> Value {
        let me = if role == "host" { ("u_a", "Camille", "alpha") } else { ("u_b", "Lou", "bravo") };
        json!({
            "id": "m_9x2k", "state": "ready", "origin": {"kind": "queue", "queue_id": "duel-ranked"},
            "rules": {"teams": [1, 1], "mode": "turf", "stage": null, "options": {}, "rated": true, "ladder": "duel", "season": "s3", "result_policy": "standard"},
            "participants": [],
            "you": {"user_id": me.0, "member_name": me.1, "team": me.2, "role": role},
            "game": {
                "contract": {"id": "lan-auto-match", "version": 1},
                "match_id": "K7Q2MXA9PL4DZB3W8R6TNY5HCF",
                "relay": {"host": "relay.example.org", "port": 47313, "room": "K7Q2MXA9PL4DZB3W8R6TNY5HCF"},
                "players": 2,
                "rules": "players=2,teams=Camille:A,Lou:B,mode=turf",
                "host_name": "Camille",
                "timeouts": null,
                "x-future": 1
            }
        })
    }

    fn get<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
        env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    /// Contract 1 and the five mode keys (an engine without a capability list).
    fn v1() -> crate::proto::engine::Caps {
        crate::proto::engine::caps(None)
    }

    /// An engine that lists contract 2 and rounds.
    fn v2() -> crate::proto::engine::Caps {
        crate::proto::engine::Caps {
            contract_versions: vec![1, 2],
            modes: MODES.iter().map(|s| s.to_string()).collect(),
            stages: vec!["S1".into()],
            rule_keys: ["players", "teams", "mode", "stage", "results_hold"].iter().map(|s| s.to_string()).collect(),
            rounds: true,
            max_players: 8,
            max_per_team: 4,
            uneven_teams: true,
            spectators: false,
            spectator_max: 0,
            spectator_modes: Vec::new(),
            spectator_verified: Vec::new(),
            spectator_host: false,
            listed: true,
            loadout: Vec::new(),
            rounds_faster: false,
            stage_list: "host_save".into(),
        }
    }

    fn plan(m: &Value, modes: &[&str]) -> Result<GamePlan, Refusal> {
        let mut c = v1();
        c.modes = modes.iter().map(|s| s.to_string()).collect();
        super::plan(m, &c)
    }

    /// A contract 2 match of a series: its own match id, the session's room, rounds.
    fn example2(role: &str) -> Value {
        let mut m = example(role);
        m["id"] = json!("m_round2");
        m["game"]["contract"]["version"] = json!(2);
        m["game"]["match_id"] = json!("m_round2");
        m["game"]["keep_open"] = json!(true);
        m["game"]["continue_from"] = json!("m_round1");
        m["game"]["rules"] = json!("players=2,teams=Camille:A,Lou:B,mode=turf,stage=S1,results_hold=15");
        m["rules"]["stage"] = json!("S1");
        m
    }

    #[test]
    fn a_spectator_is_seated_by_the_rules_without_a_loadout() {
        let mut m = example2("join");
        m["game"]["keep_open"] = json!(false);
        m["game"]["players"] = json!(3);
        m["game"]["rules"] = json!("players=3,teams=Camille:A,Lou:B,Cy:S,mode=turf,stage=S1,spectators_wait=60");
        m["you"] = json!({"user_id": "u_c", "member_name": "Cy", "team": "spectator", "seat": "spectator", "role": "join"});
        m["spectators"] = json!([{"user": {"id": "u_c"}, "member_name": "Cy", "role": "join", "left": false}]);
        m["rules"]["loadout"] = json!({"all": {"weapon": {"fixed": "40"}}});
        let mut c = v2();
        // An engine without places for spectators refuses, whatever its seat.
        assert_eq!(super::plan(&m, &c).unwrap_err().code, ERR_INCOMPATIBLE);
        c.spectator_max = 2;
        c.loadout = vec!["weapon".into()];
        let p = super::plan(&m, &c).unwrap();
        assert!(p.spectator && p.loadout.is_empty(), "a spectator plays nothing: no loadout");
        assert_eq!(p.players, 3);
        let env = build_env(&MatchVars::default(), &p, Ipv4Addr::new(127, 0, 0, 1), &paths(Path::new(r"C:\d"), &p)).unwrap();
        assert_eq!(get(&env, "SPLAT3_MATCH_RULES"), Some("players=3,teams=Camille:A,Lou:B,Cy:S,mode=turf,stage=S1,spectators_wait=60"));
        assert_eq!(get(&env, "SPLAT3_MATCH"), Some("join"));
        assert_eq!(get(&env, "SPLAT3_NICKNAME"), Some("Cy"));
        assert!(get(&env, "SPLAT3_MATCH_LOADOUT").is_none());
        // A player of the same match: its loadout applies, the spectator counts in players=.
        m["you"] = json!({"user_id": "u_b", "member_name": "Lou", "team": "bravo", "seat": "player", "role": "join"});
        let p = super::plan(&m, &c).unwrap();
        assert!(!p.spectator && p.loadout.len() == 1 && p.players == 3);
        c.spectator_max = 0;
        assert_eq!(super::plan(&m, &c).unwrap_err().code, ERR_INCOMPATIBLE, "an engine that cannot play with spectators present");
    }

    #[test]
    #[cfg(windows)]
    fn contract_2_rules_to_everyone_rounds_and_paths_per_round() {
        let d = Path::new(r"C:\d");
        let p = super::plan(&example2("join"), &v2()).unwrap();
        assert_eq!((p.version, p.keep_open, p.continue_from.as_deref(), p.match_id.as_str()), (2, true, Some("m_round1"), "m_round2"));
        let paths = paths(d, &p);
        let env = build_env(&MatchVars::default(), &p, Ipv4Addr::new(127, 0, 0, 1), &paths).unwrap();
        assert_eq!(get(&env, "SPLAT3_MATCH_RULES"), Some("players=2,teams=Camille:A,Lou:B,mode=turf,stage=S1,results_hold=15"), "a guest gets the rules too");
        assert_eq!(get(&env, "SPLAT3_MATCH_HOST"), Some("Camille"));
        assert_eq!(get(&env, "SPLAT3_MATCH_ID"), Some("m_round2"));
        assert_eq!(get(&env, "SPLAT3_LAN_ROOM"), Some("K7Q2MXA9PL4DZB3W8R6TNY5HCF"), "the session's room, not the match id");
        assert_eq!(get(&env, "SPLAT3_MATCH_EXIT"), None);
        assert_eq!(get(&env, "SPLAT3_MATCH_CMD"), Some(r"C:\d\matches\K7Q2MXA9PL4DZB3W8R6TNY5HCF\commands.txt"));
        assert_eq!(get(&env, "SPLAT3_MATCH_STATUS"), Some(r"C:\d\matches\K7Q2MXA9PL4DZB3W8R6TNY5HCF\status-{id}.json"));
        assert_eq!(paths.result_of("m_round2"), Path::new(r"C:\d\matches\K7Q2MXA9PL4DZB3W8R6TNY5HCF\result-m_round2.json"));
        assert_eq!(next_command(&p), "next id=m_round2");
        let h = super::plan(&example2("host"), &v2()).unwrap();
        assert_eq!(next_command(&h), "next id=m_round2 players=2,teams=Camille:A,Lou:B,mode=turf,stage=S1,results_hold=15");
        // an engine without rounds: the game closes after each match, as in version 1
        let mut no_rounds = v2();
        no_rounds.rounds = false;
        let p = super::plan(&example2("join"), &no_rounds).unwrap();
        assert!(!p.keep_open);
        let env = build_env(&MatchVars::default(), &p, Ipv4Addr::new(127, 0, 0, 1), &super::paths(d, &p)).unwrap();
        assert_eq!((get(&env, "SPLAT3_MATCH_EXIT"), get(&env, "SPLAT3_MATCH_CMD")), (Some("1"), None));
    }

    #[test]
    fn contract_2_refusals_name_what_the_engine_lacks() {
        // an engine without the list runs contract 1 only
        let e = super::plan(&example2("host"), &v1()).unwrap_err();
        assert_eq!(e.code, ERR_INCOMPATIBLE);
        // a stage for a host whose engine cannot be told one
        let mut c = v2();
        c.rule_keys.retain(|k| k != "stage");
        assert_eq!(super::plan(&example2("host"), &c).unwrap_err().code, ERR_INCOMPATIBLE);
        assert!(super::plan(&example2("join"), &c).is_ok(), "a guest's engine does not need it");
        // a loadout this launcher cannot apply
        let mut m = example2("join");
        m["rules"]["loadout"] = json!({"all": {"weapon": {"fixed": "1"}}});
        assert_eq!(super::plan(&m, &v2()).unwrap_err().code, ERR_LOADOUT, "an engine without match_loadout");
        let mut with = v2();
        with.loadout = vec!["weapon".into()];
        let p = super::plan(&m, &with).unwrap();
        assert_eq!(p.loadout, vec![("weapon".to_string(), Constraint::Fixed("1".into()))]);
        m["rules"]["loadout"]["teams"] = json!({"bravo": {"gear": {"head": {"allowed": ["7", 8]}}}});
        assert_eq!(super::plan(&m, &with).unwrap_err().code, ERR_LOADOUT, "gear is not applied by this engine");
        // more players than the engine
        let mut c = v2();
        c.max_players = 2;
        let mut m = example2("join");
        m["game"]["players"] = json!(4);
        assert_eq!(super::plan(&m, &c).unwrap_err().code, ERR_INCOMPATIBLE);
        // match ids of version 2: lower case and dashes allowed, at most 64
        let mut m = example2("join");
        m["game"]["match_id"] = json!("x".repeat(65));
        assert_eq!(super::plan(&m, &v2()).unwrap_err().code, ERR_RULES);
    }

    #[test]
    #[cfg(windows)]
    fn the_host_gets_the_rules_verbatim() {
        let p = plan(&example("host"), MODES).unwrap();
        assert_eq!((p.role, p.member_name.as_str(), p.players), (Role::Host, "Camille", 2));
        let paths = paths(Path::new(r"C:\data\online\0a1b2c3d"), &p);
        let env = build_env(&MatchVars::default(), &p, Ipv4Addr::new(198, 51, 100, 4), &paths).unwrap();
        let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names,
            ["SPLAT3_LAN", "SPLAT3_LAN_ROOM", "SPLAT3_LAN_NAME", "SPLAT3_START", "SPLAT3_MATCH", "SPLAT3_MATCH_ID", "SPLAT3_MATCH_RULES", "SPLAT3_MATCH_STATUS", "SPLAT3_MATCH_RESULT", "SPLAT3_MATCH_EXIT", "SPLAT3_MATCH_ON_ERROR", "SPLAT3_MATCH_REVEAL", "SPLAT3_NICKNAME"]
        );
        assert_eq!(get(&env, "SPLAT3_LAN"), Some("relay:198.51.100.4"), "the resolved IPv4, never the name");
        assert_eq!(get(&env, "SPLAT3_LAN_ROOM"), Some("K7Q2MXA9PL4DZB3W8R6TNY5HCF"));
        assert_eq!(get(&env, "SPLAT3_MATCH_ID"), Some("K7Q2MXA9PL4DZB3W8R6TNY5HCF"));
        assert_eq!(get(&env, "SPLAT3_MATCH_RULES"), Some("players=2,teams=Camille:A,Lou:B,mode=turf"));
        assert_eq!(get(&env, "SPLAT3_MATCH_HOST"), None);
        assert_eq!(get(&env, "SPLAT3_NICKNAME"), Some("Camille"));
        assert_eq!(get(&env, "SPLAT3_LAN_NAME"), Some("Camille"));
        assert_eq!(get(&env, "SPLAT3_MATCH_STATUS"), Some(r"C:\data\online\0a1b2c3d\matches\K7Q2MXA9PL4DZB3W8R6TNY5HCF\status.json"));
        assert_eq!(get(&env, "SPLAT3_MATCH_TIMEOUTS"), None);
    }

    #[test]
    fn a_guest_names_the_host_and_gets_no_rules() {
        let mut m = example("join");
        m["game"]["relay"]["port"] = json!(7000);
        m["game"]["timeouts"] = json!("boot=300,list=90");
        let p = plan(&m, MODES).unwrap();
        let env = build_env(&MatchVars::default(), &p, Ipv4Addr::new(127, 0, 0, 1), &paths(Path::new("d"), &p)).unwrap();
        assert_eq!(get(&env, "SPLAT3_MATCH"), Some("join"));
        assert_eq!(get(&env, "SPLAT3_MATCH_HOST"), Some("Camille"));
        assert_eq!(get(&env, "SPLAT3_MATCH_RULES"), None);
        assert_eq!(get(&env, "SPLAT3_LAN"), Some("relay:127.0.0.1:7000"));
        assert_eq!(get(&env, "SPLAT3_NICKNAME"), Some("Lou"));
        assert_eq!(get(&env, "SPLAT3_MATCH_TIMEOUTS"), Some("boot=300,list=90"));
    }

    #[test]
    fn rules_are_opaque_but_safe() {
        // A key this launcher does not know (a stage key of a later runtime) passes through unchanged.
        let mut m = example("host");
        m["game"]["rules"] = json!("players=2,teams=Camille:A,Lou:B,mode=turf,stage=Stage_Other,results_hold=15");
        assert_eq!(plan(&m, MODES).unwrap().rules, "players=2,teams=Camille:A,Lou:B,mode=turf,stage=Stage_Other,results_hold=15");
        m["game"]["rules"] = json!("players=2\nmode=turf");
        assert_eq!(plan(&m, MODES).unwrap_err().code, ERR_RULES);
        m["game"]["rules"] = json!("x".repeat(1025));
        assert!(plan(&m, MODES).is_err());
    }

    #[test]
    fn refusals() {
        let bad = |f: &dyn Fn(&mut Value)| {
            let mut m = example("host");
            f(&mut m);
            plan(&m, MODES).unwrap_err()
        };
        assert!(bad(&|m| m["game"]["contract"]["version"] = json!(3)).reason.contains("contract"));
        assert_eq!(bad(&|m| m["game"]["contract"]["version"] = json!(2)).code, ERR_INCOMPATIBLE, "version 2 needs an engine that lists it");
        assert!(bad(&|m| m["game"]["contract"]["id"] = json!("other")).reason.contains("contract"));
        assert!(bad(&|m| m["rules"]["mode"] = json!("mode_x")).reason.contains("mode"));
        assert!(bad(&|m| m["rules"]["stage"] = json!("Stage_A")).reason.contains("stage"));
        assert!(bad(&|m| m["game"]["players"] = json!(9)).reason.contains("players"));
        assert!(bad(&|m| m["game"]["match_id"] = json!("lower")).reason.contains("match_id"));
        assert!(bad(&|m| m["game"]["relay"]["room"] = json!("ROOM WITH SPACE")).reason.contains("room"));
        assert!(bad(&|m| m["game"]["relay"]["host"] = json!("2001:db8::1")).reason.contains("relay.host"));
        assert!(bad(&|m| m["you"]["member_name"] = json!("Way too long name")).reason.contains("member_name"));
        assert!(bad(&|m| m["you"]["member_name"] = json!("a,b")).reason.contains("member_name"));
        assert!(bad(&|m| m["you"]["role"] = json!("spectator")).reason.contains("role"));
        assert!(bad(&|m| { m.as_object_mut().unwrap().remove("game"); }).reason.contains("game"));
        assert_eq!(bad(&|m| m["rules"]["mode"] = json!("mode_x")).code, ERR_INCOMPATIBLE);
        // a mode the build lists is accepted
        let mut m = example("host");
        m["rules"]["mode"] = json!("zones");
        assert!(plan(&m, MODES).is_ok());
        assert!(plan(&m, &["turf"]).is_err());
    }

    #[test]
    fn loadout_constraints_resolve_by_precedence_pick_and_current() {
        let mut m = example2("join"); // Lou, team bravo
        m["rules"]["loadout"] = json!({
            "all": {"weapon": {"allowed": ["10", 20]}, "gear": {"shoes": {"denied": ["6000"]}}},
            "teams": {"bravo": {"weapon": {"allowed": ["30", "40"]}}},
            "players": {"Lou": {"gear": {"head": {"fixed": 1004}}}}
        });
        let cons = loadout_for(&m, "Lou", "bravo");
        assert_eq!(cons, vec![
            ("weapon".to_string(), Constraint::Allowed(vec!["30".into(), "40".into()])),
            ("gear.head".to_string(), Constraint::Fixed("1004".into())),
            ("gear.shoes".to_string(), Constraint::Denied(vec!["6000".into()])),
        ], "players over teams over all");
        let none = HashMap::new();
        let current: HashMap<String, String> = [("weapon".to_string(), "50".to_string()), ("gear.shoes".to_string(), "6001".to_string())].into();
        // The current shoes comply; the current weapon does not: the player must pick.
        let (chosen, open) = resolve_loadout(&cons, &none, &current, false);
        assert_eq!((chosen.clone(), open), (vec![("gear.head".to_string(), "1004".to_string()), ("gear.shoes".to_string(), "6001".to_string())], vec!["weapon".to_string()]));
        let picks: HashMap<String, String> = [("weapon".to_string(), "40".to_string())].into();
        assert_eq!(resolve_loadout(&cons, &picks, &current, false).1, Vec::<String>::new());
        // A pick that does not comply is ignored; at the last call the first allowed id is taken.
        let bad: HashMap<String, String> = [("weapon".to_string(), "10".to_string())].into();
        let (last, open) = resolve_loadout(&cons, &bad, &none, true);
        assert!(open.is_empty());
        assert_eq!(last[0], ("weapon".to_string(), "30".to_string()));
        assert_eq!(last.len(), 2, "denied shoes with nothing compliant known: left to the save");
        assert_eq!(loadout_profile(&last), json!({"version": 1, "weapon": 30, "gear": {"head": 1004}}));
    }

    #[test]
    fn member_names() {
        for ok in ["Camille", "Camille2", "Zoé", "a", "ten chars!"] {
            assert!(valid_member_name(ok), "{ok}");
        }
        for bad in ["", "eleven chars", " lead", "trail ", "a:b", "a=b", "a\"b", "a\u{1}b"] {
            assert!(!valid_member_name(bad), "{bad}");
        }
    }

    #[test]
    fn reports_and_files() {
        let r = error_report(1, "K7Q2", ERR_CRASHED, "the game exited with code 3 and no result");
        assert_eq!(r, json!({"version": 1, "match_id": "K7Q2", "error": "launcher-crashed", "reason": "the game exited with code 3 and no result"}));
        assert_eq!(error_report(2, "m_2", "room-not-found", "no session")["version"], 2);
        // 1.1.11: a departure's report copies the game's in_play, and only for left-by-player.
        let st = json!({"version": 1, "state": "error", "error": "left-by-player", "in_play": true, "session_silent_ms": 40});
        assert_eq!(in_play(&st), Some(true));
        assert_eq!(in_play(&json!({"state": "error", "error": "left-by-player"})), None, "an engine before 1.1.11");
        assert_eq!(in_play(&json!({"state": "error", "error": "connection-lost", "in_play": true})), None);
        let r = error_report_in_play(2, "m_2", LEFT_BY_PLAYER, "left", in_play(&st));
        assert_eq!(r, json!({"version": 2, "match_id": "m_2", "error": "left-by-player", "reason": "left", "in_play": true}));
        assert!(error_report_in_play(2, "m_2", LEFT_BY_PLAYER, "left", None).get("in_play").is_none());
        assert!(error_report_in_play(2, "m_2", "connection-lost", "lost", Some(true)).get("in_play").is_none());
        assert_eq!(status_state(r#"{"version":1,"state":"in-match"}"#).as_deref(), Some("in-match"));
        assert_eq!(status_state(r#"{"state":3}"#), None);
        assert_eq!(status_state(r#"{"state":"#), None);
        assert!(result_ready(r#"{"version":1,"match_id":"M","error":"boot-timeout"}"#, 65536));
        assert!(!result_ready(r#"[1]"#, 65536));
        assert!(!result_ready(r#"{"a":1}"#, 3));
    }

    /// The reports of the measured runs: a window closed in the match (exit code 0, no error in the status file), a
    /// game stopped by its PID (code 1), the launcher's stop before the lobby and in the match.
    #[test]
    fn a_game_closed_in_the_match_left_it() {
        assert_eq!(ended_without_result(Some("in-match"), Some(0), false).0, LEFT_BY_PLAYER);
        assert_eq!(ended_without_result(Some("loading-stage"), Some(0), false).0, LEFT_BY_PLAYER);
        assert_eq!(ended_without_result(Some("in-match"), Some(1), true).0, LEFT_BY_PLAYER);
        let (code, why) = ended_without_result(Some("in-match"), Some(1), false);
        assert_eq!((code, why.as_str()), (ERR_CRASHED, "the game exited with code 1 and wrote no result"));
        assert_eq!(ended_without_result(Some("in-match"), None, false).0, ERR_CRASHED);
        assert_eq!(ended_without_result(Some("booting"), Some(1), true).0, ERR_CANCELLED);
        assert_eq!(ended_without_result(None, None, true).0, ERR_CANCELLED);
        // Closed before the stage (the window is hidden then) or after the result: not a departure from the match.
        assert_eq!(ended_without_result(Some("in-room"), Some(0), false).0, ERR_CRASHED);
        assert_eq!(ended_without_result(Some("results"), Some(0), false).0, ERR_CRASHED);
    }

    /// 1.1.13: a relay's refusal reaches the server as the engine wrote it, under either contract; never as
    /// `launcher-crashed` (an error in the match) nor `connection-lost`.
    #[test]
    fn a_relays_refusal_goes_to_the_server_as_it_is() {
        for code in RELAY_REFUSALS {
            assert!(engine_code_passes(1, code) && engine_code_passes(2, code), "{code}");
            let r = error_report(1, "R1", code, "the relay refused this player");
            assert_eq!(r["error"], *code);
            assert_ne!(*code, ERR_CRASHED);
        }
        assert!(engine_code_passes(1, LEFT_BY_PLAYER) && engine_code_passes(2, "network-unavailable"));
        assert!(!engine_code_passes(1, "network-unavailable"), "contract 1 keeps its rule for the other codes");
    }

    #[test]
    fn own_part_of_a_running_match() {
        let m = |me: Value| {
            json!({"id": "m_1", "state": "running", "you": {"user_id": "u_b"},
                   "participants": [{"user": {"id": "u_a"}, "status": {"state": "in-match"}, "reported": true}, me],
                   "spectators": [{"user": {"id": "u_c"}, "status": {"state": "in-match"}, "reported": false}]})
        };
        assert_eq!(own_part(&m(json!({"user": {"id": "u_b"}, "status": null, "reported": false}))), OwnPart::Fresh);
        assert_eq!(own_part(&m(json!({"user": {"id": "u_b"}, "status": {"state": "in-room"}, "reported": false}))), OwnPart::Fresh);
        assert_eq!(own_part(&m(json!({"user": {"id": "u_b"}, "status": {"state": "in-match"}, "reported": false}))), OwnPart::Started);
        assert_eq!(own_part(&m(json!({"user": {"id": "u_b"}, "status": {"state": "error"}, "reported": true}))), OwnPart::Reported);
        let mut s = m(json!({"user": {"id": "u_b"}, "status": null, "reported": false}));
        s["you"]["user_id"] = json!("u_c");
        assert_eq!(own_part(&s), OwnPart::Started, "a spectator's own line");
        assert_eq!(own_entry(&json!({"participants": []})), None);
    }
}
