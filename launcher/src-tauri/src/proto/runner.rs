//! The match on this machine (`match-contract.md` 3 and 4, `match-contract-2.md` 4 to 7): when the player's match
//! reaches `ready`, start the game through the existing automatic-match launch, report the launch, mirror every change
//! of the game's status file, upload the result file verbatim (or an error report), and close a game whose match was
//! cancelled before it started. One match at a time; driven by the match events and by `GET /v1/matches/current`.
//!
//! **Rounds in one game session** (contract 2, `keep_open`): the game stays open after its result, waiting for a
//! command (`waiting-command`). A match whose `game.continue_from` is the round the running game just played is sent to
//! it (`next id=...` appended to the command file) instead of starting a new process; `quit` is appended when the
//! series ends, when another match comes, or when nothing continues in time. A launcher whose game is not running any
//! more simply launches the next match anew: `continue_from` is a hint.

use crate::proto::client::Client;
use crate::proto::consent::Ask;
use crate::proto::contract::{self, GamePlan, Paths};
use crate::proto::{discovery, relayping};
use crate::proto::engine::{self, EngineInfo};
use crate::proto::http::{Body, CallError};
use crate::servers::{self, Resolver};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The `code` of a match that stopped because the game the player had open could not be closed (the page shows its
/// own sentence for it; the server gets `launcher-failed` with the reason, as for any failed launch).
pub const CODE_NOT_CLOSED: &str = "game-not-closed";

/// The game process as the runner needs it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GameRun {
    pub running: bool,
    /// The launch of this match has ended (exit code when known).
    pub exited: bool,
    pub exit_code: Option<i32>,
    pub stopped_by_user: bool,
}

/// Starts and watches the game; the launcher's implementation goes through `commands::launch_protocol_match`.
pub trait GameHost: Send + Sync {
    /// Starts the game with these variables (on top of the launcher's usual ones). `Err`: a sentence for the player.
    fn launch(&self, env: Vec<(String, String)>) -> Result<(), String>;
    fn status(&self) -> GameRun;
    fn stop(&self);
    /// What the game that is open stands for (`launch::open_game`): the launcher's own match in progress, or a game
    /// started for solo play, which cannot play a match and is closed for it. `Nothing` by default.
    fn open_game(&self) -> crate::launch::OpenGame {
        crate::launch::OpenGame::Nothing
    }
    /// Closes the game opened for solo play: asks it to close, waits a few seconds, then terminates only that game.
    /// Blocking (call off the runtime). `Err`: a sentence for the player.
    fn close_open_game(&self) -> Result<(), String> {
        Ok(())
    }
    /// An opaque id of the game build (`PUT /v1/me/game`).
    fn build_id(&self) -> String;
    /// The installed game was built for another game version than this launcher's (`installer::gamever`): it is
    /// not declared to a server and takes part in no match; the player is asked for the game update instead.
    fn game_outdated(&self) -> bool {
        false
    }
    /// The engine's capability list (`engine.rs`); may run the engine's listing once (blocking: call off the runtime).
    fn engine(&self) -> Option<EngineInfo> {
        None
    }
    /// The launcher's data folder (the engine cache lives there).
    fn data_dir(&self) -> Option<PathBuf> {
        None
    }
    /// Announce rounds in one game session (`engine::game_body`); off by default.
    fn declare_rounds(&self) -> bool {
        false
    }
    /// The player's current weapon and gear as the launcher knows them (its profile), by loadout slot: a constraint
    /// that admits them keeps them.
    fn current_loadout(&self) -> HashMap<String, String> {
        HashMap::new()
    }
    /// The sub abilities the player chose in the Locker, by gear part (`head`, `clothes`, `shoes`), when the launcher
    /// applies its profile: a match loadout that names gear for a part carries them (`contract::loadout_profile`).
    fn current_sub_abilities(&self) -> HashMap<String, Vec<Option<String>>> {
        HashMap::new()
    }
    /// The main ability the player chose in the Locker, by gear part, when the launcher applies its profile: a match
    /// loadout that names gear for a part carries it (`contract::loadout_profile`).
    fn current_main_abilities(&self) -> HashMap<String, String> {
        HashMap::new()
    }
    /// Developer tools only (`proto_peer --loadout-rules`): loadout constraints handled as if the match carried them
    /// (`rules.loadout`), to try the launcher's loadout path with a server that imposes none. The launcher never sets it.
    fn local_loadout_rules(&self) -> Option<Value> {
        None
    }
}

/// Where the runner stands, for the page (`proto-runner` events).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RunnerStatus {
    pub match_id: String,
    /// `checking`, `confirm` (the launcher asks its player before the launch, `consent`), `resolving`, `closing` (the game the player had open for solo play is closed), `launching`, `continuing` (sent to the open game), `running`, `uploading`, `done`,
    /// `waiting` (the game stays open for the next round), `refused`, `failed`, `closed`.
    pub phase: String,
    /// The game's last state from its status file.
    pub game_state: Option<String>,
    /// The game's last status in full (state, since when, players present, window shown, step limit).
    pub game: Option<crate::proto::progress::GameProgress>,
    /// For `refused`/`failed`: the launcher code and its reason.
    pub code: Option<String>,
    pub reason: Option<String>,
    pub relay_ip: Option<String>,
    pub role: Option<String>,
    pub member_name: Option<String>,
    /// The environment given to the game (names and values; nothing secret is in it).
    pub env: Vec<(String, String)>,
    pub status_path: Option<String>,
    pub result_path: Option<String>,
    pub result_uploaded: bool,
    /// Contract version of the match.
    pub contract: u32,
    /// The game session continues after this match (`keep_open`).
    pub keep_open: bool,
    /// `choosing`: the loadout slots waiting for the player's pick, with their constraint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choices: Option<Value>,
    /// The weapon and gear imposed for this match (`SPLAT3_MATCH_LOADOUT`), slot by slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loadout: Option<Value>,
    /// The result's `loadout.ok`: this player's record holds what was imposed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loadout_ok: Option<bool>,
    /// 1.1.7: `spectator` when this player watches from the stands (its screens say so), else `player`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seat: Option<String>,
    /// `confirm`: why the launcher asks before it starts the game (`not-joined`, `too-many`: `consent::Ask`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask: Option<String>,
    /// `confirm`: the relay the match names (`host:port`), and how long the question waits (seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask_s: Option<u64>,
}

pub trait RunnerSink: Send + Sync {
    fn runner(&self, status: &RunnerStatus);
}

struct Active {
    id: String,
    /// The match ended without the player (void, cancelled): stop uploading, close a game that did not start.
    ended: bool,
    /// 1.1.7: the player left its place in the stands (the match no longer has `you`): its game closes.
    left: bool,
}

/// An open game session (contract 2 `keep_open`): what the next round needs.
#[derive(Debug, Clone)]
struct Session {
    room: String,
    paths: Paths,
    /// The last round the game played (its service match id).
    last: String,
    /// Used to tell the game to stop when nothing continues the session.
    series: Option<String>,
    /// The session's host and this player's role in it: a round can continue only with the same host (the engine's
    /// room is the host's), else the game is started anew.
    host_name: String,
    role: crate::automatch::Role,
}

pub struct Runner {
    client: Arc<Client>,
    host: Arc<dyn GameHost>,
    sink: Arc<dyn RunnerSink>,
    resolver: Arc<dyn Resolver + Send + Sync>,
    active: Mutex<Option<Active>>,
    handled: Mutex<HashSet<String>>,
    last: Mutex<Option<RunnerStatus>>,
    session: Mutex<Option<Session>>,
    /// The player's loadout picks, by match id (`pick_loadout`).
    picks: Mutex<HashMap<String, HashMap<String, String>>>,
    /// The player's answers to the launcher's question before a launch (`confirm`), by match id.
    answers: Mutex<HashMap<String, bool>>,
    /// The longest the question waits for an answer (the match's launch deadline comes first).
    pub ask_max: Duration,
    /// How often the status file is read.
    pub poll: Duration,
    /// How long an open game waits for a continuing match when the match gave no `continue_window_s` (1.1.1 default).
    pub idle_quit: Duration,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Appends a line to the game's command file (the engine reads lines appended after it started).
pub fn append_command(path: &Path, line: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")?;
    f.sync_all()
}

impl Runner {
    pub fn new(client: Arc<Client>, host: Arc<dyn GameHost>, sink: Arc<dyn RunnerSink>, resolver: Arc<dyn Resolver + Send + Sync>) -> Arc<Runner> {
        Arc::new(Runner {
            client,
            host,
            sink,
            resolver,
            active: Mutex::new(None),
            handled: Mutex::new(HashSet::new()),
            last: Mutex::new(None),
            session: Mutex::new(None),
            picks: Mutex::new(HashMap::new()),
            answers: Mutex::new(HashMap::new()),
            ask_max: Duration::from_secs(120),
            poll: Duration::from_millis(400),
            idle_quit: Duration::from_secs(90),
        })
    }

    pub fn last(&self) -> Option<RunnerStatus> {
        lock(&self.last).clone()
    }

    fn publish(&self, s: &RunnerStatus) {
        *lock(&self.last) = Some(s.clone());
        self.sink.runner(s);
    }

    fn ended(&self, id: &str) -> bool {
        lock(&self.active).as_ref().is_some_and(|a| a.id == id && a.ended)
    }

    /// The game session is still open (the process runs).
    fn session_alive(&self) -> bool {
        lock(&self.session).is_some() && self.host.status().running
    }

    /// The player's picks for the loadout slots of a match waiting in `choosing` (slot -> id).
    pub fn pick_loadout(&self, match_id: &str, picks: HashMap<String, String>) {
        lock(&self.picks).insert(match_id.to_string(), picks);
    }

    /// The player's answer to the question before the launch of a match (runner phase `confirm`).
    pub fn answer(&self, match_id: &str, start: bool) {
        lock(&self.answers).insert(match_id.to_string(), start);
    }

    /// Tells an open game to close (`quit`) and forgets the session.
    pub fn quit_session(&self, why: &str) {
        if let Some(s) = lock(&self.session).take()
            && let Some(cmd) = &s.paths.cmd
            && self.host.status().running
        {
            let _ = append_command(cmd, "quit");
            let _ = why;
        }
    }

    /// A match object from an event or from `GET /v1/matches/current`: start, or note that it ended.
    pub fn on_match(self: &Arc<Self>, m: &Value) {
        let Some(id) = m.get("id").and_then(Value::as_str).map(str::to_string) else { return };
        let state = m.get("state").and_then(Value::as_str).unwrap_or("");
        // 1.1.7: a spectator who left its place gets the match without `you`: its game (if any) closes.
        if m.get("you").is_none_or(Value::is_null) && !matches!(state, "void" | "cancelled" | "completed") {
            if let Some(a) = lock(&self.active).as_mut().filter(|a| a.id == id) {
                a.ended = true;
                a.left = true;
            }
            return;
        }
        match state {
            "ready" | "running" if m.get("game").is_some_and(Value::is_object) => {
                let fresh = lock(&self.handled).insert(id.clone());
                // One match at a time, an ended one included until its watch is done: a replacement match (a void
                // match's `rematch_id`, the next game of a series) starts once the previous game is closed or
                // stopped, so that closing it can never touch the new game.
                let busy = lock(&self.active).as_ref().is_some_and(|a| a.id != id);
                if fresh && !busy {
                    *lock(&self.active) = Some(Active { id: id.clone(), ended: false, left: false });
                    let me = self.clone();
                    let m = m.clone();
                    tokio::spawn(async move { me.run(m).await });
                } else if fresh && busy {
                    // Another match is still being watched: try again when it is done.
                    lock(&self.handled).remove(&id);
                    let me = self.clone();
                    let m = m.clone();
                    tokio::spawn(async move {
                        for _ in 0..600 {
                            tokio::time::sleep(Duration::from_millis(250)).await;
                            if lock(&me.active).is_none() {
                                me.on_match(&m);
                                return;
                            }
                        }
                    });
                }
            }
            // `under_review` (a referee decides) and any unknown state: waiting, nothing to launch.
            "void" | "cancelled" | "completed" => {
                // A replacement of a match this launcher started is covered like it (`consent`).
                if let Some(next) = m.pointer("/outcome/rematch_id").and_then(Value::as_str) {
                    self.client.consent.rematch(&id, next);
                }
                if let Some(a) = lock(&self.active).as_mut().filter(|a| a.id == id) {
                    a.ended = state != "completed";
                }
            }
            _ => {}
        }
    }

    /// A series changed (`series.updated`): when it is over, an open game of it closes.
    pub fn on_series(&self, s: &Value) {
        let state = s.get("state").and_then(Value::as_str).unwrap_or("running");
        let id = s.get("id").and_then(Value::as_str).unwrap_or_default();
        if state != "running" && lock(&self.session).as_ref().is_some_and(|x| x.series.as_deref() == Some(id)) {
            self.quit_session("the series is over");
        }
    }

    async fn upload(&self, id: &str, body: Body) -> Result<(), CallError> {
        let mut wait = 1u64;
        let deadline = Instant::now() + Duration::from_secs(15 * 60);
        loop {
            match self.client.call_limited("POST", &format!("/v1/matches/{id}/result"), body_clone(&body), true, 1 << 20).await {
                Ok(_) => return Ok(()),
                // The match ended (decided without this report): nothing more to do.
                Err(CallError::Api(e)) if e.status == 409 || e.status == 404 || e.status == 413 || e.status == 400 || e.status == 401 || e.status == 403 => return Err(CallError::Api(e)),
                Err(e) => {
                    if Instant::now() > deadline || self.ended(id) {
                        return Err(e);
                    }
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                    wait = (wait * 2).min(60);
                }
            }
        }
    }

    async fn report_error(&self, id: &str, version: u32, game_match_id: &str, code: &str, reason: &str) {
        self.report_error_in_play(id, version, game_match_id, code, reason, None).await;
    }

    /// 1.1.11: a report with the game's `in_play` when it is a departure (`contract::in_play`).
    async fn report_error_in_play(&self, id: &str, version: u32, game_match_id: &str, code: &str, reason: &str, in_play: Option<bool>) {
        let _ = self.upload(id, Body::Json(contract::error_report_in_play(version, game_match_id, code, reason, in_play))).await;
    }

    fn data_dir(&self) -> PathBuf {
        self.host.data_dir().unwrap_or_else(|| self.client.dir.parent().and_then(|p| p.parent()).unwrap_or(&self.client.dir).to_path_buf())
    }

    async fn run(self: Arc<Self>, mut m: Value) {
        if let (Some(l), Some(rules)) = (self.host.local_loadout_rules(), m.get_mut("rules").filter(|r| r.is_object())) {
            rules["loadout"] = l;
        }
        let id = m.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        let game_match_id = m.pointer("/game/match_id").and_then(Value::as_str).unwrap_or(&id).to_string();
        let gver = m.pointer("/game/contract/version").and_then(Value::as_u64).unwrap_or(1) as u32;
        let mut st = RunnerStatus {
            match_id: id.clone(),
            phase: "checking".into(),
            game_state: None,
            game: None,
            code: None,
            reason: None,
            relay_ip: None,
            role: m.pointer("/you/role").and_then(Value::as_str).map(str::to_string),
            member_name: m.pointer("/you/member_name").and_then(Value::as_str).map(str::to_string),
            env: Vec::new(),
            status_path: None,
            result_path: None,
            result_uploaded: false,
            contract: gver,
            keep_open: false,
            choices: None,
            loadout: None,
            loadout_ok: None,
            seat: Some(if m.pointer("/you/team").and_then(Value::as_str) == Some("spectator") { "spectator" } else { "player" }.into()),
            ask: None,
            relay: None,
            ask_s: None,
        };
        self.publish(&st);
        // 1. Check: a match this launcher and its engine cannot run exactly is refused, never adapted.
        let host = self.host.clone();
        let info = tokio::task::spawn_blocking(move || host.engine()).await.ok().flatten();
        let caps = engine::caps(info.as_ref());
        // A game that does not know the engine's listing cannot join or host by itself: refused before any launch.
        let other_version = self.host.game_outdated();
        if other_version || crate::proto::freshness::engine_too_old(info.as_ref()) {
            let reason = if other_version {
                format!("the installed game is not Splatoon 3 {}: update the game from the launcher", crate::installer::gamever::CURRENT.display)
            } else {
                "the installed game is too old for the automatic match (no --list-match-options): recompile it from the launcher".to_string()
            };
            st.phase = "refused".into();
            st.code = Some(contract::ERR_INCOMPATIBLE.into());
            st.reason = Some(reason.clone());
            self.publish(&st);
            self.report_error(&id, gver, &game_match_id, contract::ERR_INCOMPATIBLE, &reason).await;
            self.finish(&id);
            return;
        }
        let plan: GamePlan = match contract::plan(&m, &caps) {
            Ok(p) => p,
            Err(r) => {
                st.phase = "refused".into();
                st.code = Some(r.code.into());
                st.reason = Some(r.reason.clone());
                self.publish(&st);
                self.report_error(&id, gver, &game_match_id, r.code, &r.reason).await;
                self.finish(&id);
                return;
            }
        };
        st.keep_open = plan.keep_open;
        let series = m.pointer("/series/id").and_then(Value::as_str).map(str::to_string);
        // 2. A round of the open game session: sent to the running game, no new process.
        let continuing = {
            let s = lock(&self.session);
            s.as_ref().is_some_and(|s| plan.continue_from.as_deref() == Some(s.last.as_str()) && s.room == plan.room && s.paths.cmd.is_some() && s.host_name == plan.host_name && s.role == plan.role)
        } && plan.loadout.is_empty()
            && self.host.status().running;
        // 2a. Only a match this launcher engaged in during this run starts by itself (`consent`): anything else, and a
        // launch beyond the per-server caps, waits for the player's yes. A new game only ever talks to a relay the
        // server announced. Nothing is closed, written or started before that.
        let own = if continuing { contract::OwnPart::Fresh } else { contract::own_part(&m) };
        if own == contract::OwnPart::Fresh {
            if !continuing && !self.relay_announced(&plan).await {
                let reason = format!("the relay {}:{} is not one this server announced (meta.relays, meta.relay or the relay saved with the server)", plan.relay_host, plan.relay_port);
                st.phase = "refused".into();
                st.code = Some(contract::ERR_RULES.into());
                st.reason = Some(reason.clone());
                st.relay = Some(format!("{}:{}", plan.relay_host, plan.relay_port));
                self.publish(&st);
                self.report_error(&id, plan.version, &plan.match_id, contract::ERR_RULES, &reason).await;
                self.finish(&id);
                return;
            }
            if let Some(ask) = self.client.consent.ask(&m, continuing, Instant::now())
                && !self.confirm(&id, &m, &plan, ask, &mut st).await
            {
                if !self.ended(&id) {
                    let reason = "the player did not confirm the launch of a match this launcher had not joined".to_string();
                    st.phase = "refused".into();
                    st.code = Some(contract::ERR_CANCELLED.into());
                    st.reason = Some(reason.clone());
                    self.publish(&st);
                    self.report_error(&id, plan.version, &plan.match_id, contract::ERR_CANCELLED, &reason).await;
                } else {
                    st.phase = "closed".into();
                    self.publish(&st);
                }
                self.finish(&id);
                return;
            }
        }
        if !continuing && self.session_alive() {
            // Another match comes: the open game cannot play it. It closes first.
            self.quit_session("another match");
            for _ in 0..40 {
                if !self.host.status().running {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
        // 2b. This launcher started (again) while the match was under way, after a crash of the launcher or of the
        // computer: the player's game had already reached the stage, or the player's report is in. The engine cannot
        // join a match in progress (measured: a game started again for it looked for its host's session for 5 minutes
        // while the host's game had stopped, then failed with room-not-found, and the match waited for it), so it is
        // not started again: what the earlier game left is sent instead, at once when that game is gone.
        if !continuing {
            match own {
                contract::OwnPart::Reported => {
                    st.phase = "done".into();
                    self.publish(&st);
                    self.finish(&id);
                    return;
                }
                contract::OwnPart::Started => {
                    self.resume(&id, &plan, &mut st).await;
                    self.finish(&id);
                    return;
                }
                contract::OwnPart::Fresh => {}
            }
        }
        let (paths, ip) = if continuing {
            let paths = lock(&self.session).as_ref().map(|s| s.paths.clone()).expect("session");
            (paths, None)
        } else {
            // 3. Resolve the relay: the game only takes an IPv4 address.
            st.phase = "resolving".into();
            self.publish(&st);
            let resolver = self.resolver.clone();
            let relay_host = plan.relay_host.clone();
            let ip = tokio::task::spawn_blocking(move || servers::parse_host(&relay_host).map_err(|e| e.message().to_string()).and_then(|h| servers::resolve(resolver.as_ref(), &h).map_err(|e| format!("{e:?}"))))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            match ip {
                Ok(ip) => (contract::paths(&self.client.dir, &plan), Some(ip)),
                Err(why) => {
                    st.phase = "failed".into();
                    st.code = Some(contract::ERR_RELAY.into());
                    st.reason = Some(format!("the relay {} could not be resolved: {why}", plan.relay_host));
                    self.publish(&st);
                    self.report_error(&id, plan.version, &plan.match_id, contract::ERR_RELAY, st.reason.as_deref().unwrap_or("")).await;
                    self.finish(&id);
                    return;
                }
            }
        };
        st.relay_ip = ip.map(|i| i.to_string());
        // 4. Fresh files for this round.
        let _ = std::fs::create_dir_all(&paths.dir);
        let (status_path, result_path) = (paths.status_of(&plan.match_id), paths.result_of(&plan.match_id));
        for f in [&status_path, &result_path] {
            let _ = std::fs::remove_file(f);
        }
        st.status_path = Some(status_path.display().to_string());
        st.result_path = Some(result_path.display().to_string());
        if self.ended(&id) {
            st.phase = "closed".into();
            self.publish(&st);
            self.finish(&id);
            return;
        }
        if continuing {
            st.phase = "continuing".into();
            self.publish(&st);
            let cmd = paths.cmd.clone().expect("command file");
            if let Err(e) = append_command(&cmd, &contract::next_command(&plan)) {
                st.phase = "failed".into();
                st.code = Some(contract::ERR_FAILED.into());
                st.reason = Some(format!("the next round could not be sent to the game: {e}"));
                self.publish(&st);
                self.report_error(&id, plan.version, &plan.match_id, contract::ERR_FAILED, st.reason.as_deref().unwrap_or("")).await;
                self.finish(&id);
                return;
            }
        } else {
            // 4b. 1.1.7: a spectator reports its launch before its game starts. Once a player's stage loads the engine
            // cannot let it in (it would wait in the game's lobby until the end, then fail): the game is not started.
            if plan.spectator {
                let started = m
                    .get("participants")
                    .and_then(Value::as_array)
                    .is_some_and(|ps| ps.iter().any(|p| p.pointer("/status/state").and_then(Value::as_str).is_some_and(|s| contract::STARTED_STATES.contains(&s))));
                let late = started
                    || matches!(self.client.call("POST", &format!("/v1/matches/{id}/launch"), Body::Json(json!({})), true).await, Err(CallError::Api(ref e)) if e.code == "match_started");
                if late {
                    let reason = "the match had started: the game cannot let a spectator in any more".to_string();
                    st.phase = "late".into();
                    st.code = Some(contract::ERR_SPECTATOR_LATE.into());
                    st.reason = Some(reason.clone());
                    self.publish(&st);
                    self.report_error(&id, plan.version, &plan.match_id, contract::ERR_SPECTATOR_LATE, &reason).await;
                    self.finish(&id);
                    return;
                }
            }
            // 5. Start the game.
            let vars = crate::automatch::MatchVars::load(&self.data_dir());
            let mut env = match contract::build_env(&vars, &plan, ip.expect("resolved"), &paths) {
                Ok(e) => e,
                Err(e) => {
                    st.phase = "refused".into();
                    st.code = Some(contract::ERR_RULES.into());
                    st.reason = Some(e.to_string());
                    self.publish(&st);
                    self.report_error(&id, plan.version, &plan.match_id, contract::ERR_RULES, &e.to_string()).await;
                    self.finish(&id);
                    return;
                }
            };
            if let Some(cmd) = &paths.cmd {
                // A new session: the command file starts empty (the engine reads only lines appended after its start).
                let _ = std::fs::write(cmd, b"");
            }
            // Loadout constraints (contract 2, `match-contract-2.md` 5): applied to the match's copy of the save
            // (`SPLAT3_MATCH_LOADOUT`); a choice left to the player waits for the pick until shortly before the launch
            // deadline, then the current item (when it complies) or the first allowed one is taken.
            if !plan.loadout.is_empty() {
                let current = self.host.current_loadout();
                let subs = self.host.current_sub_abilities();
                let mains = self.host.current_main_abilities();
                let mine = |me: &Self| lock(&me.picks).get(&id).cloned().unwrap_or_default();
                let (_, open) = contract::resolve_loadout(&plan.loadout, &mine(&self), &current, false);
                if !open.is_empty() {
                    let left = m.pointer("/deadlines/launch").and_then(Value::as_str).and_then(crate::proto::tokens::parse_time).map(|t| t - self.client.server_now() - 20).unwrap_or(40).clamp(5, 60);
                    st.choices = Some(Value::Object(plan.loadout.iter().filter(|(s, _)| open.contains(s)).map(|(s, c)| (s.clone(), serde_json::to_value(c).unwrap_or(Value::Null))).collect()));
                    st.phase = "choosing".into();
                    self.publish(&st);
                    let until = Instant::now() + Duration::from_secs(left as u64);
                    while Instant::now() < until && !self.ended(&id) && !contract::resolve_loadout(&plan.loadout, &mine(&self), &current, false).1.is_empty() {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                    st.choices = None;
                }
                let (chosen, _) = contract::resolve_loadout(&plan.loadout, &mine(&self), &current, true);
                let file = paths.dir.join(format!("loadout-{}.json", plan.match_id));
                if !chosen.is_empty() {
                    if let Err(e) = crate::util::atomic_write(&file, contract::loadout_profile(&chosen, &subs, &mains).to_string().as_bytes()) {
                        st.phase = "failed".into();
                        st.code = Some(contract::ERR_FAILED.into());
                        st.reason = Some(format!("the match loadout could not be written: {e}"));
                        self.publish(&st);
                        self.report_error(&id, plan.version, &plan.match_id, contract::ERR_FAILED, st.reason.as_deref().unwrap_or("")).await;
                        self.finish(&id);
                        return;
                    }
                    env.push(("SPLAT3_MATCH_LOADOUT".into(), file.display().to_string()));
                }
                st.loadout = Some(Value::Object(chosen.iter().map(|(s, x)| (s.clone(), Value::String(x.clone()))).collect()));
            }
            // 4c. A game the player opened for solo play cannot play the match: it is closed first (asked, then terminated
            // after a few seconds, only that game), within the launch deadline. The launcher's own match in progress is
            // not touched: it keeps the wait and the refusal below.
            if self.host.open_game() == crate::launch::OpenGame::Solo {
                st.phase = "closing".into();
                self.publish(&st);
                let host = self.host.clone();
                let closed = tokio::task::spawn_blocking(move || host.close_open_game()).await.unwrap_or_else(|e| Err(e.to_string()));
                if let Err(why) = closed {
                    st.phase = "failed".into();
                    st.code = Some(CODE_NOT_CLOSED.into());
                    st.reason = Some(why.clone());
                    self.publish(&st);
                    self.report_error(&id, plan.version, &plan.match_id, contract::ERR_FAILED, &why).await;
                    self.finish(&id);
                    return;
                }
                if self.ended(&id) {
                    st.phase = "closed".into();
                    self.publish(&st);
                    self.finish(&id);
                    return;
                }
            }
            st.env = crate::launch::shown_env(&env);
            st.phase = "launching".into();
            self.publish(&st);
            // The previous match's game closes by itself after its result screens (SPLAT3_MATCH_EXIT): a series' next
            // game may be ready before that. Wait for it (within the launch deadline) rather than fail.
            let until = Instant::now() + Duration::from_secs(90);
            while self.host.status().running && Instant::now() < until && !self.ended(&id) {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            if let Err(why) = self.host.launch(env) {
                st.phase = "failed".into();
                st.code = Some(contract::ERR_FAILED.into());
                st.reason = Some(why.clone());
                self.publish(&st);
                self.report_error(&id, plan.version, &plan.match_id, contract::ERR_FAILED, &why).await;
                self.finish(&id);
                return;
            }
        }
        self.client.consent.launched(&m, !continuing, Instant::now());
        *lock(&self.session) = plan.keep_open.then(|| Session { room: plan.room.clone(), paths: paths.clone(), last: plan.id.clone(), series: series.clone(), host_name: plan.host_name.clone(), role: plan.role });
        // 6. Report the launch (idempotent; retried on network errors).
        for i in 0..5 {
            // What was imposed, for the server (informational): `{"loadout": {"weapon", "gear": {...}}}`.
            let body = match &st.loadout {
                Some(Value::Object(l)) if !l.is_empty() => {
                    let chosen: Vec<(String, String)> = l.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect();
                    let mut p = contract::loadout_profile(&chosen, &HashMap::new(), &HashMap::new()); // what the server is told: ids only
                    if let Some(o) = p.as_object_mut() {
                        o.remove("version");
                    }
                    json!({"loadout": p})
                }
                _ => json!({}),
            };
            match self.client.call("POST", &format!("/v1/matches/{id}/launch"), Body::Json(body), true).await {
                Ok(_) | Err(CallError::Api(_)) => break,
                Err(_) => tokio::time::sleep(Duration::from_secs(1 << i)).await,
            }
        }
        st.phase = "running".into();
        self.publish(&st);
        // 7. Mirror the status, upload the result.
        self.watch(&id, &plan, &status_path, &result_path, &mut st).await;
        self.finish(&id);
        // 8. An open session waits for the next round at most `game.continue_window_s` (1.1.1: 1 to 300, default
        // 90); then the hidden game gets `quit`.
        if plan.keep_open && self.session_alive() {
            let wait = m
                .pointer("/game/continue_window_s")
                .and_then(Value::as_u64)
                .filter(|s| (1..=300).contains(s))
                .map(Duration::from_secs)
                .unwrap_or(self.idle_quit);
            let me = self.clone();
            let last = plan.id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(wait).await;
                if lock(&me.session).as_ref().is_some_and(|s| s.last == last) {
                    me.quit_session("nothing continued the session in time");
                }
            });
        }
    }

    /// 2b: a match this player's earlier game had started, found again by a launcher that started anew. Its files are
    /// the earlier game's: its result file is sent when there is one, else its status file's error (contract 2), else
    /// `launcher-crashed`; while that game still runs (only the launcher had stopped), it is waited for first.
    async fn resume(&self, id: &str, plan: &GamePlan, st: &mut RunnerStatus) {
        let paths = contract::paths(&self.client.dir, plan);
        let (status_path, result_path) = (paths.status_of(&plan.match_id), paths.result_of(&plan.match_id));
        st.status_path = Some(status_path.display().to_string());
        st.result_path = Some(result_path.display().to_string());
        st.phase = "running".into();
        self.publish(st);
        let max_result = self.client.cached_meta().and_then(|(m, _)| m.pointer("/limits/result_bytes").and_then(Value::as_u64)).unwrap_or(65536) as usize;
        let exe = "splat3.exe";
        loop {
            if let Ok(text) = std::fs::read_to_string(&result_path)
                && contract::result_ready(&text, max_result)
            {
                st.phase = "uploading".into();
                self.publish(st);
                let _ = self.upload(id, Body::Raw(text.into_bytes())).await;
                st.result_uploaded = true;
                st.phase = "done".into();
                self.publish(st);
                return;
            }
            let status = std::fs::read_to_string(&status_path).ok().and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok());
            st.game_state = status.as_ref().and_then(|s| s.get("state")).and_then(Value::as_str).map(str::to_string);
            let error = status.as_ref().and_then(|s| s.get("error")).and_then(Value::as_str).filter(|c| !c.is_empty() && c.len() <= 64);
            let pid = status.as_ref().and_then(|s| s.get("pid")).and_then(Value::as_u64).and_then(|p| u32::try_from(p).ok());
            let alive = pid.and_then(|p| crate::installer::sys::process_alive(p, exe)) == Some(true);
            if alive && error.is_none() && !self.ended(id) {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            if self.ended(id) {
                st.phase = "closed".into();
                self.publish(st);
                return;
            }
            let (code, reason, in_play) = match error {
                Some(code) if contract::engine_code_passes(plan.version, code) => {
                    let r = status.as_ref().and_then(|s| s.get("reason")).and_then(Value::as_str).unwrap_or_default();
                    (code.to_string(), if r.is_empty() { "the engine stopped the match".to_string() } else { r.to_string() }, status.as_ref().and_then(contract::in_play))
                }
                _ => (
                    contract::ERR_CRASHED.to_string(),
                    "the launcher started again during the match and found its game closed; a game cannot rejoin a match in progress".to_string(),
                    None,
                ),
            };
            st.phase = "uploading".into();
            self.publish(st);
            self.report_error_in_play(id, plan.version, &plan.match_id, &code, &reason, in_play).await;
            st.code = Some(code);
            st.reason = Some(reason);
            st.result_uploaded = true;
            st.phase = "failed".into();
            self.publish(st);
            return;
        }
    }

    /// Asks the player before the launch (`consent::Ask`): phase `confirm` until the player answers ([`Runner::answer`]),
    /// the match ends, or the question's time is up (the launch deadline less a margin, at most [`Runner::ask_max`]).
    /// `true`: start the game.
    async fn confirm(&self, id: &str, m: &Value, plan: &GamePlan, ask: Ask, st: &mut RunnerStatus) -> bool {
        lock(&self.answers).remove(id);
        let to_deadline = m.pointer("/deadlines/launch").and_then(Value::as_str).and_then(crate::proto::tokens::parse_time).map(|t| t - self.client.server_now() - 10).unwrap_or(i64::MAX).clamp(10, 3600);
        let left = Duration::from_secs(to_deadline as u64).min(self.ask_max);
        st.phase = "confirm".into();
        st.ask = Some(ask.code().into());
        st.relay = Some(format!("{}:{}", plan.relay_host, plan.relay_port));
        st.ask_s = Some(left.as_secs().max(1));
        self.publish(st);
        let until = Instant::now() + left;
        let yes = loop {
            if let Some(a) = lock(&self.answers).remove(id) {
                break a;
            }
            if self.ended(id) || Instant::now() >= until {
                break false;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        };
        st.ask = None;
        st.ask_s = None;
        if yes {
            self.client.consent.confirm(id);
            st.phase = "checking".into();
            self.publish(st);
        } else {
            self.client.consent.refuse(id);
        }
        yes
    }

    /// The match's relay is one the server announced: an entry of `meta.relays`, the meta's `relay`, or the relay saved
    /// with the server when it was added (its discovery document). Same name (or address) and same port.
    async fn relay_announced(&self, plan: &GamePlan) -> bool {
        if self.client.cached_meta().is_none() {
            let _ = self.client.fetch_meta().await;
        }
        let norm = |h: &str| servers::parse_host(h).ok().map(|h| h.text().to_ascii_lowercase());
        let Some(want) = norm(&plan.relay_host) else { return false };
        let mut known: Vec<(String, u16)> = Vec::new();
        if let Some((meta, check)) = self.client.cached_meta() {
            known.extend(relayping::relays_of(&meta).into_iter().map(|r| (r.host.text().to_ascii_lowercase(), r.port)));
            if let Some(r) = check.relay.as_deref().and_then(|t| discovery::parse_relay(&Value::String(t.to_string())).ok().flatten())
                && let Some(h) = norm(&r.host)
            {
                known.push((h, r.port.unwrap_or(servers::DEFAULT_RELAY_PORT)));
            }
        }
        let store = crate::online::load(&self.data_dir());
        if let Some(s) = store.servers.iter().find(|s| s.id == self.client.server_id)
            && let Some(h) = s.relay.as_deref().and_then(norm)
        {
            known.push((h, s.relay_port.unwrap_or(servers::DEFAULT_RELAY_PORT)));
        }
        known.iter().any(|(h, p)| *h == want && *p == plan.relay_port)
    }

    /// This match's run is over (only its own: a later match may already be the active one).
    fn finish(&self, id: &str) {
        let mut a = lock(&self.active);
        if a.as_ref().is_some_and(|a| a.id == id) {
            *a = None;
        }
    }

    async fn watch(&self, id: &str, plan: &GamePlan, status_path: &PathBuf, result_path: &PathBuf, st: &mut RunnerStatus) {
        let max_result = self.client.cached_meta().and_then(|(m, _)| m.pointer("/limits/result_bytes").and_then(Value::as_u64)).unwrap_or(65536) as usize;
        let max_status = self.client.cached_meta().and_then(|(m, _)| m.pointer("/limits/status_bytes").and_then(Value::as_u64)).unwrap_or(4096) as usize;
        let mut last_sent: Option<String> = None;
        // The status file's last error: its code, its sentence and (1.1.11) the game's in_play for a departure.
        let mut last_error: Option<(String, String, Option<bool>)> = None;
        let mut last_post = Instant::now() - Duration::from_secs(5);
        let mut exited_at: Option<Instant> = None;
        let mut uploaded_at: Option<Instant> = None;
        let data_dir = self.data_dir();
        loop {
            // The status file: sent verbatim at every change (at most 2 per second).
            if let Ok(text) = std::fs::read_to_string(status_path) {
                let text = text.trim_start_matches('\u{feff}').trim_end().to_string();
                if last_sent.as_deref() != Some(text.as_str())
                    && text.len() <= max_status
                    && last_post.elapsed() >= Duration::from_millis(500)
                    && let Some(state) = contract::status_state(&text)
                {
                    if let Ok(v) = serde_json::from_str::<Value>(&text) {
                        if let Some(stages) = v.get("stage_scenes").or_else(|| v.get("stages")).and_then(Value::as_array) {
                            engine::remember_room_stages(&data_dir, stages);
                        }
                        if let Some(code) = v.get("error").and_then(Value::as_str).filter(|c| !c.is_empty() && c.len() <= 64) {
                            last_error = Some((code.to_string(), v.get("reason").and_then(Value::as_str).unwrap_or_default().to_string(), contract::in_play(&v)));
                        }
                    }
                    st.game_state = Some(state);
                    st.game = crate::proto::progress::from_status(&text, plan.timeouts.as_deref());
                    self.publish(st);
                    if !self.ended(id) {
                        match self.client.call("POST", &format!("/v1/matches/{id}/status"), Body::Raw(text.clone().into_bytes()), true).await {
                            Ok(_) | Err(CallError::Api(_)) => last_sent = Some(text),
                            Err(_) => {}
                        }
                    } else {
                        last_sent = Some(text);
                    }
                    last_post = Instant::now();
                }
            }
            // 1.1.7: the player left its place in the stands: its game closes, whatever the step.
            if lock(&self.active).as_ref().is_some_and(|a| a.id == id && a.left) {
                self.quit_session("the spectator left");
                self.host.stop();
                st.phase = "closed".into();
                self.publish(st);
                return;
            }
            // The match was cancelled or voided before the game reached the stage: close it, upload nothing more.
            if self.ended(id) {
                let started = matches!(st.game_state.as_deref(), Some("in-match") | Some("results") | Some("finished") | Some("waiting-command"));
                if !started {
                    self.quit_session("the match was cancelled");
                    self.host.stop();
                    st.phase = "closed".into();
                    self.publish(st);
                    return;
                }
            }
            // The result file: uploaded verbatim, once, as soon as it is complete (a record, or an aborted round).
            if !st.result_uploaded
                && let Ok(text) = std::fs::read_to_string(result_path)
                && contract::result_ready(&text, max_result)
            {
                // The status of the result screens goes first, whatever the pace: the decision can follow the upload at
                // once, and a status sent after it is refused (the players' list would keep "in the match").
                if let Ok(s) = std::fs::read_to_string(status_path) {
                    let s = s.trim_start_matches('\u{feff}').trim_end().to_string();
                    if last_sent.as_deref() != Some(s.as_str())
                        && s.len() <= max_status
                        && let Some(state) = contract::status_state(&s)
                        && !self.ended(id)
                    {
                        st.game_state = Some(state);
                        st.game = crate::proto::progress::from_status(&s, plan.timeouts.as_deref());
                        if let Ok(_) | Err(CallError::Api(_)) = self.client.call("POST", &format!("/v1/matches/{id}/status"), Body::Raw(s.clone().into_bytes()), true).await {
                            last_sent = Some(s);
                        }
                        last_post = Instant::now();
                    }
                }
                st.loadout_ok = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok().and_then(|v| v.pointer("/loadout/ok").and_then(Value::as_bool));
                st.phase = "uploading".into();
                self.publish(st);
                let _ = self.upload(id, Body::Raw(text.into_bytes())).await;
                st.result_uploaded = true;
                uploaded_at = Some(Instant::now());
                st.phase = if plan.keep_open { "waiting".into() } else { "done".into() };
                self.publish(st);
            }
            // Uploaded, and the game says it is closing or waits for the next round (or took too long to say).
            if let Some(t) = uploaded_at
                && (matches!(st.game_state.as_deref(), Some("finished") | Some("waiting-command")) || t.elapsed() > Duration::from_secs(90))
                && last_sent.as_deref().is_some_and(|s| contract::status_state(s).as_deref() == st.game_state.as_deref())
            {
                return;
            }
            let g = self.host.status();
            if g.exited || !g.running {
                let first = *exited_at.get_or_insert_with(Instant::now);
                // Give the game's last writes a moment to land before deciding there is no result.
                if first.elapsed() >= Duration::from_millis(1500) {
                    lock(&self.session).take();
                    if !st.result_uploaded && !self.ended(id) {
                        let before_lobby = matches!(st.game_state.as_deref(), None | Some("booting"));
                        // Contract 2: the status file's error code when the engine wrote no result. 1.1.11: a departure the
                        // engine wrote (Leave match, or the window closed in the match) whatever the contract: its in_play is
                        // the only proof of a departure in play. 1.1.13: a relay's refusal too (why the match never began).
                        let engine_error = last_error.clone().filter(|(code, _, _)| contract::engine_code_passes(plan.version, code));
                        let (code, reason, in_play) = match engine_error {
                            Some((code, reason, in_play)) if !(g.stopped_by_user && before_lobby) => {
                                (code, if reason.is_empty() { "the engine stopped the match".into() } else { reason }, in_play)
                            }
                            // Cancelled before the lobby, left in the match (the window closed, an engine before 1.1.11), or
                            // crashed.
                            _ => {
                                let (code, reason) = contract::ended_without_result(st.game_state.as_deref(), g.exit_code, g.stopped_by_user);
                                (code.to_string(), reason, None)
                            }
                        };
                        st.phase = "uploading".into();
                        self.publish(st);
                        self.report_error_in_play(id, plan.version, &plan.match_id, &code, &reason, in_play).await;
                        st.code = Some(code);
                        st.reason = Some(reason);
                        st.result_uploaded = true;
                        st.phase = "failed".into();
                        self.publish(st);
                    } else if st.phase != "done" && st.phase != "failed" {
                        st.phase = "done".into();
                        self.publish(st);
                    }
                    return;
                }
            }
            tokio::time::sleep(self.poll).await;
        }
    }
}

fn body_clone(b: &Body) -> Body {
    match b {
        Body::None => Body::None,
        Body::Json(v) => Body::Json(v.clone()),
        Body::Raw(r) => Body::Raw(r.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_appended_lines() {
        let d = std::env::temp_dir().join(format!("s3l-cmd-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("commands.txt");
        append_command(&f, "next id=m_2").unwrap();
        append_command(&f, "quit").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "next id=m_2\nquit\n");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
