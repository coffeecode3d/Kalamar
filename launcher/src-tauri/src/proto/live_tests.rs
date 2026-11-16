//! Tests against a running server of the protocol (the reference `kalamar-service` with its local configuration and
//! `kalamar-relay`). Ignored by default; run them with the server up:
//!
//! ```text
//! set KALAMAR_TEST_HOST=127.0.0.1:8080
//! cargo test --lib proto::live_tests -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! Two (or more) launchers are simulated in one process: each has its own data folder, hub, event stream and match
//! runner. The game is a stand-in (`FakeGame`) that writes the status and result files the way the runtime does, from
//! the environment the runner gives it; no game is started.

use super::glue::{self, AddInput, Discovered};
use super::http::Body;
use super::runner::{GameHost, GameRun};
use super::{Conn, Hub, Sink};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn host() -> Option<String> {
    std::env::var("KALAMAR_TEST_HOST").ok().filter(|s| !s.trim().is_empty())
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().unwrap()
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("s3l-live-{tag}-{}", crate::util::random_hex(4)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[derive(Default)]
struct Collector {
    seen: Mutex<Vec<(String, Value)>>,
}

impl Sink for Collector {
    fn emit(&self, channel: &str, payload: Value) {
        self.seen.lock().unwrap().push((channel.to_string(), payload));
    }
}

impl Collector {
    fn find(&self, f: &dyn Fn(&str, &Value) -> bool) -> Option<Value> {
        self.seen.lock().unwrap().iter().rev().find(|(c, v)| f(c, v)).map(|(_, v)| v.clone())
    }
    fn count(&self, f: &dyn Fn(&str, &Value) -> bool) -> usize {
        self.seen.lock().unwrap().iter().filter(|(c, v)| f(c, v)).count()
    }
    async fn wait(&self, what: &str, secs: u64, f: impl Fn(&str, &Value) -> bool) -> Value {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            if let Some(v) = self.find(&f) {
                return v;
            }
            if Instant::now() > end {
                let tail: Vec<String> = self.seen.lock().unwrap().iter().rev().take(12).map(|(c, v)| format!("{c} {}", v.get("type").or(v.get("state")).or(v.get("phase")).cloned().unwrap_or(Value::Null))).collect();
                panic!("timed out waiting for {what}; last pushes: {tail:?}");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

fn is_event(kind: &'static str) -> impl Fn(&str, &Value) -> bool {
    move |c, v| c == "proto-event" && v["type"] == kind
}

/// The game, simulated: statuses every 120 ms, then a result built from the match's participants (identical shared
/// fields for every player, as the real game guarantees), then `finished`. `crash`: exits after loading the stage.
struct FakeGame {
    col: Arc<Collector>,
    env: Mutex<Option<Vec<(String, String)>>>,
    run: Arc<Mutex<GameRun>>,
    crash: bool,
    /// The engine's capability list this fake declares (its stages), when it has one.
    listed: Option<super::engine::EngineInfo>,
}

/// A made-up engine list (neutral scenes and labels, none of the game's) with these stages, every one loadable.
fn listed_engine(stages: &[&str]) -> super::engine::EngineInfo {
    let list: Vec<Value> = stages.iter().enumerate().map(|(i, s)| json!({"stage": i + 1, "scene": s, "label": format!("Stage {}", i + 1), "in_stage_lists": true})).collect();
    super::engine::EngineInfo {
        fingerprint: "fake".into(),
        options: Some(json!({"format": "splat3-match-options", "version": 1, "modes": [{"key": "turf", "label": "Mode A"}], "stages": list,
            "stage_list": "all", "teams": {"places_per_team": 4, "players_max": 8, "uneven": true}, "result": {"version": 2, "outcomes": ["completed", "aborted"]}})),
        room_stages: None,
        note: None,
        lang: None,
    }
}

struct FakeHost(Arc<FakeGame>);

fn var(env: &[(String, String)], k: &str) -> String {
    env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default()
}

fn write_atomic(path: &str, text: &str) {
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, text).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

impl GameHost for FakeHost {
    fn launch(&self, env: Vec<(String, String)>) -> Result<(), String> {
        let g = self.0.clone();
        *g.env.lock().unwrap() = Some(env.clone());
        *g.run.lock().unwrap() = GameRun { running: true, exited: false, exit_code: None, stopped_by_user: false };
        std::thread::spawn(move || {
            let (id, role, me) = (var(&env, "SPLAT3_MATCH_ID"), var(&env, "SPLAT3_MATCH"), var(&env, "SPLAT3_NICKNAME"));
            // Contract 2 paths carry `{id}` (the engine puts the round's match id there).
            let (status, result) = (var(&env, "SPLAT3_MATCH_STATUS").replace("{id}", &id), var(&env, "SPLAT3_MATCH_RESULT").replace("{id}", &id));
            let set = |state: &str| {
                write_atomic(&status, &json!({"version": 1, "match_id": id, "role": role, "state": state, "since_unix": crate::util::now_secs(), "detail": state, "visible": state == "in-match", "pid": 4242}).to_string());
                std::thread::sleep(Duration::from_millis(120));
            };
            let steps: &[&str] = if role == "host" { &["booting", "lobby", "creating-room", "waiting-for-players", "in-room", "loading-stage"] } else { &["booting", "lobby", "joining", "in-room", "loading-stage"] };
            for s in steps {
                set(s);
            }
            if g.crash {
                *g.run.lock().unwrap() = GameRun { running: false, exited: true, exit_code: Some(3), stopped_by_user: false };
                return;
            }
            set("in-match");
            // The match object (the runner got it from an event or from the start answer).
            let end = Instant::now() + Duration::from_secs(10);
            let m = loop {
                if let Some(v) = g.col.find(&|c, v| c == "proto-event" && v.pointer("/data/game/match_id").and_then(Value::as_str) == Some(id.as_str()) && v.pointer("/data/participants").is_some_and(Value::is_array)) {
                    break v["data"].clone();
                }
                assert!(Instant::now() < end, "the fake game never saw its match");
                std::thread::sleep(Duration::from_millis(50));
            };
            let parts = m["participants"].as_array().cloned().unwrap_or_default();
            let players: Vec<Value> = parts
                .iter()
                .enumerate()
                .map(|(i, p)| json!({"name": p["member_name"], "self": p["member_name"] == me.as_str(), "team": p["team"], "paint": 100 + i * 7, "kills": i, "assists": 0, "deaths": 1, "specials": 0, "disconnected": false}))
                .collect();
            let paint = |t: &str| players.iter().filter(|p| p["team"] == t).map(|p| p["paint"].as_u64().unwrap()).sum::<u64>();
            let (pa, pb) = (paint("alpha"), paint("bravo"));
            let win = if pa >= pb { 0 } else { 1 };
            let record = json!({
                "version": 1, "match_id": id, "role": role, "rule": 0, "mode": m["rules"]["mode"], "stage_id": 11,
                "stage_scene": m["rules"]["stage"].as_str().unwrap_or("Stage_Test"),
                "end_utc": crate::util::now_secs(), "battle_frames": 10800, "duration_s": 180.0, "win_team": win, "winner": if win == 0 { "alpha" } else { "bravo" },
                "deemed_result": -1,
                "teams": [{"team": "alpha", "paint_points": pa, "turf_permille": 400}, {"team": "bravo", "paint_points": pb, "turf_permille": 300}, {"team": "charlie", "paint_points": 0, "turf_permille": 0}],
                "players": players, "t0_unix": 1.5, "timeline_s": [["lobby loading", 1.0]]
            });
            set("results");
            write_atomic(&result, &serde_json::to_string_pretty(&record).unwrap());
            set("finished");
            *g.run.lock().unwrap() = GameRun { running: false, exited: true, exit_code: Some(0), stopped_by_user: false };
        });
        Ok(())
    }
    fn status(&self) -> GameRun {
        self.0.run.lock().unwrap().clone()
    }
    fn stop(&self) {
        *self.0.run.lock().unwrap() = GameRun { running: false, exited: true, exit_code: Some(1), stopped_by_user: true };
    }
    fn build_id(&self) -> String {
        "test-build".into()
    }
    fn engine(&self) -> Option<super::engine::EngineInfo> {
        self.0.listed.clone()
    }
}

/// The game as a real process: the launcher's stand-in executable (`KALAMAR_TEST_STANDIN`), which emulates the engine
/// of the match-rules branch (its capability list, contract 2, rounds through the command file). Its listing is read
/// by the launcher's own code (`engine::engine_info`), as with the real engine.
struct ProcHost {
    exe: PathBuf,
    dir: PathBuf,
    child: Mutex<Option<std::process::Child>>,
    last_env: Mutex<Option<Vec<(String, String)>>>,
    exit: Mutex<Option<i32>>,
    rounds: bool,
    /// More variables for the stand-in (its room's stage list).
    extra_env: Mutex<Vec<(String, String)>>,
    /// Loadout constraints handled as if the match carried them, and this player's current items.
    loadout_rules: Mutex<Option<Value>>,
    current: Mutex<std::collections::HashMap<String, String>>,
}

impl GameHost for ProcHost {
    fn launch(&self, env: Vec<(String, String)>) -> Result<(), String> {
        // As the launcher's game manager: one game at a time.
        if self.status().running {
            return Err("the game is already running".into());
        }
        let log = std::fs::File::create(self.dir.join(format!("standin-{}.log", crate::util::random_hex(3)))).map_err(|e| e.to_string())?;
        let child = std::process::Command::new(&self.exe)
            .envs(env.iter().cloned())
            .env("STANDIN_STEP_MS", "150")
            .envs(self.extra_env.lock().unwrap().iter().cloned())
            .current_dir(&self.dir)
            .stdout(std::process::Stdio::null())
            .stderr(log)
            .spawn()
            .map_err(|e| e.to_string())?;
        *self.last_env.lock().unwrap() = Some(env);
        *self.exit.lock().unwrap() = None;
        *self.child.lock().unwrap() = Some(child);
        Ok(())
    }
    fn status(&self) -> GameRun {
        let mut g = self.child.lock().unwrap();
        match g.as_mut().map(|c| c.try_wait()) {
            Some(Ok(None)) => GameRun { running: true, ..Default::default() },
            Some(Ok(Some(s))) => {
                *self.exit.lock().unwrap() = s.code();
                GameRun { running: false, exited: true, exit_code: s.code(), stopped_by_user: false }
            }
            _ => GameRun { running: false, exited: true, exit_code: *self.exit.lock().unwrap(), stopped_by_user: false },
        }
    }
    fn stop(&self) {
        if let Some(c) = self.child.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }
    fn build_id(&self) -> String {
        super::engine::fingerprint(&self.exe).unwrap_or_default()
    }
    fn engine(&self) -> Option<super::engine::EngineInfo> {
        Some(super::engine::engine_info(&self.dir, &self.exe, &self.dir))
    }
    fn data_dir(&self) -> Option<PathBuf> {
        Some(self.dir.clone())
    }
    fn declare_rounds(&self) -> bool {
        self.rounds
    }
    fn local_loadout_rules(&self) -> Option<Value> {
        self.loadout_rules.lock().unwrap().clone()
    }
    fn current_loadout(&self) -> std::collections::HashMap<String, String> {
        self.current.lock().unwrap().clone()
    }
}

/// One simulated launcher: its data folder, hub, pushes and game.
struct Launcher {
    dir: PathBuf,
    hub: Hub,
    col: Arc<Collector>,
    game: Arc<FakeGame>,
    proc_host: Option<Arc<ProcHost>>,
    server: String,
}

struct HostRef(Arc<ProcHost>);

impl GameHost for HostRef {
    fn launch(&self, env: Vec<(String, String)>) -> Result<(), String> {
        self.0.launch(env)
    }
    fn status(&self) -> GameRun {
        self.0.status()
    }
    fn stop(&self) {
        self.0.stop()
    }
    fn build_id(&self) -> String {
        self.0.build_id()
    }
    fn engine(&self) -> Option<super::engine::EngineInfo> {
        self.0.engine()
    }
    fn data_dir(&self) -> Option<PathBuf> {
        self.0.data_dir()
    }
    fn declare_rounds(&self) -> bool {
        self.0.declare_rounds()
    }
    fn local_loadout_rules(&self) -> Option<Value> {
        self.0.local_loadout_rules()
    }
    fn current_loadout(&self) -> std::collections::HashMap<String, String> {
        self.0.current_loadout()
    }
    fn current_sub_abilities(&self) -> std::collections::HashMap<String, Vec<Option<String>>> {
        self.0.current_sub_abilities()
    }
    fn current_main_abilities(&self) -> std::collections::HashMap<String, String> {
        self.0.current_main_abilities()
    }
}

fn standin() -> Option<PathBuf> {
    std::env::var("KALAMAR_TEST_STANDIN").ok().filter(|s| !s.is_empty()).map(PathBuf::from)
}

impl Launcher {
    async fn new(tag: &str, crash: bool) -> Launcher {
        let dir = tmp(tag);
        let col = Arc::new(Collector::default());
        let game = Arc::new(FakeGame { col: col.clone(), env: Mutex::new(None), run: Arc::new(Mutex::new(GameRun::default())), crash, listed: None });
        let hub = Hub::new(dir.clone(), col.clone(), Arc::new(FakeHost(game.clone())), Arc::new(crate::servers::SystemResolver));
        Launcher::finish(dir, hub, col, game, None).await
    }

    /// A launcher whose fake game lists these stages (so that the server can draft them).
    async fn with_stages(tag: &str, stages: &[&str]) -> Launcher {
        let dir = tmp(tag);
        let col = Arc::new(Collector::default());
        let game = Arc::new(FakeGame { col: col.clone(), env: Mutex::new(None), run: Arc::new(Mutex::new(GameRun::default())), crash: false, listed: Some(listed_engine(stages)) });
        let hub = Hub::new(dir.clone(), col.clone(), Arc::new(FakeHost(game.clone())), Arc::new(crate::servers::SystemResolver));
        Launcher::finish(dir, hub, col, game, None).await
    }

    /// A launcher whose game is the stand-in executable (a real process). `rounds`: announce rounds in one session.
    async fn with_standin(tag: &str, exe: &std::path::Path, rounds: bool) -> Launcher {
        let dir = tmp(tag);
        let col = Arc::new(Collector::default());
        let game = Arc::new(FakeGame { col: col.clone(), env: Mutex::new(None), run: Arc::new(Mutex::new(GameRun::default())), crash: false, listed: None });
        let ph = Arc::new(ProcHost { exe: exe.to_path_buf(), dir: dir.clone(), child: Mutex::new(None), last_env: Mutex::new(None), exit: Mutex::new(None), rounds, extra_env: Mutex::new(Vec::new()), loadout_rules: Mutex::new(None), current: Mutex::new(Default::default()) });
        let hub = Hub::new(dir.clone(), col.clone(), Arc::new(HostRef(ph.clone())), Arc::new(crate::servers::SystemResolver));
        Launcher::finish(dir, hub, col, game, Some(ph)).await
    }

    async fn finish(dir: PathBuf, hub: Hub, col: Arc<Collector>, game: Arc<FakeGame>, proc_host: Option<Arc<ProcHost>>) -> Launcher {
        let preview = match glue::discover(&host().unwrap()).await.unwrap_or_else(|f| panic!("discover: {f:?}")) {
            Discovered::Service { preview } => preview,
            other => panic!("not a service: {other:?}"),
        };
        let server = glue::add(&dir, AddInput { name: preview.name.clone(), api: preview.api.clone(), relay: preview.relay.clone() }).await.unwrap();
        Launcher { dir, hub, col, game, proc_host, server }
    }
    fn conn(&self) -> Arc<Conn> {
        self.hub.conn(&self.server, "fr").unwrap()
    }
    async fn login_guest(&self, name: &str) -> Value {
        let c = self.conn();
        c.client.fetch_meta().await.unwrap();
        let me = c.client.login_direct("guest", json!({"display_name": name})).await.unwrap();
        c.relays.declare().await.unwrap();
        self.hub.ensure_stream(&self.server, &c);
        self.col.wait("stream open", 10, |ch, v| ch == "proto-stream" && v["state"] == "open").await;
        me
    }
    async fn req(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, super::http::CallError> {
        let b = match body {
            Some(v) => Body::Json(v),
            None if method == "POST" => Body::Json(json!({})),
            None => Body::None,
        };
        self.conn().client.call(method, path, b, true).await.map(|r| r.body)
    }
    fn env(&self) -> Vec<(String, String)> {
        match &self.proc_host {
            Some(p) => p.last_env.lock().unwrap().clone().unwrap_or_default(),
            None => self.game.env.lock().unwrap().clone().unwrap_or_default(),
        }
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        if let Ok(c) = self.hub.conn(&self.server, "fr") {
            c.stop_stream();
        }
        if let Some(p) = &self.proc_host {
            p.stop();
            std::thread::sleep(Duration::from_millis(300));
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The player's own data: the export names the account, and after the erasure the server no longer knows the session
/// (the launcher has already forgotten it, without a logout request and without the "session lost" notice).
#[test]
#[ignore]
fn a_player_exports_then_erases_the_account() {
    let Some(_) = host() else { return eprintln!("KALAMAR_TEST_HOST is not set: skipped") };
    rt().block_on(async {
        let l = Launcher::new("erase", false).await;
        let me = l.login_guest("Eraser").await;
        let id = me["id"].as_str().unwrap().to_string();
        let c = l.conn();
        let token = c.client.access_token().unwrap();

        let bytes = super::account::export_document(&c.client).await.unwrap();
        let doc: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(doc.is_object());
        assert!(String::from_utf8_lossy(&bytes).contains(&id), "the export names the account");
        let file = l.dir.join(super::account::export_file_name(&c.client.api, chrono::Utc::now()));
        super::account::write_export(&file, &bytes).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        assert!(c.client.session().is_some());

        glue::erase_on(&l.hub, &l.server, &c).await.unwrap();
        assert!(c.client.session().is_none());
        assert!(super::tokens::load(&c.client.dir).is_none());
        assert_eq!(l.col.count(&|ch, _| ch == "proto-session"), 0, "erasing is not a lost session");

        // The token the launcher held is dead on the server, and the account is gone for everyone.
        let url = super::url::join(&c.client.api, "/v1/me");
        let again = super::http::send(c.client.http(), "GET", &url, Some(&token), &Body::None, "en", super::http::MAX_ANSWER).await;
        assert_eq!(again.unwrap_err().status(), 401);
        let other = Launcher::new("erase-witness", false).await;
        other.login_guest("Witness").await;
        let seen = other.req("GET", &format!("/v1/users/{id}"), None).await;
        assert_eq!(seen.unwrap_err().status(), 404);
    });
}

/// Runs an activity action and returns the answer.
async fn act(l: &Launcher, activity: &str, action: &str, values: Value) -> Value {
    let view = l.req("GET", &format!("/v1/activities/{activity}"), None).await.unwrap();
    l.req("POST", &format!("/v1/activities/{activity}/actions/{action}"), Some(json!({"values": values, "revision": view["revision"]}))).await.unwrap_or_else(|e| panic!("{action}: {}", e.message()))
}

/// A best of 3 of the server's activities, played by two launchers whose game is the stand-in (contract 2): the
/// capability list is read from the executable and declared, every game of the series is decided, and with rounds the
/// next game is sent to the open game (`next` in the command file) instead of a new process.
#[test]
#[ignore]
fn a_best_of_3_activity_plays_its_series_in_one_game_session() {
    let (Some(_), Some(exe)) = (host(), standin()) else { return eprintln!("KALAMAR_TEST_HOST or KALAMAR_TEST_STANDIN is not set: skipped") };
    rt().block_on(async {
        // Rounds announced (the launcher's default is not to): the server then keeps the game session open.
        let a = Launcher::with_standin("bo3-a", &exe, true).await;
        let b = Launcher::with_standin("bo3-b", &exe, true).await;
        a.login_guest("Ana").await;
        b.login_guest("Bo").await;
        // What was declared: the engine's list, relayed as is, and contract 2 with rounds.
        let info = a.proc_host.as_ref().unwrap().engine().unwrap();
        assert!(info.options.is_some(), "the stand-in's list was read: {:?}", info.note);
        let list = a.req("GET", "/v1/activities", None).await.unwrap();
        let bo3 = list["items"].as_array().unwrap().iter().find(|x| x["id"] == "bo3").cloned().expect("the bo3 activity");
        println!("activity: {} ({}), join action {}", bo3["title"], bo3["state"], bo3["join"]);
        let r = act(&a, "bo3", "find", json!({})).await;
        println!("find (a): notice {}", r["notice"]);
        act(&b, "bo3", "find", json!({})).await;
        // Game 1 starts a session; the following games continue it.
        let first = a.col.wait("match.ready", 20, is_event("match.ready")).await;
        let g = &first["data"]["game"];
        println!("game 1: contract {}, keep_open {}, series {}", g["contract"]["version"], g["keep_open"], first["data"]["series"]);
        assert_eq!(g["contract"]["version"], 2);
        let sid = first["data"]["series"]["id"].as_str().expect("a series").to_string();
        let done = a.col.wait("series over", 240, |c, v| c == "proto-event" && v["type"] == "series.updated" && v["data"]["id"] == sid.as_str() && v["data"]["state"] != "running").await;
        let s = &done["data"];
        println!("series: state {}, winner {}, games {}", s["state"], s["winner"], s["games"]);
        assert_eq!(s["state"], "completed");
        let games = s["games"].as_array().unwrap();
        for l in [&a, &b] {
            for (c, v) in l.col.seen.lock().unwrap().iter() {
                if c == "proto-event" && v["type"] == "match.void" {
                    println!("VOID {}: {}", v["data"]["id"], v["data"]["outcome"]);
                }
                if c == "proto-runner" && (v["phase"] == "failed" || v["phase"] == "refused" || v["phase"] == "continuing" || v["phase"] == "launching") {
                    println!("RUNNER {} {} {} {}", v["match_id"], v["phase"], v["code"], v["reason"]);
                }
            }
        }
        assert!(games.len() >= 2 && games.iter().all(|g| g["state"] == "completed"), "{games:?}");
        let ea = a.env();
        assert!(ea.iter().any(|(k, _)| k == "SPLAT3_MATCH_RULES"));
        let eb = b.env();
        assert!(eb.iter().any(|(k, _)| k == "SPLAT3_MATCH_RULES"), "contract 2: the guest gets the rules too");
        let keep = g["keep_open"] == true;
        if keep {
            assert!(eb.iter().any(|(k, _)| k == "SPLAT3_MATCH_CMD") && !eb.iter().any(|(k, _)| k == "SPLAT3_MATCH_EXIT"));
            // A game continues the open session only when its host is the session's (the engine's room is the host's);
            // the reference server picks a host at random for each game, so both paths happen.
            let readies: Vec<Value> = a.col.seen.lock().unwrap().iter().filter(|(c, v)| c == "proto-event" && v["type"] == "match.ready").map(|(_, v)| v["data"].clone()).collect();
            for w in readies.windows(2) {
                let same_host = w[0]["game"]["host_name"] == w[1]["game"]["host_name"];
                let id = w[1]["id"].as_str().unwrap_or_default();
                let continued = a.col.count(&|c, v| c == "proto-runner" && v["match_id"] == id && v["phase"] == "continuing") > 0;
                let first_completed = games.iter().any(|g| g["match_id"] == w[0]["id"] && g["state"] == "completed");
                println!("game {id}: same host {same_host}, continued in the open game {continued}");
                if first_completed {
                    assert_eq!(same_host, continued, "continue exactly when the host is the session's");
                }
            }
        }
        // The open games close when the series is over.
        let end = Instant::now() + Duration::from_secs(20);
        while a.proc_host.as_ref().unwrap().status().running || b.proc_host.as_ref().unwrap().status().running {
            assert!(Instant::now() < end, "the stand-ins did not close after the series");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let view = a.req("GET", "/v1/activities/bo3", None).await.unwrap();
        println!("view after: {} sections, actions {:?}", view["sections"].as_array().map(|s| s.len()).unwrap_or(0), view["actions"].as_array().map(|x| x.iter().map(|y| y["id"].clone()).collect::<Vec<_>>()));
    });
}

/// A room on a stage the host's save lacks (1.1.1): the host's engine stops with `stage-unavailable`, the launcher
/// reports that code, the server voids the match with `outcome.rematch_id` and both launchers play the replacement
/// (the game's draw) as a new match. The stand-in host's room list is its first stage only; the server's rooms must
/// offer the stand-in's second stage (`Stand_B`), else the test is skipped.
#[test]
#[ignore]
fn a_stage_missing_from_the_hosts_save_is_redrawn() {
    let (Some(_), Some(exe)) = (host(), standin()) else { return eprintln!("KALAMAR_TEST_HOST or KALAMAR_TEST_STANDIN is not set: skipped") };
    rt().block_on(async {
        let a = Launcher::with_standin("redraw-a", &exe, false).await;
        let b = Launcher::with_standin("redraw-b", &exe, false).await;
        a.proc_host.as_ref().unwrap().extra_env.lock().unwrap().push(("STANDIN_ROOM_STAGES".into(), "1".into()));
        a.login_guest("Hana").await;
        b.login_guest("Ivo").await;
        let room = match a.req("POST", "/v1/rooms", Some(json!({"options": {"teams": [1, 1], "stage": "Stand_B"}}))).await {
            Ok(r) => r,
            Err(e) => return eprintln!("the server's rooms do not offer the stage Stand_B ({}): skipped", e.message()),
        };
        let (rid, code) = (room["id"].as_str().unwrap().to_string(), room["code"].as_str().unwrap().to_string());
        b.req("POST", "/v1/rooms/join", Some(json!({"code": code}))).await.unwrap();
        a.col.wait("room.updated with 2 members", 10, |c, v| c == "proto-event" && v["type"] == "room.updated" && v["data"]["members"].as_array().is_some_and(|m| m.len() == 2)).await;
        b.req("POST", &format!("/v1/rooms/{rid}/ready"), Some(json!({"ready": true}))).await.unwrap();
        let m = a.req("POST", &format!("/v1/rooms/{rid}/start"), None).await.unwrap();
        let first = m["id"].as_str().unwrap().to_string();
        assert_eq!(m["rules"]["stage"], "Stand_B");
        a.conn().runner.on_match(&m);
        // The host's runner reports the engine's code; the server voids the match and names its replacement.
        a.col.wait("the host reports stage-unavailable", 30, |c, v| c == "proto-runner" && v["match_id"] == first.as_str() && v["code"] == "stage-unavailable").await;
        let void = a.col.wait("match.void", 60, |c, v| c == "proto-event" && v["type"] == "match.void" && v["data"]["id"] == first.as_str()).await;
        let o = &void["data"]["outcome"];
        println!("void: decision {}, rematch {}, summary {}", o["decision"], o["rematch_id"], o["summary"]);
        let second = o["rematch_id"].as_str().expect("a replacement match").to_string();
        // Both launchers play the replacement as a new match, once the host's first game has closed.
        let done = a.col.wait("the replacement is decided", 60, |c, v| c == "proto-event" && (v["type"] == "match.completed" || v["type"] == "match.void") && v["data"]["id"] == second.as_str()).await;
        println!("replacement: {} {} (stage {})", done["type"], done["data"]["outcome"]["decision"], done["data"]["rules"]["stage"]);
        assert_eq!(done["type"], "match.completed");
        assert!(done["data"]["rules"]["stage"].is_null(), "the game's draw");
        for l in [&a, &b] {
            let launched = l.col.count(&|c, v| c == "proto-runner" && v["match_id"] == second.as_str() && v["phase"] == "launching");
            assert_eq!(launched, 1, "the replacement was launched once");
        }
    });
}

/// Weapon and gear constraints applied through the engine's match copy of the save (`SPLAT3_MATCH_LOADOUT`): a fixed
/// weapon for the host, a choice among two for the guest whose current weapon complies. The reference server imposes
/// no loadout, so the constraints are handed to both launchers' runners as if the match carried them.
#[test]
#[ignore]
fn loadout_constraints_reach_the_engine_through_the_match_save() {
    let (Some(_), Some(exe)) = (host(), standin()) else { return eprintln!("KALAMAR_TEST_HOST or KALAMAR_TEST_STANDIN is not set: skipped") };
    rt().block_on(async {
        let a = Launcher::with_standin("lo-a", &exe, false).await;
        let b = Launcher::with_standin("lo-b", &exe, false).await;
        let rules = json!({"players": {"Juno": {"weapon": {"fixed": "41"}}}, "all": {"weapon": {"allowed": ["50", "51"]}, "gear": {"head": {"fixed": 1004}}}});
        for l in [&a, &b] {
            *l.proc_host.as_ref().unwrap().loadout_rules.lock().unwrap() = Some(rules.clone());
        }
        b.proc_host.as_ref().unwrap().current.lock().unwrap().insert("weapon".into(), "51".into());
        a.login_guest("Juno").await;
        b.login_guest("Kai").await;
        let room = a.req("POST", "/v1/rooms", Some(json!({"options": {"teams": [1, 1]}}))).await.unwrap();
        let rid = room["id"].as_str().unwrap().to_string();
        b.req("POST", "/v1/rooms/join", Some(json!({"code": room["code"]}))).await.unwrap();
        a.col.wait("room.updated with 2 members", 10, |c, v| c == "proto-event" && v["type"] == "room.updated" && v["data"]["members"].as_array().is_some_and(|m| m.len() == 2)).await;
        b.req("POST", &format!("/v1/rooms/{rid}/ready"), Some(json!({"ready": true}))).await.unwrap();
        let m = a.req("POST", &format!("/v1/rooms/{rid}/start"), None).await.unwrap();
        let id = m["id"].as_str().unwrap().to_string();
        a.conn().runner.on_match(&m);
        let done = a.col.wait("decided", 60, |c, v| c == "proto-event" && (v["type"] == "match.completed" || v["type"] == "match.void") && v["data"]["id"] == id.as_str()).await;
        assert_eq!(done["type"], "match.completed", "{}", done["data"]["outcome"]);
        for (l, weapon) in [(&a, 41), (&b, 51)] {
            let path = env_of(&l.env(), "SPLAT3_MATCH_LOADOUT").map(str::to_string).expect("SPLAT3_MATCH_LOADOUT");
            let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(file, json!({"version": 1, "weapon": weapon, "gear": {"head": 1004}}), "{path}");
            let ok = l.col.wait("loadout.ok", 10, |c, v| c == "proto-runner" && v["match_id"] == id.as_str() && v["loadout_ok"].is_boolean()).await;
            assert_eq!(ok["loadout_ok"], true);
        }
    });
}

/// The server's single-elimination cup with four launchers: register, semi-finals and final (each a best of 3), a/// champion. Every game is played by stand-in processes through the launcher's runner.
#[test]
#[ignore]
fn a_four_player_cup_reaches_a_champion() {
    let (Some(_), Some(exe)) = (host(), standin()) else { return eprintln!("KALAMAR_TEST_HOST or KALAMAR_TEST_STANDIN is not set: skipped") };
    rt().block_on(async {
        let mut ls = Vec::new();
        for (i, n) in ["Cyan", "Dora", "Eli", "Fay"].iter().enumerate() {
            // The launcher's default: no rounds announced, every game is a fresh launch.
            let l = Launcher::with_standin(&format!("cup-{i}"), &exe, false).await;
            l.login_guest(n).await;
            ls.push(l);
        }
        let before = ls[0].req("GET", "/v1/activities/cup", None).await.unwrap();
        println!("cup before: state {}", before["state"]);
        if before["state"] != "open" {
            // The reference cup reopens a while after its final (`reopen_after_s`): a fresh server is needed.
            return eprintln!("the cup is not open (state {}): skipped; restart the test server", before["state"]);
        }
        for l in &ls {
            let r = act(l, "cup", "register", json!({})).await;
            println!("register: {}", r["notice"]["text"]);
        }
        let end = Instant::now() + Duration::from_secs(600);
        let champion = loop {
            let v = ls[0].req("GET", "/v1/activities/cup", None).await.unwrap();
            let champ = v["sections"].as_array().unwrap().iter().find(|s| s["type"] == "notice" && s["level"] == "success").cloned();
            if let Some(c) = champ {
                break (c, v);
            }
            assert!(Instant::now() < end, "no champion in time; last view: {v}");
            tokio::time::sleep(Duration::from_secs(2)).await;
        };
        println!("champion: {}", champion.0["text"]);
        let bracket = champion.1["sections"].as_array().unwrap().iter().find(|s| s["type"] == "bracket").cloned().expect("a bracket");
        let rounds = bracket["rounds"].as_array().unwrap();
        assert_eq!(rounds.len(), 2, "semi-finals and final");
        for r in rounds {
            for m in r["matches"].as_array().unwrap() {
                assert_eq!(m["state"], "completed", "{m}");
            }
        }
        let series_cards = ls.iter().map(|l| l.col.count(&is_event("series.updated"))).sum::<usize>();
        println!("series.updated pushes: {series_cards}");
        assert!(series_cards > 0);
    });
}

#[test]
#[ignore]
fn discovery_cards_and_the_relay_only_fallback() {
    let Some(h) = host() else { return eprintln!("KALAMAR_TEST_HOST is not set: skipped") };
    rt().block_on(async {
        let Discovered::Service { preview } = glue::discover(&h).await.unwrap() else { panic!("no service at {h}") };
        println!("discovered {} at {} (features {:?}, compat {:?})", preview.name, preview.api, preview.features, preview.compat);
        assert!(preview.compat.ok);
        assert!(preview.insecure_loopback, "the local server is plain http on the loopback");
        assert!(preview.features.iter().any(|f| f == "rooms") && preview.features.iter().any(|f| f == "queues"));
        assert!(preview.providers.iter().any(|p| p["id"] == "guest"));
        // The relay's UDP port has no HTTP service: the line is offered as a relay only, as before this protocol.
        match glue::discover("127.0.0.1:47313").await.unwrap() {
            Discovered::RelayOnly { line, reason, .. } => {
                println!("relay-only fallback for {line}: {reason}");
                assert_eq!(line, "127.0.0.1:47313");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(glue::discover("http://example.org").await.unwrap_err().code, "not_https");
        // Saving: once; a second add selects the same entry.
        let dir = tmp("add");
        let a = glue::add(&dir, AddInput { name: preview.name.clone(), api: preview.api.clone(), relay: preview.relay.clone() }).await.unwrap();
        let b = glue::add(&dir, AddInput { name: "again".into(), api: preview.api.clone(), relay: None }).await.unwrap();
        assert_eq!(a, b);
        let store = crate::online::load(&dir);
        assert_eq!(store.servers.len(), 1);
        assert_eq!(store.servers[0].api.as_deref(), Some(preview.api.as_str()));
        assert_eq!(store.selected.as_deref(), Some(a.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
#[ignore]
fn accounts_guest_dev_refresh_rotation_and_logout() {
    if host().is_none() {
        return eprintln!("KALAMAR_TEST_HOST is not set: skipped");
    }
    rt().block_on(async {
        let l = Launcher::new("acct", false).await;
        let c = l.conn();
        c.client.fetch_meta().await.unwrap();
        let me = c.client.login_direct("guest", json!({"display_name": "Alice"})).await.unwrap();
        assert_eq!(me["display_name"], "Alice");
        let file = super::tokens::path(&c.client.dir);
        let raw = std::fs::read(&file).unwrap();
        let s = c.client.session().unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains(&s.access_token), "tokens are encrypted on disk");
        // Rotation: a new pair; the old access token stops working.
        let old = s.access_token.clone();
        c.client.refresh(&old).await.unwrap();
        let s2 = c.client.session().unwrap();
        assert_ne!(s2.access_token, old);
        assert_ne!(s2.refresh_token, s.refresh_token);
        let stale = super::http::send(c.client.http(), "GET", &super::url::join(&c.client.api, "/v1/me"), Some(&old), &Body::None, "fr", 1 << 20).await;
        println!("old access token after rotation: {:?}", stale.as_ref().err().map(|e| e.code().to_string()));
        assert_eq!(stale.unwrap_err().status(), 401);
        assert_eq!(l.req("GET", "/v1/me", None).await.unwrap()["display_name"], "Alice");
        // A new Client on the same folder finds the session again (the launcher restarted).
        let again = super::client::Client::new("x", &c.client.api, c.client.dir.clone(), "en").unwrap();
        assert_eq!(again.session_info().user_id, c.client.session_info().user_id);
        // The dev provider (declared by this test server): the same account for the same username.
        let d = super::client::Client::new("d", &c.client.api, tmp("dev"), "en").unwrap();
        let a1 = d.login_direct("dev", json!({"username": "kalamar-tester"})).await.unwrap();
        let a2 = d.login_direct("dev", json!({"username": "KALAMAR-tester"})).await.unwrap();
        assert_eq!(a1["id"], a2["id"]);
        let _ = std::fs::remove_dir_all(&d.dir);
        // Logout: the session ends on the server and here.
        c.client.logout(false).await;
        assert!(!file.exists());
        let e = super::http::send(c.client.http(), "GET", &super::url::join(&c.client.api, "/v1/me"), Some(&s2.access_token), &Body::None, "fr", 1 << 20).await.unwrap_err();
        assert_eq!(e.status(), 401);
    });
}

fn env_of<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
    env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

#[test]
#[ignore]
fn a_private_room_with_two_launchers_plays_a_match_to_the_end() {
    if host().is_none() {
        return eprintln!("KALAMAR_TEST_HOST is not set: skipped");
    }
    rt().block_on(async {
        let a = Launcher::new("room-a", false).await;
        let b = Launcher::new("room-b", false).await;
        a.login_guest("Alice").await;
        b.login_guest("Bob").await;
        let room = a.req("POST", "/v1/rooms", Some(json!({"options": {"teams": [1, 1]}}))).await.unwrap();
        let (rid, code) = (room["id"].as_str().unwrap().to_string(), room["code"].as_str().unwrap().to_string());
        println!("room {rid} code {code}");
        let joined = b.req("POST", "/v1/rooms/join", Some(json!({"code": code.to_lowercase()}))).await.unwrap();
        assert_eq!(joined["members"].as_array().unwrap().len(), 2);
        a.col.wait("room.updated with 2 members", 10, |c, v| c == "proto-event" && v["type"] == "room.updated" && v["data"]["members"].as_array().is_some_and(|m| m.len() == 2)).await;
        // Teams: Bob is in bravo (fewer players), ready; Alice starts.
        b.req("POST", &format!("/v1/rooms/{rid}/ready"), Some(json!({"ready": true}))).await.unwrap();
        let m = a.req("POST", &format!("/v1/rooms/{rid}/start"), None).await.unwrap();
        assert_eq!(m["state"], "ready");
        let mid = m["id"].as_str().unwrap().to_string();
        a.conn().runner.on_match(&m); // what proto_request does with the start answer
        // Both launchers start their game with the contract's environment.
        b.col.wait("match.ready for the guest", 10, is_event("match.ready")).await;
        let end = Instant::now() + Duration::from_secs(10);
        while (a.env().is_empty() || b.env().is_empty()) && Instant::now() < end {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let (ea, eb) = (a.env(), b.env());
        println!("host env: {ea:?}\nguest env: {eb:?}");
        assert_eq!(env_of(&ea, "SPLAT3_MATCH"), Some("host"));
        assert_eq!(env_of(&ea, "SPLAT3_MATCH_RULES"), m["game"]["rules"].as_str(), "game.rules verbatim");
        assert_eq!(env_of(&ea, "SPLAT3_MATCH_HOST"), None);
        assert_eq!(env_of(&eb, "SPLAT3_MATCH"), Some("join"));
        assert_eq!(env_of(&eb, "SPLAT3_MATCH_HOST"), m["game"]["host_name"].as_str());
        assert_eq!(env_of(&eb, "SPLAT3_MATCH_RULES"), None);
        assert_eq!(env_of(&ea, "SPLAT3_MATCH_ID"), m["game"]["relay"]["room"].as_str());
        assert_eq!(env_of(&ea, "SPLAT3_LAN_ROOM"), m["game"]["relay"]["room"].as_str());
        assert_eq!(env_of(&ea, "SPLAT3_LAN"), Some("relay:127.0.0.1"), "the relay as an IPv4 (default port implicit)");
        assert_eq!(env_of(&ea, "SPLAT3_NICKNAME"), m["you"]["member_name"].as_str());
        assert_eq!(env_of(&ea, "SPLAT3_LAN_NAME"), m["you"]["member_name"].as_str());
        assert_eq!(env_of(&eb, "SPLAT3_NICKNAME"), Some("Bob"));
        // Statuses mirrored, results uploaded, decision pushed.
        let done = a.col.wait("match.completed", 60, |c, v| c == "proto-event" && v["type"] == "match.completed" && v["data"]["id"] == mid.as_str()).await;
        let o = &done["data"]["outcome"];
        println!("outcome: {}", serde_json::to_string(o).unwrap());
        assert_eq!(o["decision"], "agreed");
        assert_eq!(o["counted"], true);
        assert_eq!(o["reports"]["results"], 2);
        let view = a.req("GET", &format!("/v1/matches/{mid}"), None).await.unwrap();
        for p in view["participants"].as_array().unwrap() {
            assert_eq!(p["launched"], true);
            assert_eq!(p["reported"], true);
            assert!(p["status"]["state"].is_string(), "status mirrored: {p}");
        }
        let ra = a.col.wait("runner done", 10, |c, v| c == "proto-runner" && v["match_id"] == mid.as_str() && v["phase"] == "done").await;
        assert_eq!(ra["result_uploaded"], true);
        // The room is open again for another round.
        let r = a.req("GET", "/v1/rooms/current", None).await.unwrap();
        assert_eq!(r["room"]["state"], "open");
        a.req("POST", &format!("/v1/rooms/{rid}/leave"), None).await.unwrap();
        b.req("POST", &format!("/v1/rooms/{rid}/leave"), None).await.unwrap();
    });
}

#[test]
#[ignore]
fn queues_accept_rating_and_the_decline_penalty() {
    if host().is_none() {
        return eprintln!("KALAMAR_TEST_HOST is not set: skipped");
    }
    rt().block_on(async {
        let a = Launcher::new("q-a", false).await;
        let b = Launcher::new("q-b", false).await;
        a.login_guest("Cleo").await;
        b.login_guest("Dax").await;
        let queues = a.req("GET", "/v1/queues", None).await.unwrap();
        let duel = queues["items"].as_array().unwrap().iter().find(|q| q["teams"] == json!([1, 1]) && q["eligible"] == true).cloned().expect("a 1v1 queue");
        let qid = duel["id"].as_str().unwrap().to_string();
        println!("queue {qid}: {}", duel["name"]);
        // 1. Found, both accept, played, rated.
        a.req("POST", &format!("/v1/queues/{qid}/tickets"), None).await.unwrap();
        b.req("POST", &format!("/v1/queues/{qid}/tickets"), None).await.unwrap();
        let found = a.col.wait("match.found", 20, is_event("match.found")).await;
        let mid = found["data"]["id"].as_str().unwrap().to_string();
        assert_eq!(found["data"]["state"], "pending_accept");
        assert!(found["data"]["deadlines"]["accept"].is_string());
        b.col.wait("match.found (b)", 10, is_event("match.found")).await;
        a.req("POST", &format!("/v1/matches/{mid}/accept"), None).await.unwrap();
        b.req("POST", &format!("/v1/matches/{mid}/accept"), None).await.unwrap();
        let done = a.col.wait("match.completed", 60, |c, v| c == "proto-event" && v["type"] == "match.completed" && v["data"]["id"] == mid.as_str()).await;
        println!("queue outcome: {}", serde_json::to_string(&done["data"]["outcome"]).unwrap());
        assert_eq!(done["data"]["outcome"]["rated"], true);
        let rc = a.col.wait("rating.changed", 10, is_event("rating.changed")).await;
        println!("rating.changed: {}", rc["data"]);
        let me = a.conn().client.session_info().user_id.unwrap();
        let ratings = a.req("GET", &format!("/v1/users/{me}/ratings"), None).await.unwrap();
        assert!(!ratings["items"].as_array().unwrap().is_empty());
        // 2. Dax declines twice: the first is a warning, the second blocks the queues (the server's declared penalties).
        for round in 0..2 {
            let before = a.col.count(&is_event("match.found"));
            a.req("POST", &format!("/v1/queues/{qid}/tickets"), None).await.ok();
            b.req("POST", &format!("/v1/queues/{qid}/tickets"), None).await.unwrap();
            let end = Instant::now() + Duration::from_secs(20);
            while a.col.count(&is_event("match.found")) <= before {
                assert!(Instant::now() < end, "no match found in round {round}");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let f = a.col.find(&is_event("match.found")).unwrap();
            let mid = f["data"]["id"].as_str().unwrap().to_string();
            b.req("POST", &format!("/v1/matches/{mid}/decline"), None).await.unwrap();
            let c = a.col.wait("match.cancelled", 10, |c, v| c == "proto-event" && v["type"] == "match.cancelled" && v["data"]["id"] == mid.as_str()).await;
            assert_eq!(c["data"]["cancel_reason"], "declined");
        }
        let e = b.req("POST", &format!("/v1/queues/{qid}/tickets"), None).await.unwrap_err();
        let api = e.api().unwrap();
        println!("third queue attempt: {} {} retry_after_s={:?}", api.status, api.code, api.retry_after_s);
        assert_eq!((api.status, api.code.as_str()), (403, "queue_cooldown"));
        assert!(api.retry_after_s.unwrap_or(0) > 0);
        let me_b = b.req("GET", "/v1/me", None).await.unwrap();
        assert!(me_b["restrictions"].as_array().unwrap().iter().any(|r| r["kind"] == "queue_cooldown"));
        let _ = a.req("DELETE", "/v1/tickets/current", None).await;
    });
}

/// The `oauth_loopback` login against a server whose provider is a stand-in (`KALAMAR_TEST_OAUTH_HOST`: a reference
/// service configured with `[auth.discord]` pointing at a local mock of the provider's three endpoints). The test
/// plays the browser: it follows the authorization URL, whose redirect lands on the launcher's loopback listener.
#[test]
#[ignore]
fn an_oauth_loopback_login_with_pkce() {
    let Some(h) = std::env::var("KALAMAR_TEST_OAUTH_HOST").ok().filter(|s| !s.is_empty()) else {
        return eprintln!("KALAMAR_TEST_OAUTH_HOST is not set: skipped");
    };
    rt().block_on(async {
        let Discovered::Service { preview } = glue::discover(&h).await.unwrap() else { panic!("no service at {h}") };
        let p = preview.providers.iter().find(|p| p["kind"] == "oauth_loopback").cloned().expect("an oauth_loopback provider");
        let dir = tmp("oauth");
        let c = super::client::Client::new("o", &preview.api, dir.clone(), "fr").unwrap();
        let browser = reqwest::Client::new();
        let opened = Arc::new(Mutex::new(String::new()));
        let seen = opened.clone();
        let open = move |url: &str| -> Result<(), String> {
            *seen.lock().unwrap() = url.to_string();
            let (b, u) = (browser.clone(), url.to_string());
            tokio::spawn(async move {
                let r = b.get(&u).send().await;
                println!("browser: {:?}", r.map(|r| r.status()));
            });
            Ok(())
        };
        let me = super::oauth::login(&c, &p, &open, Duration::from_secs(20)).await.unwrap_or_else(|e| panic!("{e:?}"));
        let url = opened.lock().unwrap().clone();
        println!("authorize URL had: {}", reqwest::Url::parse(&url).unwrap().query_pairs().map(|(k, _)| k.into_owned()).collect::<Vec<_>>().join(","));
        assert!(url.contains("code_challenge=") && url.contains("code_challenge_method=S256") && url.contains("state="));
        println!("logged in through {} as {} ({})", p["id"], me["display_name"], me["id"]);
        assert!(c.session_info().logged_in);
        assert_eq!(c.session().unwrap().provider, p["id"].as_str().unwrap());
        // The same identity logs in to the same account.
        let again = super::oauth::login(&c, &p, &open, Duration::from_secs(20)).await.unwrap();
        assert_eq!(again["id"], me["id"]);
        c.logout(false).await;
        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
#[ignore]
fn the_event_stream_resumes_after_a_drop_and_a_crash_is_reported() {
    if host().is_none() {
        return eprintln!("KALAMAR_TEST_HOST is not set: skipped");
    }
    rt().block_on(async {
        let a = Launcher::new("res-a", false).await;
        let b = Launcher::new("res-b", true).await; // this player's game crashes on the stage load
        a.login_guest("Eve").await;
        b.login_guest("Finn").await;
        let room = a.req("POST", "/v1/rooms", Some(json!({"options": {"teams": [1, 1]}}))).await.unwrap();
        let rid = room["id"].as_str().unwrap().to_string();
        b.req("POST", "/v1/rooms/join", Some(json!({"code": room["code"]}))).await.unwrap();
        a.col.wait("2 members", 10, |c, v| c == "proto-event" && v["type"] == "room.updated" && v["data"]["members"].as_array().is_some_and(|m| m.len() == 2)).await;
        // Drop A's connection; while it is down, B gets ready: A must receive that change from the resumed stream.
        let resyncs = a.col.count(&is_event("resync"));
        let opens = a.col.count(&|c, v| c == "proto-stream" && v["state"] == "open");
        a.conn().reconnect();
        a.col.wait("stream retrying", 5, |c, v| c == "proto-stream" && v["state"] == "retrying").await;
        b.req("POST", &format!("/v1/rooms/{rid}/ready"), Some(json!({"ready": true}))).await.unwrap();
        let end = Instant::now() + Duration::from_secs(15);
        while a.col.count(&|c, v| c == "proto-stream" && v["state"] == "open") <= opens {
            assert!(Instant::now() < end, "never reconnected");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let open = a.col.find(&|c, v| c == "proto-stream" && v["state"] == "open").unwrap();
        println!("after the drop: {open}");
        assert_eq!(open["resumed"], true, "the stream resumed");
        assert_eq!(a.col.count(&is_event("resync")), resyncs, "no refetch needed");
        a.col.wait("B ready, replayed", 10, |c, v| c == "proto-event" && v["type"] == "room.updated" && v["data"]["members"].as_array().is_some_and(|m| m.iter().any(|x| x["ready"] == true && x["user"]["display_name"] == "Finn"))).await;
        // A match where B's game crashes: B's launcher reports launcher-crashed.
        let m = a.req("POST", &format!("/v1/rooms/{rid}/start"), None).await.unwrap();
        let mid = m["id"].as_str().unwrap().to_string();
        a.conn().runner.on_match(&m);
        let rb = b.col.wait("B runner failed", 30, |c, v| c == "proto-runner" && v["match_id"] == mid.as_str() && v["phase"] == "failed").await;
        println!("B runner: {rb}");
        assert_eq!(rb["code"], "launcher-crashed");
        let done = a.col.wait("decision", 60, |c, v| c == "proto-event" && (v["type"] == "match.completed" || v["type"] == "match.void") && v["data"]["id"] == mid.as_str()).await;
        println!("decision with a crash: {} {}", done["type"], serde_json::to_string(&done["data"]["outcome"]).unwrap());
        let verdicts = done["data"]["outcome"]["players"].as_array().unwrap();
        assert!(verdicts.iter().any(|p| p["verdict"] == "error"), "B's report is an error report");
        let _ = a.req("POST", &format!("/v1/rooms/{rid}/leave"), None).await;
        let _ = b.req("POST", &format!("/v1/rooms/{rid}/leave"), None).await;
    });
}

/// Protocol 1.1.3 loadouts from the server itself: the room form's `loadout` preset field, and the example
/// configuration's imposed-weapon evening (`imposed-night`, a series whose games carry `rules.loadout`) played by two
/// stand-ins declaring `match_loadout`; then a loadout the engine refuses (`loadout-failed`) reported as an error.
/// Skipped on a server without these examples.
#[test]
#[ignore]
fn server_imposed_loadouts_and_a_refused_loadout() {
    let (Some(_), Some(exe)) = (host(), standin()) else { return eprintln!("KALAMAR_TEST_HOST or KALAMAR_TEST_STANDIN is not set: skipped") };
    rt().block_on(async {
        let a = Launcher::with_standin("imp-a", &exe, false).await;
        let b = Launcher::with_standin("imp-b", &exe, false).await;
        a.login_guest("Lena").await;
        b.login_guest("Milo").await;
        let meta = a.conn().client.cached_meta().map(|(m, _)| m).unwrap_or(Value::Null);
        // The room form's declared fields (whatever member of `rooms` lists them).
        let fields: Vec<Value> = meta.get("rooms").and_then(Value::as_object).map(|o| o.values().filter_map(Value::as_array).flatten().cloned().collect()).unwrap_or_default();
        let preset = fields.iter().find(|f| f["key"] == "loadout");
        println!("room field loadout: {}", preset.map(|f| f.to_string()).unwrap_or_else(|| "absent".into()));
        let p = preset.expect("the 1.1.3 room field loadout (this server declares presets)");
        {
            assert!(crate::proto::glue::KNOWN_FIELD_TYPES.contains(&p["type"].as_str().unwrap_or("")), "the generic form renders it");
        }
        let list = a.req("GET", "/v1/activities", None).await.unwrap();
        if !list["items"].as_array().is_some_and(|i| i.iter().any(|x| x["id"] == "imposed-night")) {
            return eprintln!("no imposed-night activity on this server: skipped");
        }
        act(&a, "imposed-night", "find", json!({})).await;
        act(&b, "imposed-night", "find", json!({})).await;
        let first = a.col.wait("match.ready", 20, is_event("match.ready")).await;
        let id = first["data"]["id"].as_str().unwrap().to_string();
        println!("game 1 rules.loadout: {}", first["data"]["rules"]["loadout"]);
        assert!(first["data"]["rules"]["loadout"].is_object(), "the activity's loadout reaches the match");
        let done = a.col.wait("game 1 decided", 60, |c, v| c == "proto-event" && (v["type"] == "match.completed" || v["type"] == "match.void") && v["data"]["id"] == id.as_str()).await;
        println!("game 1: {} {}", done["type"], done["data"]["outcome"]["decision"]);
        for l in [&a, &b] {
            let path = env_of(&l.env(), "SPLAT3_MATCH_LOADOUT").map(str::to_string).expect("SPLAT3_MATCH_LOADOUT");
            let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(file["weapon"], first["data"]["rules"]["loadout"]["all"]["weapon"]["fixed"], "the fixed weapon, as the server named it");
        }
        // A loadout the engine refuses: the host stops with loadout-failed, reported as the match's error.
        let (c, d) = (Launcher::with_standin("imp-c", &exe, false).await, Launcher::with_standin("imp-d", &exe, false).await);
        *c.proc_host.as_ref().unwrap().loadout_rules.lock().unwrap() = Some(json!({"all": {"weapon": {"fixed": "bad"}}}));
        c.login_guest("Nora").await;
        d.login_guest("Otto").await;
        let room = c.req("POST", "/v1/rooms", Some(json!({"options": {"teams": [1, 1]}}))).await.unwrap();
        let rid = room["id"].as_str().unwrap().to_string();
        d.req("POST", "/v1/rooms/join", Some(json!({"code": room["code"]}))).await.unwrap();
        c.col.wait("room.updated with 2 members", 10, |ch, v| ch == "proto-event" && v["type"] == "room.updated" && v["data"]["members"].as_array().is_some_and(|m| m.len() == 2)).await;
        d.req("POST", &format!("/v1/rooms/{rid}/ready"), Some(json!({"ready": true}))).await.unwrap();
        let m = c.req("POST", &format!("/v1/rooms/{rid}/start"), None).await.unwrap();
        let mid = m["id"].as_str().unwrap().to_string();
        c.conn().runner.on_match(&m);
        let failed = c.col.wait("loadout-failed reported", 30, |ch, v| ch == "proto-runner" && v["match_id"] == mid.as_str() && v["phase"] == "failed").await;
        assert_eq!(failed["code"], "loadout-failed", "{failed}");
        assert_eq!(failed["result_uploaded"], true);
    });
}

/// 1.1.6: a private room whose form offers a draft. Two launchers (fake games listing five made-up stages) declare
/// the draft screen, the match starts in `drafting`, each player acts when its launcher says it may (what the draft
/// screen does), the match becomes ready with the drafted stage in its rules and in the game's settings, and the
/// games report a record on that stage. Needs a server whose rooms offer a `draft` option (the reference server's
/// local configuration does).
#[test]
#[ignore]
fn a_room_match_is_drafted_then_launched_on_the_drafted_stage() {
    let Some(_) = host() else { return eprintln!("KALAMAR_TEST_HOST is not set: skipped") };
    rt().block_on(async {
        const STAGES: [&str; 5] = ["Draft_A", "Draft_B", "Draft_C", "Draft_D", "Draft_E"];
        let a = Launcher::with_stages("draft-a", &STAGES).await;
        let b = Launcher::with_stages("draft-b", &STAGES).await;
        a.login_guest("Ana").await;
        b.login_guest("Bo").await;
        let (m, _) = a.conn().client.cached_meta().unwrap();
        let Some(draft) = m.pointer("/rooms/options").and_then(Value::as_array).and_then(|o| o.iter().find(|f| f["key"] == "draft")).and_then(|f| f["values"][0]["id"].as_str()).map(str::to_string) else {
            return eprintln!("the server's rooms offer no draft: skipped");
        };
        assert!(a.conn().client.features().iter().any(|f| f == "draft"), "the area is declared and known");
        let room = a.req("POST", "/v1/rooms", Some(json!({"options": {"teams": [1, 1], "draft": draft}}))).await.unwrap();
        let (rid, code) = (room["id"].as_str().unwrap().to_string(), room["code"].as_str().unwrap().to_string());
        b.req("POST", "/v1/rooms/join", Some(json!({"code": code}))).await.unwrap();
        b.req("POST", &format!("/v1/rooms/{rid}/ready"), Some(json!({"ready": true}))).await.unwrap();
        let started = a.req("POST", &format!("/v1/rooms/{rid}/start"), None).await.unwrap();
        assert_eq!(started["state"], "drafting", "{started}");
        let mid = started["id"].as_str().unwrap().to_string();
        let turns = started["draft"]["turns"].as_array().map(|t| t.iter().map(|x| format!("{}:{}", x["action"], x["teams"])).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        println!("draft {}: {} options, turns {turns}", started["draft"]["preset"], started["draft"]["options"].as_array().map(Vec::len).unwrap_or(0));
        let mut acted = [0, 0];
        let end = Instant::now() + Duration::from_secs(90);
        loop {
            let mut drafting = false;
            for (k, l) in [&a, &b].into_iter().enumerate() {
                let cur = l.req("GET", "/v1/matches/current", None).await.unwrap()["match"].clone();
                if cur["state"] != "drafting" {
                    continue;
                }
                drafting = true;
                let d = &cur["draft"];
                if d["you"]["can_act"] != true {
                    continue;
                }
                let t = &d["turns"][d["current"].as_u64().unwrap() as usize];
                let n = t["count"].as_u64().unwrap_or(1) as usize;
                let ids: Vec<Value> = d["options"].as_array().unwrap().iter().filter(|o| o["kind"] == t["kind"] && o["state"] == "available").take(n).map(|o| o["id"].clone()).collect();
                let r = l.req("POST", &format!("/v1/matches/{mid}/draft"), Some(json!({"turn": d["current"], "choices": ids}))).await;
                let r = r.unwrap_or_else(|e| panic!("draft action: {}", e.message()));
                println!("{} {} {:?} -> current {}", ["Ana", "Bo"][k], t["action"], ids, r["draft"]["current"]);
                acted[k] += 1;
            }
            if !drafting {
                break;
            }
            assert!(Instant::now() < end, "the draft did not end");
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        assert!(acted[0] >= 1 && acted[1] >= 1, "both players banned: {acted:?}");
        // The contract carries the drafted stage, for both launchers.
        for l in [&a, &b] {
            let ready = l.col.wait("match.ready", 20, |c, v| c == "proto-event" && v["type"] == "match.ready" && v["data"]["id"] == mid.as_str()).await;
            let stage = ready["data"]["draft"]["result"]["stage"].as_str().expect("a drafted stage").to_string();
            assert!(STAGES.contains(&stage.as_str()));
            assert_eq!(ready["data"]["rules"]["stage"], json!(stage));
            let rules = ready["data"]["game"]["rules"].as_str().unwrap();
            assert!(rules.ends_with(&format!(",stage={stage}")), "{rules}");
            println!("ready: stage {stage}, rules {rules}");
        }
        let done = a.col.wait("match.completed", 60, |c, v| c == "proto-event" && v["type"] == "match.completed" && v["data"]["id"] == mid.as_str()).await;
        let o = &done["data"]["outcome"];
        println!("outcome: {} on {}", o["decision"], o["record"]["stage_scene"]);
        assert_eq!(o["record"]["stage_scene"], done["data"]["rules"]["stage"]);
        for l in [&a, &b] {
            assert!(l.env().iter().any(|(k, v)| k == "SPLAT3_MATCH_RULES" && v.ends_with(&format!(",stage={}", o["record"]["stage_scene"].as_str().unwrap_or("?")))), "contract 2: every game got the stage");
        }
    });
}

/// Protocol 1.1.9 against a server that lists several relays (skipped otherwise; their addresses and ports come from
/// its meta): the launcher measures each relay in the background after the login and declares the round trips with its
/// game; with `KALAMAR_TEST_EXPECT_RELOAD=1` it then waits for the operator's `admin reload` (run it by hand within
/// 40 s) and reads the new meta by itself.
#[test]
#[ignore]
fn several_relays_are_measured_and_a_reload_reaches_the_launcher() {
    use super::relayping::RelayState;
    if host().is_none() {
        return eprintln!("KALAMAR_TEST_HOST is not set: skipped");
    }
    rt().block_on(async {
        let a = Launcher::new("relays", false).await;
        a.login_guest("Relais").await;
        let c = a.conn();
        let (meta, _) = c.client.cached_meta().unwrap();
        let relays = super::relayping::relays_of(&meta);
        if relays.len() < 2 {
            return eprintln!("the server lists {} relay(s): skipped", relays.len());
        }
        // The stream's resync starts the measurement; it ends 2 s after its HELLOs.
        let end = Instant::now() + Duration::from_secs(15);
        let view = loop {
            let v = c.relays.view();
            if !v.is_empty() && v.iter().all(|r| !matches!(r.state, RelayState::Pending | RelayState::Unmeasured)) {
                break v;
            }
            assert!(Instant::now() < end, "the relays were not measured: {v:?}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        eprintln!("relays: {view:?}");
        assert_eq!(view.len(), super::relayping::retained(&relays).len(), "every relay address is measured once");
        assert!(view.iter().any(|r| r.state == RelayState::Ok), "at least one relay answers");
        let rtt = c.relays.known().unwrap();
        assert_eq!(rtt.len(), relays.len(), "every listed relay is declared");
        // Declared again from the kept measurement (no network but the PUT).
        let before = Instant::now();
        c.relays.declare().await.unwrap();
        assert!(before.elapsed() < Duration::from_millis(1500), "the kept measurement is reused");
        if std::env::var("KALAMAR_TEST_EXPECT_RELOAD").is_err() {
            return;
        }
        let revision = meta["revision"].as_u64().unwrap_or(0);
        eprintln!("waiting for `admin reload` (revision {revision}) ...");
        a.col.wait("meta.changed", 40, is_event("meta.changed")).await;
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let now = c.client.cached_meta().and_then(|(m, _)| m["revision"].as_u64()).unwrap_or(0);
            if now > revision {
                eprintln!("meta read again: revision {now}, name {}", c.client.cached_meta().unwrap().0["name"]);
                break;
            }
            assert!(Instant::now() < end, "the launcher did not read the new meta");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
}
