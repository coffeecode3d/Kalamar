//! The match runner against a stand-in service on loopback: a server cannot start the game by itself (the
//! review's zero-click launch, `docs/security-model.md`), the usual flows still start it without a click, and a relay
//! the server never announced is refused.

use super::client::Client;
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
/// stands for; `closes`: how often the runner closed it; `close_fails`: closing it fails.
#[derive(Default)]
struct Game {
    launches: AtomicUsize,
    open: Mutex<Option<OpenGame>>,
    closes: AtomicUsize,
    close_fails: AtomicBool,
}

impl GameHost for Game {
    fn launch(&self, _: Vec<(String, String)>) -> Result<(), String> {
        self.launches.fetch_add(1, SeqCst);
        Ok(())
    }
    fn status(&self) -> GameRun {
        GameRun { running: false, exited: self.launches.load(SeqCst) > 0, exit_code: Some(0), stopped_by_user: false }
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
