//! The match runner against a stand-in service on loopback: a server cannot start the game by itself (the
//! review's zero-click launch, `docs/security-model.md`), the usual flows still start it without a click, and a relay
//! the server never announced is refused. Also the window to change weapon before a launch (phase `weapon`): when it
//! is offered, how long it lasts, and what ends it.

use super::client::Client;
use super::engine::EngineInfo;
use super::runner::{GameHost, GameRun, Runner, RunnerSink, RunnerStatus};
use crate::launch::OpenGame;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RELAY_PORT: u16 = 47313;

/// A stand-in service: `GET /v1/meta` lists one relay (127.0.0.1:47313), the room answers name `r_1`, every other
/// request succeeds with `{}`. Every request is noted (method, path, body).
struct Api {
    port: u16,
    seen: Arc<Mutex<Vec<(String, String, Value)>>>,
}

impl Api {
    fn start() -> Api {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { continue };
                let mut buf = Vec::new();
                let mut b = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut b) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => buf.extend_from_slice(&b[..k]),
                    }
                }
                let Some(start) = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4) else { continue };
                let head = String::from_utf8_lossy(&buf[..start]).to_string();
                let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                while buf.len() < start + len {
                    match s.read(&mut b) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => buf.extend_from_slice(&b[..k]),
                    }
                }
                let mut first = head.lines().next().unwrap_or("").split(' ');
                let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
                let body: Value = serde_json::from_slice(&buf[start..(start + len).min(buf.len())]).unwrap_or(Value::Null);
                s2.lock().unwrap().push((method.clone(), path.clone(), body));
                let answer = match (method.as_str(), path.as_str()) {
                    ("GET", "/v1/meta") => json!({"name": "stand-in", "time": "2026-11-13T00:00:00Z", "features": ["queues", "rooms"],
                        "relays": [{"id": "main", "host": "127.0.0.1", "port": RELAY_PORT}]}),
                    ("POST", "/v1/rooms/join") | ("POST", "/v1/rooms") => json!({"id": "r_1", "code": "ABC123"}),
                    _ => json!({}),
                };
                let text = answer.to_string();
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
                let _ = s.write_all(resp.as_bytes());
            }
        });
        Api { port, seen }
    }

    fn posts(&self, suffix: &str) -> Vec<Value> {
        self.seen.lock().unwrap().iter().filter(|(m, p, _)| m == "POST" && p.ends_with(suffix)).map(|(_, _, b)| b.clone()).collect()
    }
}

/// A game that only counts its launches (it then reads as exited at once). `open`: what the game the player has open
/// stands for; `closes`: how often the runner closed it; `close_fails`: closing it fails. `applies`: the launcher
/// applies its profile (the window to change weapon); `info`: the engine's list (contract 2, rounds, loadouts,
/// spectators); `stays`: the game stays open after its first round (a game session), whose result it writes at once.
#[derive(Default)]
struct Game {
    launches: AtomicUsize,
    open: Mutex<Option<OpenGame>>,
    closes: AtomicUsize,
    close_fails: AtomicBool,
    applies: AtomicBool,
    info: Mutex<Option<EngineInfo>>,
    stays: AtomicBool,
}

fn var(env: &[(String, String)], k: &str) -> String {
    env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default()
}

impl GameHost for Game {
    fn launch(&self, env: Vec<(String, String)>) -> Result<(), String> {
        self.launches.fetch_add(1, SeqCst);
        if self.stays.load(SeqCst) {
            // The round is over at once; the game waits for the next one.
            let id = var(&env, "SPLAT3_MATCH_ID");
            let status = var(&env, "SPLAT3_MATCH_STATUS").replace("{id}", &id);
            std::fs::write(&status, json!({"version": 1, "match_id": id, "state": "waiting-command"}).to_string()).unwrap();
            std::fs::write(var(&env, "SPLAT3_MATCH_RESULT").replace("{id}", &id), json!({"version": 1, "match_id": id}).to_string()).unwrap();
        }
        Ok(())
    }
    fn status(&self) -> GameRun {
        let launched = self.launches.load(SeqCst) > 0;
        if self.stays.load(SeqCst) {
            return GameRun { running: launched, exited: false, exit_code: None, stopped_by_user: false };
        }
        GameRun { running: false, exited: launched, exit_code: Some(0), stopped_by_user: false }
    }
    fn applies_profile(&self) -> bool {
        self.applies.load(SeqCst)
    }
    fn engine(&self) -> Option<EngineInfo> {
        self.info.lock().unwrap().clone()
    }
    fn stop(&self) {}
    fn open_game(&self) -> OpenGame {
        self.open.lock().unwrap().unwrap_or(OpenGame::Nothing)
    }
    fn close_open_game(&self) -> Result<(), String> {
        self.closes.fetch_add(1, SeqCst);
        if self.close_fails.load(SeqCst) {
            return Err("the game that is open could not be closed: access denied".into());
        }
        *self.open.lock().unwrap() = None;
        Ok(())
    }
    fn build_id(&self) -> String {
        "test".into()
    }
}

#[derive(Default)]
struct Statuses(Mutex<Vec<RunnerStatus>>);

impl RunnerSink for Statuses {
    fn runner(&self, s: &RunnerStatus) {
        self.0.lock().unwrap().push(s.clone());
    }
}

impl Statuses {
    fn phases(&self) -> Vec<String> {
        self.0.lock().unwrap().iter().map(|s| s.phase.clone()).collect()
    }
    fn last(&self) -> Option<RunnerStatus> {
        self.0.lock().unwrap().last().cloned()
    }
    fn phases_of(&self, id: &str) -> Vec<String> {
        self.0.lock().unwrap().iter().filter(|s| s.match_id == id).map(|s| s.phase.clone()).collect()
    }
    /// The first status published in this phase.
    fn first(&self, phase: &str) -> Option<RunnerStatus> {
        self.0.lock().unwrap().iter().find(|s| s.phase == phase).cloned()
    }
}

struct Names;

impl crate::servers::Resolver for Names {
    fn lookup_v4(&self, _: &str) -> Result<Vec<Ipv4Addr>, String> {
        Err("no names here".into())
    }
}

struct Setup {
    api: Api,
    client: Arc<Client>,
    game: Arc<Game>,
    sink: Arc<Statuses>,
    runner: Arc<Runner>,
    dir: PathBuf,
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn setup() -> Setup {
    setup_with(Duration::from_millis(600))
}

/// `ask`: how long the launcher's question waits for an answer.
fn setup_with(ask: Duration) -> Setup {
    let api = Api::start();
    let dir = std::env::temp_dir().join(format!("s3l-consent-{}", crate::util::random_hex(4)));
    let cdir = dir.join("online").join("0a1b2c3d");
    std::fs::create_dir_all(&cdir).unwrap();
    let client = Arc::new(Client::new("0a1b2c3d", &format!("http://127.0.0.1:{}", api.port), cdir, "en").unwrap());
    client.accept_session(&json!({"access_token": "kat1.testaccess", "refresh_token": "krt1.testrefresh", "me": {"id": "u_a", "display_name": "Ana"}}), "guest").unwrap();
    let game = Arc::new(Game::default());
    let sink = Arc::new(Statuses::default());
    let mut runner = Runner::new(client.clone(), game.clone(), sink.clone(), Arc::new(Names));
    Arc::get_mut(&mut runner).unwrap().ask_max = ask;
    Setup { api, client, game, sink, runner, dir }
}

/// A ready match as a server pushes it (`match.ready`), this player hosting, on the announced relay.
fn ready(id: &str, origin: Value, relay_host: &str) -> Value {
    json!({
        "id": id, "state": "ready", "origin": origin,
        "rules": {"teams": [1, 1], "mode": "turf", "stage": null, "options": {}, "rated": false},
        "participants": [],
        "you": {"user_id": "u_a", "member_name": "Ana", "team": "alpha", "role": "host"},
        "game": {"contract": {"id": "lan-auto-match", "version": 1}, "match_id": "ZEROCLICK0",
            "relay": {"host": relay_host, "port": RELAY_PORT, "room": "ZEROCLICK0"},
            "players": 2, "rules": "players=2,teams=Ana:A,Evil:B,mode=turf", "host_name": "Ana", "timeouts": null},
        "deadlines": {"accept": null, "launch": null, "result": null}
    })
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

async fn until(what: &str, secs: u64, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

/// The review's zero-click launch (`hostile_server.py`): right after the event stream opens, the server pushes a ready
/// match the player never queued for, joined or accepted. The launcher used to start the game 20 ms later; now it asks,
/// and with no answer the game is never started (the server gets `launcher-cancelled`). A launcher that just started
/// again is in the same place: nothing of the earlier run is remembered, so a match "in progress" found then asks too.
#[test]
fn a_match_pushed_by_the_server_alone_never_starts_the_game() {
    let s = setup();
    rt().block_on(async {
        s.runner.on_match(&ready("m_zeroclick0", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1"));
        until("the question", 5, || s.sink.phases().contains(&"confirm".to_string())).await;
        let q = s.sink.0.lock().unwrap().iter().find(|x| x.phase == "confirm").cloned().unwrap();
        assert_eq!((q.ask.as_deref(), q.relay.as_deref()), (Some("not-joined"), Some("127.0.0.1:47313")));
        until("the refusal", 5, || s.sink.last().is_some_and(|x| x.phase == "refused")).await;
        assert_eq!(s.game.launches.load(SeqCst), 0, "the game was started");
        assert_eq!(s.sink.last().unwrap().code.as_deref(), Some("launcher-cancelled"));
        until("the report", 5, || !s.api.posts("/v1/matches/m_zeroclick0/result").is_empty()).await;
        assert_eq!(s.api.posts("/v1/matches/m_zeroclick0/result")[0]["error"], "launcher-cancelled");
        assert!(s.api.posts("/v1/matches/m_zeroclick0/launch").is_empty(), "no launch reported");
        // The same match pushed again (another event, a resync) is not asked again: it was refused.
        s.runner.on_match(&ready("m_zeroclick0", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1"));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(s.game.launches.load(SeqCst), 0);
    });
}

/// The usual queue: ticket, match found, the player accepts, and the game starts without another click.
#[test]
fn the_queue_flow_still_starts_the_game_without_a_click() {
    let s = setup();
    rt().block_on(async {
        s.client.call("POST", "/v1/queues/duel/tickets", super::http::Body::Json(json!({})), true).await.unwrap();
        s.client.call("POST", "/v1/matches/m_q1/accept", super::http::Body::None, true).await.unwrap();
        s.runner.on_match(&ready("m_q1", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1"));
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
        assert!(!s.sink.phases().contains(&"confirm".to_string()), "no question: {:?}", s.sink.phases());
        until("the launch report", 5, || !s.api.posts("/v1/matches/m_q1/launch").is_empty()).await;
    });
}

/// A room joined from this launcher: its match starts without a click.
#[test]
fn a_room_joined_here_starts_its_match_without_a_click() {
    let s = setup();
    rt().block_on(async {
        s.client.call("POST", "/v1/rooms/join", super::http::Body::Json(json!({"code": "abc123"})), true).await.unwrap();
        s.runner.on_match(&ready("m_r1", json!({"kind": "room", "room_id": "r_1"}), "127.0.0.1"));
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
        assert!(!s.sink.phases().contains(&"confirm".to_string()));
    });
}

/// A match not joined here starts once the player says yes.
#[test]
fn the_player_can_start_a_match_it_did_not_join_here() {
    let s = setup_with(Duration::from_secs(10));
    rt().block_on(async {
        s.runner.on_match(&ready("m_y1", json!({"kind": "room", "room_id": "r_other"}), "127.0.0.1"));
        until("the question", 5, || s.sink.phases().contains(&"confirm".to_string())).await;
        assert_eq!(s.game.launches.load(SeqCst), 0);
        s.runner.answer("m_y1", true);
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
    });
}

/// A match on a relay the server never announced is refused, even one the player accepted (the game would talk to
/// any address the server names).
#[test]
fn a_relay_the_server_never_announced_is_refused() {
    let s = setup();
    rt().block_on(async {
        s.client.call("POST", "/v1/matches/m_x1/accept", super::http::Body::None, true).await.unwrap();
        s.runner.on_match(&ready("m_x1", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.2"));
        until("the refusal", 5, || s.sink.last().is_some_and(|x| x.phase == "refused")).await;
        let last = s.sink.last().unwrap();
        assert_eq!(last.code.as_deref(), Some("launcher-unsupported-rules"));
        assert!(last.reason.unwrap_or_default().contains("127.0.0.2:47313"));
        assert!(!s.sink.phases().contains(&"confirm".to_string()));
        assert_eq!(s.game.launches.load(SeqCst), 0);
    });
}

/// A match found while the player has a game open for solo play: that game is closed, then the match starts within the
/// same flow, and the page is told ("closing") before the game starts.
#[test]
fn a_game_open_for_solo_play_is_closed_for_the_match() {
    let s = setup();
    *s.game.open.lock().unwrap() = Some(OpenGame::Solo);
    rt().block_on(async {
        s.client.call("POST", "/v1/queues/duel/tickets", super::http::Body::Json(json!({})), true).await.unwrap();
        s.client.call("POST", "/v1/matches/m_s1/accept", super::http::Body::None, true).await.unwrap();
        s.runner.on_match(&ready("m_s1", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1"));
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
        assert_eq!(s.game.closes.load(SeqCst), 1);
        let phases = s.sink.phases();
        let at = |p: &str| phases.iter().position(|x| x == p);
        assert!(at("closing").is_some() && at("closing") < at("launching"), "closing comes before launching: {phases:?}");
        until("the launch report", 5, || !s.api.posts("/v1/matches/m_s1/launch").is_empty()).await;
        assert!(s.api.posts("/v1/matches/m_s1/result").is_empty(), "no error reported");
    });
}

/// The launcher's own match in progress is not closed: the flow keeps its earlier behavior.
#[test]
fn a_match_in_progress_is_not_closed_for_another() {
    let s = setup();
    *s.game.open.lock().unwrap() = Some(OpenGame::Match);
    rt().block_on(async {
        s.client.call("POST", "/v1/queues/duel/tickets", super::http::Body::Json(json!({})), true).await.unwrap();
        s.client.call("POST", "/v1/matches/m_s2/accept", super::http::Body::None, true).await.unwrap();
        s.runner.on_match(&ready("m_s2", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1"));
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
        assert_eq!(s.game.closes.load(SeqCst), 0, "the match in progress was closed");
        assert!(!s.sink.phases().contains(&"closing".to_string()));
    });
}

/// A game that cannot be closed stops the match with its own code, and the server is told like for any failed launch.
#[test]
fn a_game_that_cannot_be_closed_fails_the_match_and_is_reported() {
    let s = setup();
    *s.game.open.lock().unwrap() = Some(OpenGame::Solo);
    s.game.close_fails.store(true, SeqCst);
    rt().block_on(async {
        s.client.call("POST", "/v1/queues/duel/tickets", super::http::Body::Json(json!({})), true).await.unwrap();
        s.client.call("POST", "/v1/matches/m_s3/accept", super::http::Body::None, true).await.unwrap();
        s.runner.on_match(&ready("m_s3", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1"));
        until("the failure", 5, || s.sink.last().is_some_and(|x| x.phase == "failed")).await;
        let last = s.sink.last().unwrap();
        assert_eq!(last.code.as_deref(), Some("game-not-closed"));
        assert!(last.reason.unwrap_or_default().contains("could not be closed"));
        assert_eq!(s.game.launches.load(SeqCst), 0, "the game was started anyway");
        until("the report", 5, || !s.api.posts("/v1/matches/m_s3/result").is_empty()).await;
        assert_eq!(s.api.posts("/v1/matches/m_s3/result")[0]["error"], "launcher-failed");
        assert!(s.api.posts("/v1/matches/m_s3/launch").is_empty(), "no launch reported");
    });
}

// ---- the window to change weapon before a launch (phase `weapon`) ----------------------------------------------------

/// The stand-in's clock (its meta's `time`): the runner's server clock while a test runs, a few milliseconds on.
fn t0() -> i64 {
    super::tokens::parse_time("2026-11-13T00:00:00Z").unwrap()
}

/// `secs` after [`t0`], as a match's deadline.
fn at(secs: i64) -> Value {
    json!(chrono::DateTime::from_timestamp(t0() + secs, 0).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// A launcher that applies its profile, with this window to change weapon.
fn setup_weapon(window: Duration) -> Setup {
    let mut s = setup();
    Arc::get_mut(&mut s.runner).unwrap().weapon_max = window;
    s.game.applies.store(true, SeqCst);
    s
}

/// The player queued and accepted the match here: it starts without the launcher's question.
async fn engage(s: &Setup, id: &str) {
    s.client.call("POST", "/v1/queues/duel/tickets", super::http::Body::Json(json!({})), true).await.unwrap();
    s.client.call("POST", &format!("/v1/matches/{id}/accept"), super::http::Body::None, true).await.unwrap();
}

/// An engine of contract 2: rounds in one game session, a loadout for one match, a place for a spectator.
fn engine_v2() -> EngineInfo {
    EngineInfo {
        fingerprint: "fake".into(),
        options: Some(json!({"format": "splat3-match-options", "version": 1, "modes": [{"key": "turf", "label": "Mode A"}],
            "teams": {"places_per_team": 4, "players_max": 8, "uneven": true}, "result": {"version": 2, "outcomes": ["completed", "aborted"]},
            "rounds": {"command": "next"}, "match_loadout": {"fields": ["weapon", "gear"]}, "spectators": {"max": 1}})),
        room_stages: None,
        note: None,
        lang: None,
    }
}

/// A ready match of contract 2 from the duel queue, in series `s_1`, whose game session stays open; this player hosts.
fn ready_v2(id: &str, round: &str) -> Value {
    json!({
        "id": id, "state": "ready", "origin": {"kind": "queue", "queue_id": "duel"}, "series": {"id": "s_1"},
        "rules": {"teams": [1, 1], "mode": "turf", "stage": null, "options": {}, "rated": false},
        "participants": [],
        "you": {"user_id": "u_a", "member_name": "Ana", "team": "alpha", "role": "host"},
        "game": {"contract": {"id": "lan-auto-match", "version": 2}, "match_id": round, "keep_open": true,
            "relay": {"host": "127.0.0.1", "port": RELAY_PORT, "room": "SESSION1"},
            "players": 2, "rules": "players=2,teams=Ana:A,Evil:B,mode=turf", "host_name": "Ana", "timeouts": null},
        "deadlines": {"accept": null, "launch": null, "result": null}
    })
}

/// The window holds the launch back until the player is ready; the page gets its end (server time), and nothing is
/// started or reported before "Ready".
#[test]
fn the_weapon_window_holds_the_launch_until_the_player_is_ready() {
    let s = setup_weapon(Duration::from_secs(30));
    rt().block_on(async {
        engage(&s, "m_w1").await;
        s.runner.on_match(&ready("m_w1", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1"));
        until("the window", 5, || s.sink.phases().contains(&"weapon".to_string())).await;
        let w = s.sink.first("weapon").unwrap();
        let end = super::tokens::parse_time(w.weapon_until.as_deref().unwrap()).unwrap();
        assert!((t0() + 29..=t0() + 31).contains(&end), "the window's end on the server's clock: {:?}", w.weapon_until);
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(s.game.launches.load(SeqCst), 0, "the game started during the window");
        assert!(s.api.posts("/v1/matches/m_w1/launch").is_empty());
        s.runner.weapon_ready("m_w1");
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
        let phases = s.sink.phases();
        let pos = |p: &str| phases.iter().position(|x| x == p);
        assert!(pos("weapon") < pos("resolving") && pos("resolving") < pos("launching"), "{phases:?}");
        let after = s.sink.0.lock().unwrap().iter().skip_while(|x| x.phase != "weapon").nth(1).cloned().unwrap();
        assert_eq!(after.weapon_until, None, "the end goes with the window");
        until("the launch report", 5, || !s.api.posts("/v1/matches/m_w1/launch").is_empty()).await;
    });
}

/// Without an answer the launch goes on when the window's time is up.
#[test]
fn the_weapon_window_ends_by_itself() {
    let s = setup_weapon(Duration::from_millis(600));
    rt().block_on(async {
        engage(&s, "m_w2").await;
        s.runner.on_match(&ready("m_w2", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1"));
        until("the window", 5, || s.sink.phases().contains(&"weapon".to_string())).await;
        let seen = Instant::now();
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
        assert!(seen.elapsed() >= Duration::from_millis(400), "the launch did not wait for the window: {:?}", seen.elapsed());
    });
}

/// The window ends 45 s before the launch deadline at the latest (the server voids the match of a player who has not
/// launched by then).
#[test]
fn the_weapon_window_is_cut_by_the_launch_deadline() {
    let s = setup_weapon(Duration::from_secs(60));
    rt().block_on(async {
        engage(&s, "m_w3").await;
        let mut m = ready("m_w3", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1");
        m["deadlines"]["launch"] = at(45 + 9);
        s.runner.on_match(&m);
        until("the window", 5, || s.sink.phases().contains(&"weapon".to_string())).await;
        let w = s.sink.first("weapon").unwrap();
        assert_eq!(super::tokens::parse_time(w.weapon_until.as_deref().unwrap()), Some(t0() + 9), "{:?}", w.weapon_until);
        s.runner.weapon_ready("m_w3");
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
    });
}

/// A deadline that would leave fewer than 5 s of window: no window, the game starts at once.
#[test]
fn a_weapon_window_cut_too_short_is_skipped() {
    let s = setup_weapon(Duration::from_secs(30));
    rt().block_on(async {
        engage(&s, "m_w4").await;
        let mut m = ready("m_w4", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1");
        m["deadlines"]["launch"] = at(45 + 3);
        s.runner.on_match(&m);
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
        assert!(!s.sink.phases().contains(&"weapon".to_string()), "{:?}", s.sink.phases());
    });
}

/// A launcher that does not apply its profile cannot change the game's weapon: no window.
#[test]
fn no_weapon_window_when_the_profile_is_not_applied() {
    let s = setup_weapon(Duration::from_secs(30));
    s.game.applies.store(false, SeqCst);
    rt().block_on(async {
        engage(&s, "m_w5").await;
        s.runner.on_match(&ready("m_w5", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1"));
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
        assert!(!s.sink.phases().contains(&"weapon".to_string()), "{:?}", s.sink.phases());
    });
}

/// A match found already running (the launcher started again during it) starts as before: no window.
#[test]
fn no_weapon_window_for_a_match_found_running() {
    let s = setup_weapon(Duration::from_secs(30));
    rt().block_on(async {
        engage(&s, "m_w6").await;
        let mut m = ready("m_w6", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1");
        m["state"] = json!("running");
        s.runner.on_match(&m);
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
        assert!(!s.sink.phases().contains(&"weapon".to_string()), "{:?}", s.sink.phases());
    });
}

/// A spectator plays no weapon: no window.
#[test]
fn no_weapon_window_for_a_spectator() {
    let s = setup_weapon(Duration::from_secs(30));
    *s.game.info.lock().unwrap() = Some(engine_v2());
    rt().block_on(async {
        engage(&s, "m_w7").await;
        let mut m = ready_v2("m_w7", "WATCH1");
        m["you"] = json!({"user_id": "u_a", "member_name": "Ana", "team": "spectator", "role": "join"});
        m["game"]["host_name"] = json!("Evil");
        m["game"]["keep_open"] = json!(false);
        s.runner.on_match(&m);
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
        assert_eq!(s.sink.last().unwrap().seat.as_deref(), Some("spectator"));
        assert!(!s.sink.phases().contains(&"weapon".to_string()), "{:?}", s.sink.phases());
    });
}

/// A match that imposes the weapon (`rules.loadout`): the weapon is chosen in `choosing` when a list is left to the
/// player, never in the window.
#[test]
fn no_weapon_window_when_the_match_imposes_the_weapon() {
    let s = setup_weapon(Duration::from_secs(30));
    *s.game.info.lock().unwrap() = Some(engine_v2());
    rt().block_on(async {
        engage(&s, "m_w8").await;
        let mut m = ready_v2("m_w8", "IMPOSED1");
        m["game"]["keep_open"] = json!(false);
        m["rules"]["loadout"] = json!({"all": {"weapon": {"allowed": ["40", "41"]}}});
        s.runner.on_match(&m);
        until("the loadout's choice", 5, || s.sink.phases().contains(&"choosing".to_string())).await;
        s.runner.pick_loadout("m_w8", [("weapon".to_string(), "41".to_string())].into());
        until("the launch", 5, || s.game.launches.load(SeqCst) == 1).await;
        assert!(!s.sink.phases().contains(&"weapon".to_string()), "{:?}", s.sink.phases());
        assert_eq!(s.sink.last().unwrap().loadout, Some(json!({"weapon": "41"})));
    });
}

/// The first game of a session has the window; the next game of the series, sent to the game still open, has none
/// (its weapon is the one the open game plays with).
#[test]
fn no_weapon_window_for_the_next_game_sent_to_the_open_game() {
    let s = setup_weapon(Duration::from_millis(300));
    *s.game.info.lock().unwrap() = Some(engine_v2());
    s.game.stays.store(true, SeqCst);
    rt().block_on(async {
        engage(&s, "m_g1").await;
        s.runner.on_match(&ready_v2("m_g1", "ROUND1"));
        until("the first game's result", 10, || s.sink.phases_of("m_g1").contains(&"waiting".to_string())).await;
        assert!(s.sink.phases_of("m_g1").contains(&"weapon".to_string()), "{:?}", s.sink.phases_of("m_g1"));
        let mut next = ready_v2("m_g2", "ROUND2");
        next["game"]["continue_from"] = json!("m_g1");
        s.runner.on_match(&next);
        until("the next game", 5, || s.sink.phases_of("m_g2").contains(&"continuing".to_string())).await;
        assert!(!s.sink.phases_of("m_g2").contains(&"weapon".to_string()), "{:?}", s.sink.phases_of("m_g2"));
        assert_eq!(s.game.launches.load(SeqCst), 1, "no new process for the next game");
    });
}

/// A match cancelled during the window ends it at once: the game is never started, nothing is reported.
#[test]
fn a_match_cancelled_during_the_weapon_window_is_not_launched() {
    let s = setup_weapon(Duration::from_secs(30));
    rt().block_on(async {
        engage(&s, "m_w9").await;
        s.runner.on_match(&ready("m_w9", json!({"kind": "queue", "queue_id": "duel"}), "127.0.0.1"));
        until("the window", 5, || s.sink.phases().contains(&"weapon".to_string())).await;
        s.runner.on_match(&json!({"id": "m_w9", "state": "cancelled"}));
        until("the end", 3, || s.sink.last().is_some_and(|x| x.phase == "closed")).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(s.game.launches.load(SeqCst), 0, "the game was started");
        assert!(s.api.posts("/v1/matches/m_w9/launch").is_empty(), "a launch was reported");
        assert!(s.api.posts("/v1/matches/m_w9/result").is_empty(), "a report was sent");
    });
}
