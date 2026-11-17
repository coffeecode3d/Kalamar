//! Kalamar, the launcher (unofficial fan project). With no community server added, the backend makes no network access
//! of its own, but the update check of a build that names an update repository (`update`, GitHub's release API, at most
//! once a day, can be turned off); with a server, it talks to that server only (`proto`, from the community page on).
//! The install wizard starts the build tools, which download from the sources listed on its confirmation screen.
//! The Discord presence (`discord`, opt-in, in a build that names a Discord application) talks only to the Discord
//! application of this PC through its local pipe: no network access of its own.

mod commands;
mod config;
pub mod datadir;
pub mod discord;
mod error;
mod gamelang;
pub mod game;
mod ini;
pub mod install;
pub mod installer;
mod automatch;
mod lan;
mod matchcode;
mod migrate;
mod launch;
mod cli;
mod shell;
mod winutil;
mod online;
pub mod loadouts;
pub mod profile;
pub mod proto;
mod relay;
mod servercard;
mod servers;
pub mod settings;
mod startmode;
mod trace;
mod tray;
pub mod update;
mod util;

use tauri::Manager;

/// Launcher data directory, decided once by [`datadir::current`]: `KALAMAR_DATA` if set (portable use, tests), else
/// `%APPDATA%\<identifier>`, where an earlier launcher's folder was moved at the first start.
fn data_dir() -> std::path::PathBuf {
    datadir::current().dir.clone()
}

/// Serves `gameicon://<key>` requests: PNG icons decoded from the player's own RomFS (cached on disk). Only keys that
/// resolve to a file of the loaded RomFS index are answered; everything else is a 404.
fn icon_response(app: &tauri::AppHandle, uri_path: &str) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::Response;
    let key = game::icons::percent_decode(uri_path.trim_start_matches('/'));
    let state = app.state::<commands::AppState>();
    let result = state.gamedata.current().ok_or_else(|| "no game data loaded".to_string()).and_then(|l| l.store.png(&l.rom, &key));
    match result {
        Ok(png) => Response::builder()
            .status(200)
            .header("Content-Type", "image/png")
            .header("Cache-Control", "private, max-age=86400")
            .body(png)
            .unwrap_or_default(),
        Err(e) => Response::builder().status(404).header("Content-Type", "text/plain").body(e.into_bytes()).unwrap_or_default(),
    }
}

/// Wraps the command handler so each command's arrival is logged when `KALAMAR_TRACE` is set.
fn traced<R: tauri::Runtime>(inner: impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke| {
        if trace::enabled() {
            trace::mark(&format!("cmd {}", invoke.message.command()));
        }
        inner(invoke)
    }
}

pub fn run() {
    // DLLs from System32 only, before anything loads one (`winutil::harden`).
    winutil::harden();
    trace::init();
    // The data folder first (an earlier launcher's moved once): the update check below reads its journal there.
    let _ = datadir::current();
    // An update interrupted or on trial is finished or undone before anything else (no window, no network).
    if !update::early_start() {
        trace::mark("update:rolled back, previous version started");
        return;
    }
    // Four workers decode icons so a grid asking for dozens at once never blocks the window thread.
    type Job = (tauri::AppHandle, String, tauri::UriSchemeResponder);
    let (tx, rx) = std::sync::mpsc::channel::<Job>();
    let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
    for _ in 0..4 {
        let rx = rx.clone();
        std::thread::spawn(move || {
            loop {
                let job = rx.lock().unwrap_or_else(|p| p.into_inner()).recv();
                let Ok((app, path, responder)) = job else { break };
                responder.respond(icon_response(&app, &path));
            }
        });
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("gameicon", move |ctx, request, responder| {
            let _ = tx.send((ctx.app_handle().clone(), request.uri().path().to_string(), responder));
        })
        .setup(|app| {
            trace::mark("rust:setup begin");
            shell::set_cli(cli::parse(std::env::args().skip(1)));
            let dir = data_dir();
            let _ = std::fs::create_dir_all(&dir);
            // Older formats of this folder's files upgraded in place, once (an earlier launcher's folder was moved here
            // already, `datadir`).
            let _ = trace::timed("migrate::run_startup", || migrate::run_startup(&dir, &[]));
            let cfg = trace::timed("config::load", || config::load(&dir));
            // A default match server kept by an earlier launcher becomes a server entry (once; the setting stays).
            let _ = trace::timed("online::migrate_match_server", || online::migrate_match_server(&dir, &cfg.match_server));
            let dir2 = dir.clone();
            // Community protocol client: an empty hub (no file read, no network until a `proto_*` command runs).
            app.manage(proto::glue::hub(app.handle().clone(), dir.clone()));
            // The launcher's own updates: a few small files read, no network (the check waits 20 s, when enabled).
            app.manage(update::updater(&dir, &cfg.update));
            // The Discord presence: nothing runs unless this build names an application and the player switched it on.
            let handle = app.handle().clone();
            let presence = discord::Presence::new(discord::available().then(|| discord::worker::Options::discord(discord::CLIENT_ID)), move |s| {
                let _ = tauri::Emitter::emit(&handle, "discord-status", s);
            });
            if cfg.discord.enabled {
                presence.set_enabled(true);
            }
            app.manage(presence);
            app.manage(commands::AppState::new(dir, cfg));
            // The window comes last: the page's first commands find the state in place. It opens where it was left.
            trace::mark("rust:setup state ready");
            shell::create_main_window(app, &dir2)?;
            update::start(app.handle());
            trace::mark("rust:setup end");
            Ok(())
        })
        .on_page_load(|_, payload| {
            trace::mark(match payload.event() {
                tauri::webview::PageLoadEvent::Started => "page:load started",
                tauri::webview::PageLoadEvent::Finished => "page:load finished",
            });
        })
        .invoke_handler(traced(tauri::generate_handler![
            commands::app_info,
            commands::get_config,
            commands::boot,
            commands::game_focus,
            commands::set_root,
            commands::set_translator,
            commands::set_language,
            commands::game_language_info,
            commands::set_game_lang_follow,
            commands::set_lan_enabled,
            commands::install_status,
            commands::settings_schema,
            commands::settings_load,
            commands::settings_save,
            commands::launch_mode,
            commands::launch_second,
            commands::play_preflight,
            commands::backups_info,
            commands::set_second_player,
            commands::set_lan_prefs,
            commands::game_status,
            commands::game2_status,
            commands::game2_stop,
            commands::last_run,
            commands::game_stop,
            commands::open_logs_folder,
            commands::read_log_tail,
            commands::lan_generate_room,
            commands::lan_preview,
            commands::relay_status,
            commands::relay_start,
            commands::relay_stop,
            commands::set_relay_exe,
            commands::launch_report,
            commands::set_launch_options,
            commands::profile_catalogue,
            commands::gamedata_stages,
            commands::gamedata_icon_cache,
            commands::gamedata_icons_warm,
            commands::gamedata_icons_cancel,
            commands::gamedata_cache_clear,
            commands::profile_load,
            commands::profile_save,
            commands::loadouts_list,
            commands::loadouts_save,
            commands::loadouts_rename,
            commands::loadouts_delete,
            commands::gear_abilities_info,
            commands::server_card_preview,
            commands::server_card_import,
            commands::set_match_server,
            commands::match_server_from_card,
            commands::match_create,
            commands::match_parse,
            commands::match_next_round,
            commands::match_start,
            commands::match_poll,
            commands::match_result,
            commands::match_last,
            commands::migration_status,
            commands::migration_last,
            commands::migration_ack,
            commands::migration_import,
            commands::set_ui_prefs,
            commands::update_set_prefs,
            update::update_status,
            update::update_confirm_start,
            update::update_check,
            update::update_download,
            update::update_cancel,
            update::update_later,
            update::update_dismiss,
            update::update_apply,
            update::update_save_to,
            update::update_show_saved,
            commands::installer_info,
            commands::installer_check_file,
            commands::installer_check_dir,
            commands::installer_plan,
            commands::installer_update_check,
            commands::installer_start,
            commands::installer_status,
            commands::installer_cancel,
            commands::installer_open_logs,
            commands::installer_uninstall,
            commands::installer_rollback,
            commands::installer_close,
            commands::installer_at_close,
            commands::notify_attention,
            commands::servers_view,
            commands::server_add_line,
            commands::server_remove,
            commands::server_select,
            commands::server_resolve,
            proto::glue::proto_discover,
            proto::glue::proto_card,
            proto::glue::proto_add,
            proto::glue::proto_overview,
            proto::glue::proto_peek,
            proto::glue::proto_population,
            proto::glue::proto_connect,
            proto::glue::proto_login,
            proto::glue::proto_oauth,
            proto::glue::proto_logout,
            proto::glue::proto_export,
            proto::glue::proto_erase,
            proto::glue::proto_request,
            proto::glue::proto_runner,
            proto::glue::proto_disconnect,
            proto::glue::proto_open_link,
            proto::glue::proto_set_background,
            proto::glue::proto_engine,
            proto::glue::proto_game_check,
            proto::glue::proto_pick_loadout,
            proto::glue::proto_weapon_ready,
            proto::glue::proto_confirm_launch,
            proto::glue::proto_forget,
            discord::discord_view,
            discord::discord_set_prefs,
            discord::discord_publish,
        ]))
        .on_window_event(|window, event| {
            shell::on_event(window, event, &window.state::<commands::AppState>().data_dir);
            match event {
                // Closing the window during an installation: the page asks whether to keep it running (minimized) or to
                // stop it (resumable); it is never cut off silently. With a game update chosen for "when the launcher
                // closes", the window goes, the icon stays in the notification area and the update runs.
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let st = window.state::<commands::AppState>();
                    if st.installer.active() {
                        api.prevent_close();
                        let _ = tauri::Emitter::emit(window, "installer-close-requested", ());
                    } else if st.at_close.lock().unwrap_or_else(|p| p.into_inner()).is_some() {
                        api.prevent_close();
                        if !commands::start_at_close(window.app_handle()) {
                            window.app_handle().exit(0);
                        }
                    }
                }
                // The window is in use again: an icon that only carried a notification goes.
                tauri::WindowEvent::Focused(true) => tray::window_used(),
                // The window is gone (an update chosen for "when the launcher closes" runs on, in the notification
                // area): for the player the launcher is closed, so is its Discord presence.
                tauri::WindowEvent::Destroyed => window.state::<discord::Presence>().publish(None),
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building the launcher")
        .run(|app, event| {
            if let tauri::RunEvent::Ready = event {
                trace::mark("rust:run-event ready");
            }
            // The relay this launcher started must not outlive it.
            if let tauri::RunEvent::Exit = event {
                let st = app.state::<commands::AppState>();
                let _ = st.relay.stop();
                // A running install stops with the launcher (its job object would kill the tools anyway); the next
                // start offers to resume it.
                st.installer.cancel();
                tray::shutdown();
                // The Discord presence is cleared and its pipe closed (Discord would clear it when this process ends
                // anyway); never more than a moment.
                app.state::<discord::Presence>().shutdown(std::time::Duration::from_millis(800));
            }
        });
}
