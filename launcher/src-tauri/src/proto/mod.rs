//! Client of the Kalamar community protocol 1.0 (the specification lives with the reference server:
//! `kalamar-server/docs/protocol/`). A data-driven client: it shows only what a server declares and renders the
//! server's data without interpreting it.
//!
//! | module | what |
//! |---|---|
//! | [`url`] | the one-line form (`host[:port]`), API URLs, the loopback exception to HTTPS |
//! | [`discovery`] | the discovery document and the server card |
//! | [`meta`] | `GET /v1/meta` checks, areas, the path → area map that keeps undeclared areas uncalled |
//! | [`http`] | one HTTPS request, the error model |
//! | [`tokens`] | the stored session (DPAPI-encrypted file in the data folder) |
//! | [`client`] | one server: session, refresh rotation, meta and clock |
//! | [`oauth`] | `oauth_loopback` logins (PKCE, loopback redirect) |
//! | [`events`] | the event WebSocket: heartbeat, resume, backoff |
//! | [`contract`] | match contract `lan-auto-match` 1: checks and the game's environment |
//! | [`relayping`] | 1.1.9: the latency to each relay a server lists, declared with the game's capabilities |
//! | [`consent`] | which matches start without a question: those this launcher engaged in during this run |
//! | [`runner`] | the match on this machine: launch (asked first when not engaged), status mirror, result upload |
//! | [`glue`] | the Tauri side: the managed [`Hub`], the commands, the game host |
//!
//! **Network.** With no server added the launcher makes no request at all. With a server, requests start when the
//! player opens the community page, or at start-up for a server set to "stay connected in the background" (meta, then
//! the event stream once logged in), and stop when they log out, choose another server or remove it; the player count
//! ([`population`]) is read while the Play or Community page is on screen. Once logged in to a server that lists
//! several relays, [`relayping`] looks their names up and sends each relay address one UDP `HELLO`, in the background
//! and within the limits written there (at most 16 per server and one per address per ten minutes).

// Errors carry the server's whole error object and a preview carries the server's description: both are rare next
// to the network round trip they come from, so their size does not matter.
#![allow(clippy::result_large_err, clippy::large_enum_variant)]

pub mod account;
pub mod client;
pub mod consent;
pub mod contract;
pub mod discovery;
pub mod engine;
pub mod events;
pub mod freshness;
pub mod glue;
pub mod http;
pub mod meta;
pub mod oauth;
pub mod population;
pub mod progress;
pub mod relayping;
pub mod runner;
pub mod tokens;
pub mod url;

#[cfg(test)]
mod consent_tests;
#[cfg(test)]
mod live_tests;

/// The version of the draft object (`draft.md`, 1.1.6) the community page renders, declared in `PUT /v1/me/game`.
pub const DRAFT_VERSION: u32 = 1;
/// The version of the spectator seat (`spectators.md`, 1.1.7) this launcher runs, declared as `launcher.spectate` in
/// `PUT /v1/me/game` when the engine has places for spectators.
pub const SPECTATE_VERSION: u32 = 1;

use client::Client;
use events::{Handler, Stream, StreamInfo};
use runner::{GameHost, Runner, RunnerSink, RunnerStatus};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Where the protocol's pushes go: the page (Tauri events) in the launcher, a collector in tests.
pub trait Sink: Send + Sync {
    /// `proto-event` (a protocol event), `proto-stream` (stream state), `proto-session` (logged out by the server),
    /// `proto-runner` (the match on this machine), `proto-relays` (the latency to a server's relays, `relayping`).
    fn emit(&self, channel: &str, payload: Value);
}

/// One server in use: its client, its match runner and its event stream.
pub struct Conn {
    pub client: Arc<Client>,
    pub runner: Arc<Runner>,
    /// 1.1.9: the latency to this server's relays, measured and declared in the background.
    pub relays: Arc<relayping::RelayJob>,
    stream: Mutex<Option<Stream>>,
}

impl Conn {
    pub fn stream_info(&self) -> Option<StreamInfo> {
        self.stream.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(Stream::info)
    }

    /// Stops the event stream and a relay measurement under way (logout, another server chosen, removal).
    pub fn stop_stream(&self) {
        self.relays.cancel();
        if let Some(s) = self.stream.lock().unwrap_or_else(|p| p.into_inner()).take() {
            s.stop();
        }
    }

    /// Drops the stream's connection; it reconnects and resumes.
    pub fn reconnect(&self) {
        if let Some(s) = self.stream.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            s.reconnect();
        }
    }
}

/// Every server the player has used during this run. Created empty at start-up (no file read, no network).
pub struct Hub {
    pub data_dir: PathBuf,
    sink: Arc<dyn Sink>,
    host: Arc<dyn GameHost>,
    resolver: Arc<dyn crate::servers::Resolver + Send + Sync>,
    /// What every server's relay measurement shares (one HELLO per address per ten minutes, whoever asks).
    prober: Arc<relayping::Prober>,
    conns: Mutex<HashMap<String, Arc<Conn>>>,
}

struct ConnHandler {
    server: String,
    sink: Arc<dyn Sink>,
    client: Arc<Client>,
    runner: Arc<Runner>,
    relays: Arc<relayping::RelayJob>,
}

fn with_server(server: &str, v: &Value) -> Value {
    let mut out = json!({"server": server});
    if let (Some(o), Some(src)) = (out.as_object_mut(), v.as_object()) {
        for (k, x) in src {
            o.insert(k.clone(), x.clone());
        }
    }
    out
}

impl Handler for ConnHandler {
    fn event(&self, ev: &Value) {
        let kind = ev.get("type").and_then(Value::as_str).unwrap_or("");
        let data = ev.get("data").cloned().unwrap_or(Value::Null);
        if kind.starts_with("match.") {
            self.runner.on_match(&data);
        } else if kind == "series.updated" {
            self.runner.on_series(&data);
        } else if kind == "me.updated" {
            self.client.remember_me(&data);
        } else if kind == "meta.changed" {
            // The server's offer changed while it runs (a reloaded configuration): its meta is read again (after a
            // random delay, once per revision, one pass at a time) and the relays are measured and declared again when
            // their list changed (1.1.9). The page reads the new meta through its own `proto_overview`.
            self.relays.meta_changed(&data);
        }
        self.sink.emit("proto-event", with_server(&self.server, ev));
    }

    fn resync(&self) {
        self.sink.emit("proto-event", json!({"server": self.server, "type": "resync", "data": {}}));
        // The match this player is in (it may have become ready while the stream was down), and what this launcher can
        // run (`PUT /v1/me/game` at each new connection, with the relays' last values: measuring runs afterwards in
        // the background and declares again when it ends).
        let (client, runner, relays) = (self.client.clone(), self.runner.clone(), self.relays.clone());
        tokio::spawn(async move {
            let _ = relays.declare().await;
            relays.kick();
            if let Ok(r) = client.call("GET", "/v1/matches/current", http::Body::None, true).await
                && let Some(m) = r.body.get("match").filter(|m| m.is_object())
            {
                runner.on_match(m);
            }
        });
    }

    fn state(&self, info: &StreamInfo) {
        self.sink.emit("proto-stream", with_server(&self.server, &serde_json::to_value(info).unwrap_or(Value::Null)));
    }
}

/// `PUT /v1/me/game` from the engine's capabilities (its listing runs once per executable, off the async runtime), and
/// 1.1.9: how far each of the server's relays is from this computer, when measured (`relayping::RelayJob::known`;
/// nothing is measured here).
pub async fn declare_game(client: &Client, host: Arc<dyn GameHost>, relay_rtt: Option<relayping::Rtt>) -> Result<(), http::CallError> {
    // A game of another version than this launcher's build is not offered to any server: its build differs from every
    // current one, and no match may start with it (`GameHost::game_outdated`).
    if host.game_outdated() {
        return Ok(());
    }
    let build = host.build_id();
    let h = host.clone();
    let info = tokio::task::spawn_blocking(move || h.engine()).await.ok().flatten();
    let mut body = engine::game_body(&build, info.as_ref(), host.declare_rounds());
    if let Some(rtt) = relay_rtt {
        body["launcher"]["relay_rtt_ms"] = json!(rtt);
    }
    client.put_game(body).await
}

struct RunnerTo {
    server: String,
    sink: Arc<dyn Sink>,
}

impl RunnerSink for RunnerTo {
    fn runner(&self, status: &RunnerStatus) {
        self.sink.emit("proto-runner", with_server(&self.server, &serde_json::to_value(status).unwrap_or(Value::Null)));
    }
}

impl Hub {
    pub fn new(data_dir: PathBuf, sink: Arc<dyn Sink>, host: Arc<dyn GameHost>, resolver: Arc<dyn crate::servers::Resolver + Send + Sync>) -> Hub {
        let prober = Arc::new(relayping::Prober::new(resolver.clone()));
        Hub { data_dir, sink, host, resolver, prober, conns: Mutex::new(HashMap::new()) }
    }

    /// With the operating system's resolver for relay names (what the launcher uses).
    pub fn with_system_resolver(data_dir: PathBuf, sink: Arc<dyn Sink>, host: Arc<dyn GameHost>) -> Hub {
        Hub::new(data_dir, sink, host, Arc::new(crate::servers::SystemResolver))
    }

    pub fn host(&self) -> Arc<dyn GameHost> {
        self.host.clone()
    }

    /// The folder of a server's own files (`online\<id>`).
    pub fn server_dir(&self, id: &str) -> PathBuf {
        crate::online::online_dir(&self.data_dir).join(id)
    }

    /// The connection of a saved server that has a service (`api`). Created on first use; replaced when its API changed.
    pub fn conn(&self, id: &str, lang: &str) -> Result<Arc<Conn>, String> {
        let store = crate::online::load(&self.data_dir);
        let server = store.servers.iter().find(|s| s.id == id).ok_or("unknown server")?;
        let api = server.api.clone().ok_or("this server has no community service (relay only)")?;
        let api = url::parse_api(&api).map_err(|e| e.message().to_string())?;
        let mut g = self.conns.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(c) = g.get(id).filter(|c| c.client.api == api) {
            c.client.set_lang(lang);
            return Ok(c.clone());
        }
        if let Some(old) = g.remove(id) {
            old.stop_stream();
        }
        let client = Arc::new(Client::new(id, &api, self.server_dir(id), lang)?);
        let sink = self.sink.clone();
        client.on_session_lost(Arc::new(move |server: &str, reason: &str| {
            sink.emit("proto-session", json!({"server": server, "logged_in": false, "reason": reason}));
        }));
        let runner = Runner::new(client.clone(), self.host.clone(), Arc::new(RunnerTo { server: id.to_string(), sink: self.sink.clone() }), self.resolver.clone());
        let relays = relayping::RelayJob::new(id, client.clone(), self.host.clone(), self.sink.clone(), self.prober.clone(), relayping::SPREAD);
        let conn = Arc::new(Conn { client, runner, relays, stream: Mutex::new(None) });
        g.insert(id.to_string(), conn.clone());
        Ok(conn)
    }

    /// Starts the event stream when the player is logged in and none is running. Must be called on the tokio runtime.
    pub fn ensure_stream(&self, id: &str, conn: &Arc<Conn>) {
        if !conn.client.session_info().logged_in {
            return;
        }
        let mut g = conn.stream.lock().unwrap_or_else(|p| p.into_inner());
        if g.as_ref().is_some_and(|s| !s.stopped()) {
            return;
        }
        let handler = Arc::new(ConnHandler { server: id.to_string(), sink: self.sink.clone(), client: conn.client.clone(), runner: conn.runner.clone(), relays: conn.relays.clone() });
        *g = Some(events::spawn(conn.client.clone(), handler));
    }

    /// The connection of a server when one was already made (nothing is created).
    pub fn existing(&self, id: &str) -> Option<Arc<Conn>> {
        self.conns.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
    }

    /// Forgets a server's connection (logout or removal): its stream stops.
    pub fn drop_conn(&self, id: &str) {
        if let Some(c) = self.conns.lock().unwrap_or_else(|p| p.into_inner()).remove(id) {
            c.stop_stream();
        }
    }
}
