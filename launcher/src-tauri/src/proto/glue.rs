//! The Tauri side of the protocol client: the managed [`Hub`] (created empty at start-up), the game host that starts
//! matches through `commands::launch_protocol_match`, and the `proto_*` commands of the page. Tokens never cross into
//! the page; the page gets the server's declarations and data, the session's public part and the pushes.

use crate::error::AppError;
use crate::proto::client::SessionInfo;
use crate::proto::discovery::{self, Discovery, RelayAddr};
use crate::proto::events::StreamInfo;
use crate::proto::http::{self, Body, CallError};
use crate::proto::meta::{self, MetaCheck};
use crate::proto::runner::{GameHost, GameRun, RunnerStatus};
use crate::proto::url as purl;
use crate::proto::{Hub, Sink, oauth};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

struct TauriSink(AppHandle, Mutex<FrontOnce>);

/// The matches that already brought the launcher to the front (at most once each), and when it last came.
#[derive(Default)]
pub struct FrontOnce(std::collections::VecDeque<String>, Option<std::time::Instant>);

/// The launcher comes to the front at most once in this long, whatever the matches: a server that pushes a new match
/// every few seconds cannot keep taking the focus.
pub const FRONT_EVERY: Duration = Duration::from_secs(20);

impl FrontOnce {
    /// The match that waits for the player's answer, when a push is one: a found match (`match.found`), or the
    /// launcher's own question before a launch (runner phase `confirm`).
    pub fn wants_front(channel: &str, payload: &Value) -> Option<String> {
        let id = match channel {
            "proto-event" if payload.get("type").and_then(Value::as_str) == Some("match.found") => payload.pointer("/data/id"),
            "proto-runner" if payload.get("phase").and_then(Value::as_str) == Some("confirm") => payload.get("match_id"),
            _ => None,
        };
        id.and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 128).map(str::to_string)
    }

    /// Whether the launcher may come to the front for `id` now: never while a game runs (nothing is taken from it),
    /// once per match (a server that pushes the same match again cannot keep taking the focus), and once per
    /// [`FRONT_EVERY`] at most (nor can one that pushes many matches).
    pub fn take(&mut self, id: &str, game_running: bool, now: std::time::Instant) -> bool {
        if game_running || self.0.iter().any(|x| x == id) || self.1.is_some_and(|t| now.duration_since(t) < FRONT_EVERY) {
            return false;
        }
        if self.0.len() >= 64 {
            self.0.pop_front();
        }
        self.0.push_back(id.to_string());
        self.1 = Some(now);
        true
    }
}

impl Sink for TauriSink {
    fn emit(&self, channel: &str, payload: Value) {
        // A match waits for the player's answer: the launcher comes to the front (as after a match), once per match and
        // never while a game is running.
        if let Some(id) = FrontOnce::wants_front(channel, &payload) {
            let st = self.0.state::<crate::commands::AppState>();
            let running = [st.game.status(), st.game2.status()].iter().any(|s| s.state == crate::launch::RunState::Running || s.restarted);
            if self.1.lock().unwrap_or_else(|p| p.into_inner()).take(&id, running, std::time::Instant::now()) {
                crate::shell::bring_back(&self.0);
            }
        }
        let _ = self.0.emit(channel, payload);
    }
}

/// The game as the runner sees it: the launcher's own game manager (first player).
struct TauriHost(AppHandle);

impl GameHost for TauriHost {
    fn launch(&self, env: Vec<(String, String)>) -> Result<(), String> {
        crate::commands::launch_protocol_match(self.0.clone(), env).map(|_| ()).map_err(|e| match e {
            AppError::ProfileIncomplete(v) => format!("the profile lacks {} (a new save needs the whole appearance)", v.join(", ")),
            e => e.to_string(),
        })
    }
    fn status(&self) -> GameRun {
        let s = self.0.state::<crate::commands::AppState>().game.status();
        let running = s.state == crate::launch::RunState::Running;
        GameRun { running: running || s.restarted, exited: !running && !s.restarted, exit_code: s.exit_code, stopped_by_user: s.stopped_by_user }
    }
    fn stop(&self) {
        let _ = self.0.state::<crate::commands::AppState>().game.stop();
    }
    fn open_game(&self) -> crate::launch::OpenGame {
        crate::launch::open_game(&self.0.state::<crate::commands::AppState>().game.status())
    }
    fn close_open_game(&self) -> Result<(), String> {
        let game = self.0.state::<crate::commands::AppState>().game.clone();
        crate::launch::close_open_game(&game, crate::launch::CLOSE_GRACE, crate::launch::CLOSE_FORCE_WAIT).map(|_| ()).map_err(|e| e.to_string())
    }
    /// The build, never the installation (`engine::build_label`): the runtime commit the game was compiled from, else the
    /// one this launcher carries.
    fn build_id(&self) -> String {
        let (installed, bundled) = runtime_commits(&self.0);
        crate::proto::engine::build_label(installed.or(bundled).as_deref())
    }
    fn game_outdated(&self) -> bool {
        self.root().is_some_and(|r| crate::installer::gamever::is_outdated_root(&r))
    }
    fn engine(&self) -> Option<crate::proto::engine::EngineInfo> {
        let root = self.root()?;
        let exe = crate::install::exe_path(&root);
        let cwd = crate::install::userdata_dir(&root);
        let _ = std::fs::create_dir_all(&cwd);
        let list_lang = crate::proto::engine::list_lang(&lang(&self.0));
        Some(crate::proto::engine::engine_info_in(&self.0.state::<crate::commands::AppState>().data_dir, &exe, &cwd, Some(list_lang)))
    }
    fn data_dir(&self) -> Option<std::path::PathBuf> {
        Some(self.0.state::<crate::commands::AppState>().data_dir.clone())
    }
    /// The locker's weapon and gear, when the launcher applies its profile (else the save's own are unknown here).
    fn current_loadout(&self) -> std::collections::HashMap<String, String> {
        let st = self.0.state::<crate::commands::AppState>();
        if !st.config.lock().unwrap_or_else(|p| p.into_inner()).apply_profile {
            return Default::default();
        }
        let p = crate::profile::load_profile(&st.data_dir);
        [("weapon", p.weapon), ("gear.head", p.head), ("gear.clothes", p.clothes), ("gear.shoes", p.shoes)].into_iter().filter_map(|(k, v)| v.map(|v| (k.to_string(), v.to_string()))).collect()
    }
    fn current_sub_abilities(&self) -> std::collections::HashMap<String, Vec<Option<String>>> {
        let st = self.0.state::<crate::commands::AppState>();
        if !st.config.lock().unwrap_or_else(|p| p.into_inner()).apply_profile {
            return Default::default();
        }
        let p = crate::profile::load_profile(&st.data_dir);
        crate::profile::GEAR_PARTS.iter().filter_map(|part| p.sub_abilities.part(part).map(|s| (part.to_string(), s.clone()))).collect()
    }
    fn current_main_abilities(&self) -> std::collections::HashMap<String, String> {
        let st = self.0.state::<crate::commands::AppState>();
        if !st.config.lock().unwrap_or_else(|p| p.into_inner()).apply_profile {
            return Default::default();
        }
        let p = crate::profile::load_profile(&st.data_dir);
        crate::profile::GEAR_PARTS.iter().filter_map(|part| p.main_abilities.part(part).map(|s| (part.to_string(), s.clone()))).collect()
    }
}

impl TauriHost {
    fn root(&self) -> Option<std::path::PathBuf> {
        let st = self.0.state::<crate::commands::AppState>();
        st.config.lock().unwrap_or_else(|p| p.into_inner()).root.clone().map(std::path::PathBuf::from).or_else(crate::config::default_root)
    }
}

/// Built in `setup`: nothing is read and nothing is contacted until a `proto_*` command runs.
pub fn hub(app: AppHandle, data_dir: std::path::PathBuf) -> Hub {
    Hub::new(data_dir, Arc::new(TauriSink(app.clone(), Mutex::new(FrontOnce::default()))), Arc::new(TauriHost(app)), Arc::new(crate::servers::SystemResolver))
}

fn lang(app: &AppHandle) -> String {
    app.state::<crate::commands::AppState>().config.lock().unwrap_or_else(|p| p.into_inner()).language.clone()
}

/// A command's failure, in the shape of `AppError` (`kind`, `message`, `details`, `fields`) plus the protocol's
/// `code`, `status`, `retry_after_s` and `data` (`details` of the server's error object).
#[derive(Debug, Serialize)]
pub struct Fail {
    pub kind: String,
    pub message: String,
    pub details: Vec<String>,
    pub fields: Vec<crate::error::FieldError>,
    pub code: String,
    pub status: u16,
    pub retry_after_s: Option<u64>,
    pub data: Value,
}

impl Fail {
    pub fn new(kind: &str, code: &str, message: impl Into<String>) -> Fail {
        Fail { kind: kind.into(), message: message.into(), details: vec![code.into()], fields: vec![], code: code.into(), status: 0, retry_after_s: None, data: Value::Null }
    }
}

impl From<CallError> for Fail {
    fn from(e: CallError) -> Fail {
        match &e {
            CallError::Api(a) => Fail {
                kind: "protocol".into(),
                message: a.message.clone(),
                details: vec![a.code.clone(), a.status.to_string()],
                fields: a.field.iter().map(|f| crate::error::FieldError { key: f.clone(), message: a.code.clone() }).collect(),
                code: a.code.clone(),
                status: a.status,
                retry_after_s: a.retry_after_s,
                data: a.details.clone(),
            },
            CallError::Transport(m) => Fail::new("network", "network", m.clone()),
            CallError::Body(m) => Fail::new("bad_answer", "bad_answer", m.clone()),
        }
    }
}

impl From<AppError> for Fail {
    fn from(e: AppError) -> Fail {
        let v = serde_json::to_value(&e).unwrap_or(Value::Null);
        let fields = match &e {
            AppError::Validation(f) => f.clone(),
            _ => Vec::new(),
        };
        Fail {
            kind: e.kind().into(),
            message: e.to_string(),
            details: v.get("details").and_then(|d| serde_json::from_value(d.clone()).ok()).unwrap_or_default(),
            fields,
            code: e.kind().into(),
            status: 0,
            retry_after_s: None,
            data: Value::Null,
        }
    }
}

type R<T> = Result<T, Fail>;

fn conn(hub: &Hub, app: &AppHandle, server: &str) -> R<Arc<crate::proto::Conn>> {
    hub.conn(server, &lang(app)).map_err(|m| Fail::new("invalid_input", "unknown_server", m))
}

// ---- adding a server ----------------------------------------------------------------------------------------------

/// What the page shows before a server is saved.
#[derive(Debug, Serialize)]
pub struct Preview {
    pub name: String,
    pub description: Value,
    pub api: String,
    pub api_host: String,
    /// The host the player typed (or the card's discovery host), when the API is elsewhere: shown to the player.
    pub typed_host: Option<String>,
    pub relay: Option<String>,
    pub operator_name: Option<String>,
    pub operator_contact: Option<String>,
    pub region: Option<String>,
    pub languages: Vec<String>,
    pub rules_url: Option<String>,
    pub privacy_url: Option<String>,
    pub rules_summary: Value,
    /// Declared areas (from `GET /v1/meta`, authoritative).
    pub features: Vec<String>,
    /// The discovery document's `features` differ from the meta's (the meta wins).
    pub features_mismatch: bool,
    pub compat: meta::Compat,
    /// Plain http (a test server on this computer).
    pub insecure_loopback: bool,
    pub providers: Vec<Value>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Discovered {
    /// A community service answered: its preview.
    Service { preview: Preview },
    /// No usable service at that address: the line can be added as a relay only (match codes, no accounts).
    RelayOnly { line: String, relay: Option<String>, reason: String, problems: Vec<String> },
    /// A card of an older format: the page uses the older import.
    Legacy,
}

async fn preview_of(api: &str, doc: Option<&Discovery>, typed: Option<String>, card_name: Option<String>, card_relay: Option<RelayAddr>) -> R<Preview> {
    let c = crate::proto::client::Client::new("preview", api, std::env::temp_dir().join("kalamar-preview-unused"), "fr").map_err(|m| Fail::new("internal", "internal", m))?;
    let (m, check): (Value, MetaCheck) = c.fetch_meta().await?;
    let api_host = purl::api_host(api);
    let s = |p: &str| m.pointer(p).and_then(Value::as_str).map(str::to_string);
    Ok(Preview {
        name: doc.map(|d| d.name.clone()).or(card_name).unwrap_or_else(|| check.name.clone()),
        description: doc.map(|d| d.description.clone()).unwrap_or_else(|| discovery::localised(m.get("description"))),
        api: api.to_string(),
        typed_host: typed.filter(|t| t.split(':').next().unwrap_or("") != api_host),
        api_host,
        relay: doc.and_then(|d| d.relay.as_ref().map(RelayAddr::text)).or(card_relay.map(|r| r.text())).or(check.relay.clone()),
        operator_name: doc.map(|d| d.operator_name.clone()).or_else(|| s("/operator/name")),
        operator_contact: doc.map(|d| d.operator_contact.clone()).or_else(|| s("/operator/contact")),
        region: doc.and_then(|d| d.region.clone()),
        languages: doc.map(|d| d.languages.clone()).unwrap_or_default(),
        rules_url: doc.and_then(|d| d.rules_url.clone()).or_else(|| s("/rules/url")),
        privacy_url: doc.map(|d| d.privacy_url.clone()).or_else(|| s("/privacy_url")),
        rules_summary: discovery::localised(m.pointer("/rules/summary")),
        features_mismatch: doc.is_some_and(|d| {
            let mut a: Vec<&str> = d.features.iter().map(String::as_str).filter(|f| meta::AREAS.contains(f)).collect();
            a.sort_unstable();
            let mut b: Vec<&str> = check.features.iter().map(String::as_str).collect();
            b.sort_unstable();
            a != b
        }),
        features: check.features.clone(),
        compat: check.compat.clone(),
        insecure_loopback: api.starts_with("http://"),
        providers: m.pointer("/auth/providers").and_then(Value::as_array).cloned().unwrap_or_default(),
    })
}

/// "Add a server" from one typed line: the discovery document, then `GET /v1/meta`. The line's port is the HTTPS port of
/// the discovery document (1.1.1); the relay and its port come from that document. When no service answers, the page
/// offers to add the line as a relay only, and only on the player's confirmation is its port read as the relay's UDP
/// port (the older relay-only servers, already saved, keep working as before).
#[tauri::command]
pub async fn proto_discover(hub: State<'_, Hub>, line: String) -> R<Discovered> {
    // A line that looks like a relay (a saved relay, or the relays' usual UDP port): the HTTPS attempt is still made on
    // that port, only the wait before the relay-only offer is shorter.
    let known = purl::parse_host_line(&line).ok().is_some_and(|l| {
        l.port == Some(crate::servers::DEFAULT_RELAY_PORT)
            || crate::online::load(&hub.data_dir).servers.iter().any(|s| s.relay.as_deref() == Some(l.host.text().as_str()) && l.port.unwrap_or(crate::servers::DEFAULT_RELAY_PORT) == s.relay_port.unwrap_or(crate::servers::DEFAULT_RELAY_PORT))
    });
    discover_with(&line, known).await
}

pub async fn discover(line: &str) -> R<Discovered> {
    discover_with(line, false).await
}

/// `quick`: a line known as a relay is given 3 s (instead of 10) before the relay-only offer. The https and (loopback)
/// http attempts run at the same time; https wins when both answer.
pub async fn discover_with(line: &str, quick: bool) -> R<Discovered> {
    let typed = purl::parse_host_line(line).map_err(|e| Fail::new("validation", e.id(), e.message()))?;
    let timeout = Duration::from_secs(if quick { 3 } else { 10 });
    let client = http::build_client(timeout).map_err(|m| Fail::new("internal", "internal", m))?;
    let mut problems = Vec::new();
    let mut reason = String::new();
    let urls = typed.discovery_urls();
    let answers = futures_util::future::join_all(urls.iter().map(|u| http::send(&client, "GET", u, None, &Body::None, "", discovery::MAX_DOC_BYTES))).await;
    for (u, answer) in urls.iter().zip(answers) {
        match answer {
            Ok(r) => match discovery::parse_discovery(&serde_json::to_vec(&r.body).unwrap_or_default()) {
                Ok(doc) => {
                    let api = doc.api.clone();
                    let p = preview_of(&api, Some(&doc), Some(typed.text()), None, None).await;
                    return match p {
                        Ok(preview) => Ok(Discovered::Service { preview }),
                        Err(f) => Ok(Discovered::RelayOnly { line: typed.text(), relay: doc.relay.map(|r| r.text()), reason: format!("{}: {}", purl::join(&api, "/v1/meta"), f.message), problems: vec![] }),
                    };
                }
                Err(p) => {
                    problems = p;
                    reason = format!("{u}: not a valid server document");
                    break;
                }
            },
            Err(e) => {
                if reason.is_empty() || u.starts_with("https://") {
                    reason = format!("{u}: {}", e.message());
                }
            }
        }
    }
    Ok(Discovered::RelayOnly { line: typed.text(), relay: None, reason, problems })
}

/// A server card file: this protocol's format (`kalamar_server`), checked like a typed host when it has an `api`.
#[tauri::command]
pub async fn proto_card(path: String) -> R<Discovered> {
    let bytes = std::fs::read(&path).map_err(|e| Fail::from(AppError::io(std::path::Path::new(&path), &e)))?;
    let card = match discovery::parse_card(&bytes) {
        Ok(None) => return Ok(Discovered::Legacy),
        Ok(Some(c)) => c,
        Err(p) => return Err(Fail { data: json!(p), ..Fail::new("validation", "bad_card", p.join("; ")) }),
    };
    match &card.api {
        Some(api) => Ok(Discovered::Service { preview: preview_of(api, None, card.discovery.clone(), Some(card.name.clone()), card.relay.clone()).await? }),
        None => Ok(Discovered::RelayOnly { line: card.relay.as_ref().map(RelayAddr::text).unwrap_or_default(), relay: card.relay.as_ref().map(RelayAddr::text), reason: "relay-only card".into(), problems: vec![] }),
    }
}

#[derive(Debug, Deserialize)]
pub struct AddInput {
    pub name: String,
    pub api: String,
    #[serde(default)]
    pub relay: Option<String>,
}

/// Saves a server with a service after checking its meta again (compatible, or refused). Selects it. Returns its id.
#[tauri::command]
pub async fn proto_add(hub: State<'_, Hub>, input: AddInput) -> R<String> {
    add(&hub.data_dir, input).await
}

pub async fn add(data_dir: &std::path::Path, input: AddInput) -> R<String> {
    let api = purl::parse_api(&input.api).map_err(|e| Fail::new("validation", e.id(), e.message()))?;
    let preview = preview_of(&api, None, None, Some(input.name.clone()), None).await?;
    if !preview.compat.ok {
        return Err(Fail::new("incompatible", preview.compat.reason.as_deref().unwrap_or("invalid"), "this server cannot be used by this launcher"));
    }
    let summary = meta::Summary::new(&preview.description, &preview.features);
    let remember = |id: &str| meta::remember_summary(&crate::online::online_dir(data_dir).join(id), &summary);
    let store = crate::online::load(data_dir);
    if let Some(s) = store.servers.iter().find(|s| s.api.as_deref().and_then(|a| purl::parse_api(a).ok()).as_deref() == Some(api.as_str())) {
        crate::online::select(data_dir, Some(&s.id))?;
        remember(&s.id);
        return Ok(s.id.clone());
    }
    // The relay comes from the service (its discovery document, read by the caller, else its meta), never from a typed
    // line (1.1.1: the port of a line is the HTTPS port).
    let relay_text = input.relay.as_deref().map(str::trim).filter(|r| !r.is_empty()).map(str::to_string).or(preview.relay.clone());
    let relay = match relay_text {
        Some(r) => Some(discovery::parse_relay(&Value::String(r)).map_err(|m| Fail::new("validation", "bad_relay", m))?),
        None => None,
    }
    .flatten();
    let name: String = input.name.chars().filter(|c| !crate::util::unsafe_char(*c)).collect::<String>().trim().chars().take(40).collect();
    let si = crate::online::ServerInput {
        name: if name.is_empty() { purl::api_host(&api) } else { name },
        relay: relay.as_ref().map(|r| r.host.clone()).unwrap_or_default(),
        relay_port: relay.as_ref().and_then(|r| r.port),
        api: api.clone(),
        ..Default::default()
    };
    let before: Vec<String> = store.servers.iter().map(|s| s.id.clone()).collect();
    let after = crate::online::upsert(data_dir, &si)?;
    let id = after.servers.iter().find(|s| !before.contains(&s.id)).map(|s| s.id.clone()).ok_or_else(|| Fail::new("internal", "internal", "not saved"))?;
    crate::online::select(data_dir, Some(&id))?;
    remember(&id);
    Ok(id)
}

// ---- one server ---------------------------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Overview {
    pub server: Value,
    /// The meta document as the server sent it (the page renders its declarations), `null` when it could not be fetched.
    pub meta: Option<Value>,
    pub check: Option<MetaCheck>,
    pub meta_error: Option<Fail>,
    pub session: SessionInfo,
    pub stream: Option<StreamInfo>,
    pub runner: Option<RunnerStatus>,
    /// Server clock minus local clock, seconds (for countdowns).
    pub offset_s: i64,
    /// 1.1.9: the relays of `meta.relays` that are measured (one per address, at most 16) and where their measurement
    /// stands; empty with fewer than two relays (never measured). Then pushed as `proto-relays` when it changes.
    pub relays: Vec<crate::proto::relayping::RelayView>,
}

async fn overview(hub: &Hub, app: &AppHandle, server: &str, refresh: bool) -> R<Overview> {
    let c = conn(hub, app, server)?;
    let store = crate::online::load(&hub.data_dir);
    let s = store.servers.iter().find(|s| s.id == server).ok_or_else(|| Fail::new("invalid_input", "unknown_server", "unknown server"))?;
    let mut meta_error = None;
    if (refresh || c.client.cached_meta().is_none())
        && let Err(e) = c.client.fetch_meta().await
    {
        meta_error = Some(Fail::from(e));
    }
    let cached = c.client.cached_meta();
    if let Some((m, k)) = &cached {
        meta::remember_summary(&hub.server_dir(server), &meta::Summary::of_meta(m, k));
    }
    Ok(Overview {
        server: json!({"id": s.id, "name": s.name, "api": c.client.api, "relay": s.relay.as_ref().map(|r| s.relay_port.map(|p| format!("{r}:{p}")).unwrap_or_else(|| r.clone()))}),
        meta: cached.as_ref().map(|(m, _)| m.clone()),
        check: cached.map(|(_, k)| k),
        meta_error,
        session: c.client.session_info(),
        stream: c.stream_info(),
        runner: c.runner.last(),
        offset_s: c.client.offset_s(),
        relays: c.relays.view(),
    })
}

/// Everything the community page needs to draw a server; fetches the meta when asked or not known yet.
#[tauri::command]
pub async fn proto_overview(app: AppHandle, hub: State<'_, Hub>, server: String, refresh: Option<bool>) -> R<Overview> {
    overview(&hub, &app, &server, refresh.unwrap_or(false)).await
}

/// What the Play page shows of a saved server with a service, from this computer only.
#[derive(Debug, Serialize)]
pub struct Peek {
    pub id: String,
    pub session: SessionInfo,
    /// The last description and areas the server declared (empty until its meta was read once).
    pub description: Value,
    pub features: Vec<String>,
}

pub fn peek(hub: &Hub) -> Vec<Peek> {
    let store = crate::online::load(&hub.data_dir);
    store
        .servers
        .iter()
        .filter(|s| s.api.is_some())
        .map(|s| {
            let dir = hub.server_dir(&s.id);
            let live = hub.existing(&s.id);
            let session = match &live {
                Some(c) => c.client.session_info(),
                None => match crate::proto::tokens::load(&dir) {
                    Some(t) => SessionInfo { logged_in: true, provider: Some(t.provider), user_id: Some(t.user_id), display_name: Some(t.display_name) },
                    None => SessionInfo { logged_in: false, provider: None, user_id: None, display_name: None },
                },
            };
            let summary = live.and_then(|c| c.client.cached_meta()).map(|(m, k)| meta::Summary::of_meta(&m, &k)).or_else(|| meta::load_summary(&dir)).unwrap_or_default();
            Peek { id: s.id.clone(), session, description: summary.description, features: summary.features }
        })
        .collect()
}

/// The saved servers as the Play page shows them, without contacting any of them: whether a login is kept (and its
/// name), and what each one declared the last time its meta was read.
#[tauri::command]
pub async fn proto_peek(hub: State<'_, Hub>) -> R<Vec<Peek>> {
    Ok(peek(&hub))
}

/// How many players a server has (`GET /v1/population`, no login sent), when it declares the area (its meta, else the
/// summary kept on this computer); `None` when it does not, or on any failure. The page asks while the Play or Community
/// page is on screen, every 30 s at most.
#[tauri::command]
pub async fn proto_population(app: AppHandle, hub: State<'_, Hub>, server: String) -> R<Option<crate::proto::population::Population>> {
    let store = crate::online::load(&hub.data_dir);
    let Some(api) = store.servers.iter().find(|s| s.id == server).and_then(|s| s.api.as_deref()).and_then(|a| purl::parse_api(a).ok()) else {
        return Ok(None);
    };
    let declared = hub
        .existing(&server)
        .and_then(|c| c.client.cached_meta())
        .map(|(_, k)| k.features)
        .or_else(|| meta::load_summary(&hub.server_dir(&server)).map(|s| s.features))
        .unwrap_or_default();
    if !declared.iter().any(|f| f == crate::proto::population::AREA) {
        return Ok(None);
    }
    Ok(crate::proto::population::fetch(&api, &lang(&app)).await)
}

/// Starts the event stream when logged in (the page calls it when it opens). Also tells the server what this game build
/// can run.
#[tauri::command]
pub async fn proto_connect(app: AppHandle, hub: State<'_, Hub>, server: String) -> R<Overview> {
    let c = conn(&hub, &app, &server)?;
    if c.client.cached_meta().is_none() {
        let _ = c.client.fetch_meta().await;
    }
    if c.client.session_info().logged_in {
        hub.ensure_stream(&server, &c);
    }
    overview(&hub, &app, &server, false).await
}

fn check_display_name(c: &crate::proto::client::Client, v: &Value) -> R<()> {
    if let Some(n) = v.get("display_name") {
        let n = n.as_str().unwrap_or("");
        let max = c.cached_meta().map(|(_, k)| k.display_name_max).unwrap_or(meta::DISPLAY_NAME_MAX);
        let min = c.cached_meta().map(|(_, k)| k.display_name_min).unwrap_or(1);
        let len = n.trim().chars().count();
        if len < min || len > max || n.chars().any(crate::util::unsafe_char) {
            let mut f = Fail::new("validation", "display_name", format!("display name: {min} to {max} characters"));
            f.fields = vec![crate::error::FieldError { key: "/display_name".into(), message: "length".into() }];
            return Err(f);
        }
    }
    Ok(())
}

/// What a login is followed by: the game's capabilities (`PUT /v1/me/game`, with the relays' last values), the event
/// stream, and the relays' measurement in the background (it declares again when it ends; the login answers first).
async fn after_login(hub: &Hub, server: &str, c: &Arc<crate::proto::Conn>) {
    let _ = c.relays.declare().await;
    hub.ensure_stream(server, c);
    c.relays.kick();
}

/// Logs in with a direct provider (a form of the server's fields). The display name is at most 10 characters.
#[tauri::command]
pub async fn proto_login(app: AppHandle, hub: State<'_, Hub>, server: String, provider: String, fields: Value) -> R<Overview> {
    let c = conn(&hub, &app, &server)?;
    if c.client.cached_meta().is_none() {
        c.client.fetch_meta().await?;
    }
    let (m, _) = c.client.cached_meta().ok_or_else(|| Fail::new("network", "network", "no meta"))?;
    let p = meta::provider(&m, &provider).ok_or_else(|| Fail::new("invalid_input", "unknown_provider", "this server does not offer that login"))?;
    if p.get("kind").and_then(Value::as_str) != Some("direct") || !fields.is_object() {
        return Err(Fail::new("invalid_input", "unknown_provider", "not a direct login"));
    }
    check_display_name(&c.client, &fields)?;
    c.client.login_direct(&provider, fields).await?;
    after_login(&hub, &server, &c).await;
    overview(&hub, &app, &server, false).await
}

/// Logs in with an `oauth_loopback` provider: the system browser opens the provider's page; the launcher waits for the
/// redirect (5 minutes at most).
#[tauri::command]
pub async fn proto_oauth(app: AppHandle, hub: State<'_, Hub>, server: String, provider: String) -> R<Overview> {
    let c = conn(&hub, &app, &server)?;
    if c.client.cached_meta().is_none() {
        c.client.fetch_meta().await?;
    }
    let (m, _) = c.client.cached_meta().ok_or_else(|| Fail::new("network", "network", "no meta"))?;
    let p = meta::provider(&m, &provider).cloned().ok_or_else(|| Fail::new("invalid_input", "unknown_provider", "this server does not offer that login"))?;
    oauth::login(&c.client, &p, &oauth::open_browser, Duration::from_secs(300)).await?;
    after_login(&hub, &server, &c).await;
    overview(&hub, &app, &server, false).await
}

/// Logs out (this session, or every session of the account): the stream stops and the stored tokens are deleted.
#[tauri::command]
pub async fn proto_logout(app: AppHandle, hub: State<'_, Hub>, server: String, all: Option<bool>) -> R<Overview> {
    let c = conn(&hub, &app, &server)?;
    c.stop_stream();
    c.client.logout(all.unwrap_or(false)).await;
    overview(&hub, &app, &server, false).await
}

/// Saves everything the server holds about the player (`GET /v1/me/export`) in a file the player chooses in the
/// system's "save as" dialog. `None`: the player closed the dialog. The answer is the file's path, to show it.
#[tauri::command]
pub async fn proto_export(app: AppHandle, hub: State<'_, Hub>, server: String) -> R<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let c = conn(&hub, &app, &server)?;
    let bytes = crate::proto::account::export_document(&c.client).await?;
    let name = crate::proto::account::export_file_name(&c.client.api, chrono::Utc::now());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app.dialog().file().set_file_name(&name).add_filter("JSON", &["json"]);
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    dialog.save_file(move |chosen| {
        let _ = tx.send(chosen);
    });
    let Some(path) = rx.await.ok().flatten().and_then(|p| p.into_path().ok()) else { return Ok(None) };
    crate::proto::account::write_export(&path, &bytes).map_err(|m| Fail::new("io", "export_write", m))?;
    Ok(Some(path.display().to_string()))
}

/// Erases the player's account on the server (`DELETE /v1/me?confirm=erase`), after checking that the name typed in
/// the confirmation is the account's. Then, as after a logout: the stream stops and the stored tokens are deleted.
#[tauri::command]
pub async fn proto_erase(app: AppHandle, hub: State<'_, Hub>, server: String, typed_name: String) -> R<Overview> {
    let c = conn(&hub, &app, &server)?;
    let known = c.client.session().map(|s| s.display_name).unwrap_or_default();
    if !crate::proto::account::name_matches(&typed_name, &known) {
        return Err(Fail::new("invalid_input", "erase_name", "the name typed is not the account's display name"));
    }
    erase_on(&hub, &server, &c).await?;
    overview(&hub, &app, &server, false).await
}

/// The erasure on a connection. The event stream stops first: the server ends the account's sessions at once, and a
/// stream still open would report a lost session ("you were logged out") for what the player just asked. After a
/// refusal the stream starts again and nothing has changed.
pub async fn erase_on(hub: &Hub, server: &str, c: &Arc<crate::proto::Conn>) -> Result<(), CallError> {
    c.stop_stream();
    let r = crate::proto::account::erase(&c.client).await;
    if r.is_err() {
        hub.ensure_stream(server, c);
    }
    r
}

/// A path the page may ask for: under `/v1/`, short, no control character, space, `#` or backslash, and no `.` or `..`
/// segment, plain or percent-encoded (the HTTP client would resolve them into another endpoint than the one checked).
pub fn request_path_ok(path: &str) -> bool {
    path.starts_with("/v1/")
        && path.len() <= 512
        && !path.chars().any(|c| c.is_control() || c == ' ' || c == '#' || c == '\\')
        && !path.split('?').next().unwrap_or("").split('/').any(|seg| {
            let s = seg.to_ascii_lowercase().replace("%2e", ".");
            s == "." || s == ".." || s.contains("%2f") || s.contains("%5c")
        })
}

/// The export and the erasure of the account are not reachable through [`proto_request`]: they have commands of their
/// own (a file chosen by the player in the system's dialog, a typed confirmation checked here).
pub fn has_own_command(method: &str, path: &str) -> bool {
    let bare = path.split('?').next().unwrap_or("").trim_end_matches('/');
    bare.eq_ignore_ascii_case("/v1/me/export") || (method.eq_ignore_ascii_case("DELETE") && bare.eq_ignore_ascii_case("/v1/me"))
}

/// The player's answer to the launcher's question before a launch (runner phase `confirm`): `start` the game or not.
#[tauri::command]
pub async fn proto_confirm_launch(app: AppHandle, hub: State<'_, Hub>, server: String, match_id: String, start: bool) -> R<()> {
    if match_id.is_empty() || match_id.len() > 128 {
        return Err(Fail::new("invalid_input", "bad_request", "unknown match"));
    }
    conn(&hub, &app, &server)?.runner.answer(&match_id, start);
    Ok(())
}

/// A request to a declared area (or the core). Logins, tokens, the event stream and the match reports are not
/// reachable this way; an undeclared area is never called.
#[tauri::command]
pub async fn proto_request(app: AppHandle, hub: State<'_, Hub>, server: String, method: String, path: String, body: Option<Value>) -> R<Value> {
    let method = method.to_ascii_uppercase();
    if !["GET", "POST", "PUT", "PATCH", "DELETE"].contains(&method.as_str()) || !request_path_ok(&path) {
        return Err(Fail::new("invalid_input", "bad_request", "not a request of this protocol"));
    }
    if has_own_command(&method, &path) {
        return Err(Fail::new("invalid_input", "bad_request", "this request is not available"));
    }
    let area = meta::area_of(&method, &path).ok_or_else(|| Fail::new("invalid_input", "bad_request", "this request is not available"))?;
    let c = conn(&hub, &app, &server)?;
    if c.client.cached_meta().is_none() {
        c.client.fetch_meta().await?;
    }
    if area != "core" && !area.split('|').any(|a| c.client.features().iter().any(|f| f == a)) {
        return Err(Fail::new("feature_disabled", "feature_disabled", format!("the server does not offer {area}")));
    }
    if method == "PATCH" && path == "/v1/me"
        && let Some(b) = &body
    {
        check_display_name(&c.client, b)?;
    }
    let b = match body {
        Some(v) => Body::Json(v),
        None if method == "POST" || method == "PUT" => Body::Json(json!({})),
        None => Body::None,
    };
    let r = c.client.call(&method, &path, b, true).await?;
    if path == "/v1/me" || path == "/v1/state" {
        let me = if path == "/v1/me" { &r.body } else { r.body.get("me").unwrap_or(&Value::Null) };
        c.client.remember_me(me);
    }
    if path == "/v1/state"
        && let Some(m) = r.body.get("match").filter(|m| m.is_object())
    {
        c.runner.on_match(m);
    }
    if path.starts_with("/v1/rooms/") && path.ends_with("/start") {
        c.runner.on_match(&r.body);
    }
    Ok(r.body)
}

/// Stops the event stream (the session is kept): the player chose another server.
#[tauri::command]
pub async fn proto_disconnect(app: AppHandle, hub: State<'_, Hub>, server: String) -> R<()> {
    if let Ok(c) = conn(&hub, &app, &server) {
        c.stop_stream();
    }
    Ok(())
}

/// Opens a server's link (a `link` component, the rules or privacy page) in the system browser, on the player's click
/// only. https only.
#[tauri::command]
pub fn proto_open_link(url: String) -> R<()> {
    let u = reqwest::Url::parse(&url).map_err(|e| Fail::new("validation", "bad_link", e.to_string()))?;
    if u.scheme() != "https" || u.host_str().is_none() || !u.username().is_empty() || url.len() > 2000 {
        return Err(Fail::new("validation", "bad_link", "only https links are opened"));
    }
    oauth::open_browser(u.as_str()).map_err(|m| Fail::new("internal", "internal", m))
}

/// "Stay connected in the background" for a server (off by default): the event stream then starts with the launcher.
#[tauri::command]
pub fn proto_set_background(hub: State<'_, Hub>, server: String, on: bool) -> R<()> {
    let mut store = crate::online::load(&hub.data_dir);
    let s = store.servers.iter_mut().find(|s| s.id == server).ok_or_else(|| Fail::new("invalid_input", "unknown_server", "unknown server"))?;
    s.background = on;
    crate::online::save(&hub.data_dir, &store)?;
    Ok(())
}

/// What the installed engine declared (its capability list read, the contract versions it runs), for the page.
#[tauri::command]
pub async fn proto_engine(hub: State<'_, Hub>) -> R<Value> {
    let host = hub.host();
    let info = tokio::task::spawn_blocking(move || host.engine()).await.ok().flatten();
    let caps = crate::proto::engine::caps(info.as_ref());
    let names = crate::proto::engine::names(info.as_ref());
    Ok(json!({"listed": caps.listed, "note": info.and_then(|i| i.note), "caps": caps, "names": names}))
}

/// Is the installed game recent enough for the automatic match, and is it the runtime this launcher carries
/// (`freshness.rs`)? Reads the engine's cached listing (run once per executable), the install state and the embedded manifest.
#[tauri::command]
pub async fn proto_game_check(app: AppHandle, hub: State<'_, Hub>) -> R<Value> {
    let host = hub.host();
    let info = tokio::task::spawn_blocking(move || host.engine()).await.ok().flatten();
    let root = game_root(&app);
    let has_exe = root.as_ref().is_some_and(|r| crate::install::exe_path(r).is_file());
    let (installed, bundled) = runtime_commits(&app);
    let old = root.as_deref().filter(|r| crate::installer::gamever::is_outdated_root(r)).and_then(crate::installer::gamever::folder_version);
    Ok(json!(crate::proto::freshness::assess(info.as_ref(), has_exe, installed.as_deref(), bundled.as_deref(), old.as_deref())))
}

fn game_root(app: &AppHandle) -> Option<std::path::PathBuf> {
    let st = app.state::<crate::commands::AppState>();
    let c = st.config.lock().unwrap_or_else(|p| p.into_inner());
    c.root.clone().map(std::path::PathBuf::from).or_else(crate::config::default_root)
}

/// The runtime commit the installed game was compiled from (the install state's record), and the one embedded in this
/// launcher.
fn runtime_commits(app: &AppHandle) -> (Option<String>, Option<String>) {
    let st = app.state::<crate::commands::AppState>();
    let install_dir = st.config.lock().unwrap_or_else(|p| p.into_inner()).install.dir.clone().map(std::path::PathBuf::from);
    let root = game_root(app);
    // The install folder: the wizard's choice, else two levels above the game folder (`<install>\work\<game>`).
    let state_files = install_dir.into_iter().chain(root.as_ref().and_then(|r| r.parent()).and_then(|p| p.parent()).map(std::path::Path::to_path_buf)).map(|d| crate::installer::Layout::new(d).state());
    let installed = state_files.into_iter().find_map(|f| crate::proto::freshness::installed_commit(&f));
    let bundled = crate::commands::bundle_dir(app).ok().as_deref().and_then(crate::installer::bundle::load_manifest).map(|m| m.commit);
    (installed, bundled)
}

/// Field types the page's generic form renders (`src/community/ui.tsx`, `KNOWN_TYPES`): kept equal for the tests.
pub const KNOWN_FIELD_TYPES: &[&str] = &["string", "integer", "boolean", "enum", "teams", "mode", "stage"];

/// The player's picks for a match's loadout choice (runner phase `choosing`): slot -> id.
#[tauri::command]
pub async fn proto_pick_loadout(app: AppHandle, hub: State<'_, Hub>, server: String, match_id: String, picks: std::collections::HashMap<String, String>) -> R<()> {
    let picks = picks.into_iter().filter(|(k, v)| crate::proto::contract::SLOTS.contains(&k.as_str()) && (1..=64).contains(&v.len())).collect();
    conn(&hub, &app, &server)?.runner.pick_loadout(&match_id, picks);
    Ok(())
}

/// The match runner's last status (after a page reload).
#[tauri::command]
pub async fn proto_runner(app: AppHandle, hub: State<'_, Hub>, server: String) -> R<Option<RunnerStatus>> {
    Ok(conn(&hub, &app, &server)?.runner.last())
}

/// Removes a server with a service: logs out first (best effort), then deletes its folder (tokens included).
#[tauri::command]
pub async fn proto_forget(app: AppHandle, hub: State<'_, Hub>, server: String) -> R<()> {
    if let Ok(c) = conn(&hub, &app, &server) {
        c.stop_stream();
        c.client.logout(false).await;
    }
    hub.drop_conn(&server);
    crate::online::remove(&hub.data_dir, &server)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_cannot_export_or_erase_the_account_through_a_plain_request() {
        assert!(has_own_command("GET", "/v1/me/export"));
        assert!(has_own_command("GET", "/v1/me/export?x=1"));
        assert!(has_own_command("DELETE", "/v1/me?confirm=erase"));
        assert!(has_own_command("DELETE", "/v1/me/"));
        assert!(!has_own_command("GET", "/v1/me"));
        assert!(!has_own_command("PATCH", "/v1/me"));
        assert!(!has_own_command("DELETE", "/v1/friends/u_1"));
    }

    #[test]
    fn the_launcher_comes_forward_once_per_match_and_never_over_a_running_game() {
        let found = |id: &str| json!({"type": "match.found", "data": {"id": id}});
        assert_eq!(FrontOnce::wants_front("proto-event", &found("m_1")).as_deref(), Some("m_1"));
        assert_eq!(FrontOnce::wants_front("proto-event", &json!({"type": "match.updated", "data": {"id": "m_1"}})), None);
        assert_eq!(FrontOnce::wants_front("proto-runner", &json!({"match_id": "m_2", "phase": "confirm"})).as_deref(), Some("m_2"));
        assert_eq!(FrontOnce::wants_front("proto-runner", &json!({"match_id": "m_2", "phase": "running"})), None);
        let mut f = FrontOnce::default();
        let t0 = std::time::Instant::now();
        assert!(!f.take("m_1", true, t0), "never while a game runs");
        assert!(f.take("m_1", false, t0));
        let later = t0 + FRONT_EVERY + Duration::from_secs(1);
        assert!(!f.take("m_1", false, later), "a match pushed again does not take the focus again");
        assert!(!f.take("m_2", false, t0 + Duration::from_secs(2)), "nor does a new match every few seconds");
        assert!(f.take("m_2", false, later));
    }

    #[test]
    fn request_paths_cannot_climb_to_another_endpoint() {
        for ok in ["/v1/rooms/r_1/ready", "/v1/queues/duel/tickets", "/v1/users/u_1/matches?limit=20"] {
            assert!(request_path_ok(ok), "{ok}");
        }
        for bad in ["/v1/rooms/../auth/refresh", "/v1/rooms/%2e%2e/auth/refresh", "/v1/rooms/%2E./x", "/v1/./me", "/v1/rooms/a%2fb", "/v1/a\\b", "/v2/x", "/v1/a b", "/v1/a#b"] {
            assert!(!request_path_ok(bad), "{bad}");
        }
    }
}