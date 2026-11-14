// Typed wrappers around the Rust IPC commands (src-tauri/src/commands.rs). The only place that calls invoke().
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { timedCall } from "./perf";

export interface FieldError { key: string; message: string }
export interface AppError { kind: string; message: string; details: string[]; fields: FieldError[] }

export function toAppError(e: unknown): AppError {
  if (e && typeof e === "object" && "kind" in e && "message" in e) {
    const o = e as Partial<AppError>;
    return { kind: String(o.kind), message: String(o.message), details: o.details ?? [], fields: o.fields ?? [] };
  }
  return { kind: "internal", message: typeof e === "string" ? e : String(e), details: [], fields: [] };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await timedCall(cmd, () => invoke<T>(cmd, args));
  } catch (e) {
    throw toAppError(e);
  }
}

export type Labels = Record<string, string>;

/** How the launcher found its data folder at this start (`datadir.rs`). */
export type DataFolder =
  | { kind: "override"; var: string }
  | { kind: "fresh" }
  | { kind: "current" }
  | { kind: "moved"; from: string; mode: "rename" | "split" | "copy"; kept: string[]; rewritten: string[]; skipped: string[] }
  | { kind: "deferred"; from: string; reason: "old_launcher" | "update_trial" | "in_use" | "busy" | "game_in_folder" | "failed" }
  | { kind: "both"; from: string; left: "game" | "copy" | "data" };
export interface AppInfo { name: string; version: string; data_dir: string; data_folder?: DataFolder }
export type LanMode = "lan" | "relay";
export interface LanPrefs { mode: LanMode; relay_host: string; relay_port: number; room: string; net_log: boolean; bind_any: boolean; lan_address: string }
/** `last_mode`: what the main "Play" button starts (`original` = full game, `range` = training, `hub` = free roam). */
export type QuickMode = "original" | "range" | "hub";
/** `advanced`: the advanced mode of the interface (logs, paths, reports, LAN...), off by default. */
/** `notify`: a Windows notification when an installation ends while the launcher is in the background (on by default). */
export interface UiPrefs { full_untouched: boolean; adv_open: boolean; match_players: number; match_mode: string; last_mode: QuickMode; minimize_on_play: boolean; advanced: boolean; sounds: boolean; sound_volume: number; notify: boolean }
export interface Migration { kind: string; source: string; date: number; files: number; imported: string[]; rejected: string[]; skipped: string[]; shown: boolean }
export interface ConfigView {
  root: string | null; translator: string | null; language: string; lan_enabled: boolean; lan: LanPrefs;
  apply_profile: boolean; skip_first_launch: boolean; unlock_all: boolean; relay_exe: string | null; relay_port: number;
  second_name: string; second_dir: string; match_server: string; ui: UiPrefs;
  /** What the game's language follows while the settings file has no `lang`: the launcher's language or Windows'. */
  game_lang_follow: "launcher" | "windows";
  /** The launcher's own updates (only used when the build names an update repository). */
  update: UpdatePrefs;
  /** The Discord presence (only used when the build names a Discord application); off by default. */
  discord?: DiscordPrefs;
  effective_root: string | null; effective_translator: string | null;
  /** The build names a Discord application: Settings › Discord exists. */
  discord_available?: boolean;
}

// ---- Discord presence (src-tauri/src/discord/) ----------------------------------------------------------------------
export type DiscordPrefs = import("./discord/presence").Prefs;
export type DiscordLink = "off" | "waiting" | "connected" | "busy" | "refused";
export interface DiscordStatus { link: DiscordLink; code: number | null; sent: number }
/** `available`: the build names a Discord application (otherwise the section does not exist); `detected`: a Discord
 *  pipe exists on this PC (a listing: nothing opened). */
export interface DiscordView { available: boolean; prefs: DiscordPrefs; status: DiscordStatus; detected: boolean }

export const discordApi = {
  view: () => call<DiscordView>("discord_view"),
  setPrefs: (prefs: DiscordPrefs) => call<DiscordView>("discord_set_prefs", { prefs }),
  /** The activity of this moment, `null` for none; sent to Discord only while the presence is on. */
  publish: (activity: import("./discord/presence").ActivityInput | null) => call<void>("discord_publish", { activity }),
};

export function onDiscordStatus(cb: (s: DiscordStatus) => void): Promise<UnlistenFn> {
  return listen<DiscordStatus>("discord-status", (e) => cb(e.payload));
}
export interface GameLanguageInfo { fonts_ok: boolean; fonts_dir: string | null; like_launcher: string }
export interface Check { id: string; path: string; present: boolean; required: boolean; size: number | null; modified: number | null }
export interface LastRunLog { path: string; name: string; modified: number | null; size: number; status: "fatal" | "completed" | "unknown" | "empty"; problem_lines: string[] }
export interface InstallStatus {
  root: string | null; root_exists: boolean; checks: Check[]; missing: string[]; ready: boolean;
  build_date: number | null; userdata: string | null; logs_dir: string | null; last_log: LastRunLog | null;
}
export interface GameStatus {
  state: "idle" | "running" | "exited"; pid: number | null; mode: string | null; started_at: number | null; ended_at: number | null;
  exit_code: number | null; stopped_by_user: boolean; abnormal: boolean; log_path: string | null; stdout_path: string | null; tail: string | null;
  restarted: boolean;
}

export type FieldKind = "bool" | "int" | "float" | "enum" | "size" | "choice_int";
export interface SchemaField {
  key: string; section: string; ini_section: string; type: FieldKind; default: string | null;
  min: number | null; max: number | null; options: string[]; presets: string[]; slider: boolean; step: number | null; label: Labels; help: Labels;
}
export interface ButtonSpec { id: string; label: Labels; default: string }
export interface Schema {
  version: number; sections: { id: string; label: Labels }[]; fields: SchemaField[];
  bindings: { ini_section: string; buttons: ButtonSpec[]; key_only_buttons: ButtonSpec[]; pad_names: string[]; key_names: string[] };
}
export interface SettingsState { path: string; exists: boolean; values: Record<string, string>; unknown_keys: string[] }
export interface Change { key: string; value: string | null }

export interface GameEntry {
  id: number; row: string; name: Labels; icon: string | null; icon_approx: boolean; order: number; season: number | null;
  selectable: boolean; how_to_get: string | null; species: "inkling" | "octoling" | null; brand: string | null;
  rarity: number | null; ability: string | null; class: string | null; sub: string | null; special: string | null; points: number | null;
  variations: number | null; unavailable: string | null;
  /** Skin tone, eye color: "#rrggbb" read from the player's game files (a second one for two-tone eyes). */
  colors?: string[];
}
export interface Named { name: Labels; icon: string | null }
export interface CategoryReport { category: string; rows: number; selectable: number; with_icon: number; with_name: number; note: string }
export type CategoryId =
  | "style" | "skin" | "eyes" | "hair" | "eyebrows" | "legwear" | "head" | "clothes" | "shoes" | "weapon" | "banner" | "adjective" | "subject" | "badge";
/** A versus stage of the player's game data: `scene` is the id of the engine's list, the protocol and the results;
 *  `image` a picture key served at the `sizes` of [`StagesView`] (`<image>@<width>`). */
export interface GameStage { scene: string; id: number; name: Labels; image: string | null; listed: boolean; order: number }
export interface GameCatalogue {
  schema: number; cache_key: string; build_ms: number; categories: Record<CategoryId, GameEntry[]>;
  brands: Record<string, Named>; abilities: Record<string, Named>; subs: Record<string, Named>; specials: Record<string, Named>;
  classes: Record<string, Named>; stages?: GameStage[]; report: CategoryReport[];
}
export interface CatalogueView {
  catalogue: GameCatalogue; origin: "game" | "neutral"; reason: string | null; from_cache: boolean; image: string | null; name_max_chars: number;
}
export interface StagesView { stages: GameStage[]; origin: "game" | "neutral"; reason: string | null; sizes: number[] }
export interface IconCacheInfo { files: number; bytes: number; warming: boolean; dir: string | null }
export interface IconProgress { done: number; total: number; failed: number; running: boolean }

/** profile.json (schema 1): every number is the id the game stores; null = not chosen. */
export interface Profile {
  schema: number; name: string;
  style: number | null; skin: number | null; eyes: number | null; hair: number | null; eyebrows: number | null; legwear: number | null; legwear_variation: number | null;
  head: number | null; clothes: number | null; shoes: number | null; weapon: number | null; banner: number | null;
  title: { adjective: number | null; subject: number | null };
  badges: (number | null)[];
}
export const EMPTY_PROFILE: Profile = {
  schema: 1, name: "", style: null, skin: null, eyes: null, hair: null, eyebrows: null, legwear: null, legwear_variation: null,
  head: null, clothes: null, shoes: null, weapon: null, banner: null, title: { adjective: null, subject: null }, badges: [null, null, null],
};

export interface LanRequest {
  mode: LanMode; relay_host: string; relay_port: number; room: string; net_log: boolean; bind_any: boolean; lan_address: string;
}
export interface RelayStatus { running: boolean; pid: number | null; port: number | null; exe: string | null; log_path: string; tail: string | null; exited: number | null }
export type ModeId = "hub" | "range" | "lan" | "original";
export type LaunchedMode = ModeId | "match";
export interface LaunchOptions { apply_profile: boolean; skip_first_launch: boolean; unlock_all: boolean }
export interface Refusal { field: string; code: string; id: number | null; species: string | null; style: string | null; from_save: boolean; raw: string }
export interface Feedback {
  state: "applied" | "already" | "new_save" | "restarting" | "needs_appearance" | "failed" | "none";
  changes: number | null; backup: string | null; reason: string | null; refusal: Refusal | null; missing: string[];
  deleted_backups: number | null; keep_backups: number | null; lines: string[];
}
export interface LaunchRecord {
  started_at: number; mode: string; slot: string; profile: "passed" | "disabled" | "skipped" | "incomplete" | "invalid"; missing: string[]; detail: string | null;
  skip_first_launch: boolean; unlock_all: boolean; save_existed: boolean; log_path: string; restart_log_path: string; userdata: string; profile_dir: string;
  scene: string | null; lan: string | null; room: string | null; nickname: string | null; env: [string, string][];
}
export interface BackupInfo { dir: string; exists: boolean; count: number; newest: string | null }
export interface LaunchReport { record: LaunchRecord | null; feedback: Feedback; backup_exists: boolean; backups: BackupInfo | null }

export interface CardPreview { name: string; relay: string | null; relay_port: number | null; api: string | null }
export interface Preflight {
  /** The profile was checked against the game's catalogue (false for the first screen's light version). */
  deep: boolean;
  save_exists: boolean; userdata: string | null; apply_profile: boolean; profile: "passed" | "disabled" | "incomplete" | "invalid"; missing: string[]; detail: string | null;
  name: string; name_ok: boolean;
  lan: { enabled: boolean; ready: boolean; problem: string | null; mode: LanMode; value: string | null; room: string | null };
  modes: { id: string; enabled: boolean; var: string; value: string }[];
  backups: BackupInfo;
  second: { name: string; dir: string; path: string | null; dir_ok: boolean; exists: boolean; save_exists: boolean; lan_address: string };
}
export interface PlayRequest { mode: ModeId; ignore_profile?: boolean; untouched_save?: boolean; extra_args?: string }

/** The launcher's command line: `--play full|training|roam` (as a mode id), `--minimized`. */
export interface Cli { play: QuickMode | null; minimized: boolean; invalid: string[] }
export interface Boot {
  info: AppInfo; config: ConfigView; status: InstallStatus; game: GameStatus; game2: GameStatus; last_run: GameStatus | null;
  install_active: boolean; migration: Migration | null; servers: ServersView; profile: Profile; cli: Cli;
}

export const api = {
  appInfo: () => call<AppInfo>("app_info"),
  getConfig: () => call<ConfigView>("get_config"),
  setRoot: (path: string | null) => call<ConfigView>("set_root", { path }),
  setTranslator: (path: string | null) => call<ConfigView>("set_translator", { path }),
  setLanguage: (language: string) => call<ConfigView>("set_language", { language }),
  gameLanguageInfo: () => call<GameLanguageInfo>("game_language_info"),
  setGameLangFollow: (follow: "launcher" | "windows") => call<ConfigView>("set_game_lang_follow", { follow }),
  setLanEnabled: (enabled: boolean) => call<ConfigView>("set_lan_enabled", { enabled }),
  installStatus: () => call<InstallStatus>("install_status"),
  settingsSchema: () => call<Schema>("settings_schema"),
  settingsLoad: () => call<SettingsState>("settings_load"),
  settingsSave: (changes: Change[]) => call<SettingsState>("settings_save", { changes }),
  launchMode: (request: PlayRequest) => call<GameStatus>("launch_mode", { request }),
  launchSecond: (request: PlayRequest) => call<GameStatus>("launch_second", { request }),
  playPreflight: (light = false) => call<Preflight>("play_preflight", { light }),
  /** Everything the first screen needs, in one call (no catalogue, no RomFS). */
  boot: () => call<Boot>("boot"),
  /** Brings the running game's window to the front; false when it has none. */
  gameFocus: () => call<boolean>("game_focus"),
  backupsInfo: () => call<BackupInfo>("backups_info"),
  setSecondPlayer: (name: string, dir: string) => call<ConfigView>("set_second_player", { name, dir }),
  setLanPrefs: (request: LanRequest) => call<ConfigView>("set_lan_prefs", { request }),
  gameStatus: () => call<GameStatus>("game_status"),
  game2Status: () => call<GameStatus>("game2_status"),
  game2Stop: () => call<void>("game2_stop"),
  lastRun: () => call<GameStatus | null>("last_run"),
  gameStop: () => call<void>("game_stop"),
  openLogsFolder: () => call<string>("open_logs_folder"),
  readLogTail: (lines = 60) => call<string | null>("read_log_tail", { lines }),
  lanGenerateRoom: () => call<string>("lan_generate_room"),
  lanPreview: (request: LanRequest) => call<[string, string][]>("lan_preview", { request }),
  relayStatus: () => call<RelayStatus>("relay_status"),
  relayStart: (port: number) => call<RelayStatus>("relay_start", { port }),
  relayStop: () => call<RelayStatus>("relay_stop"),
  setRelayExe: (path: string | null) => call<ConfigView>("set_relay_exe", { path }),
  launchReport: (second = false) => call<LaunchReport>("launch_report", { second }),
  setLaunchOptions: (options: LaunchOptions) => call<ConfigView>("set_launch_options", { options }),
  profileCatalogue: (force = false) => call<CatalogueView>("profile_catalogue", { force }),
  stages: () => call<StagesView>("gamedata_stages"),
  iconCache: () => call<IconCacheInfo>("gamedata_icon_cache"),
  iconsWarm: () => call<boolean>("gamedata_icons_warm"),
  iconsCancel: () => call<void>("gamedata_icons_cancel"),
  cacheClear: () => call<void>("gamedata_cache_clear"),
  profileLoad: () => call<Profile>("profile_load"),
  profileSave: (profile: Profile) => call<Profile>("profile_save", { profile }),
  setMatchServer: (value: string) => call<ConfigView>("set_match_server", { value }),
  matchServerFromCard: (path: string) => call<string>("match_server_from_card", { path }),
  matchCreate: (request: MatchCreateRequest) => call<MatchPlan>("match_create", { request }),
  matchParse: (code: string) => call<MatchPlan>("match_parse", { code }),
  matchNextRound: (code: string) => call<MatchPlan>("match_next_round", { code }),
  matchStart: (request: { code: string; role: MatchRole; confirm_relay?: boolean }) => call<MatchRun>("match_start", { request }),
  matchPoll: () => call<MatchProgress>("match_poll"),
  matchResult: () => call<MatchResultView | null>("match_result"),
  matchLast: () => call<MatchRun | null>("match_last"),
  migrationStatus: () => call<Migration | null>("migration_status"),
  migrationLast: () => call<Migration | null>("migration_last"),
  migrationAck: () => call<void>("migration_ack"),
  migrationImport: (path: string) => call<Migration>("migration_import", { path }),
  setUiPrefs: (ui: UiPrefs) => call<ConfigView>("set_ui_prefs", { ui }),
};

export function onIconProgress(cb: (p: IconProgress) => void): Promise<UnlistenFn> {
  return listen<IconProgress>("gamedata-icons", (e) => cb(e.payload));
}

export function onGameStatus(cb: (s: GameStatus) => void): Promise<UnlistenFn> {
  return listen<GameStatus>("game-status", (e) => cb(e.payload));
}

export function onGame2Status(cb: (s: GameStatus) => void): Promise<UnlistenFn> {
  return listen<GameStatus>("game-status-p2", (e) => cb(e.payload));
}

// ---- private match (src-tauri/src/commands.rs, automatch.rs, matchcode.rs) ----------------------------------------
export type MatchRole = "host" | "join";
export type MatchModeId = "turf" | "zones" | "tower" | "rainmaker" | "clams";
export interface MatchSpec { relay_ip: string; relay_port: number; room: string; round: number; host: string; players: number; mode: MatchModeId }
export interface MatchPlan { code: string; spec: MatchSpec; match_id: string; relay: string; rules: string; verified: boolean; own_code: boolean; relay_resolved: string | null; relay_known: boolean }
export interface MatchCreateRequest { players: number; mode: string; room?: string | null }
export interface MatchRun {
  match_id: string; role: MatchRole; code: string; started_at: number; status_path: string; result_path: string; log_path: string;
  host: string; players: number; mode: string;
}
export type MatchState =
  | "booting" | "lobby" | "creating-room" | "waiting-for-players" | "joining" | "in-room" | "loading-stage" | "in-match" | "results" | "finished" | "error" | "unknown";
export interface MatchStatus {
  match_id: string | null; role: string | null; state: MatchState; since_unix: number | null; detail: string | null;
  present: number | null; expected: number | null; error: string | null; reason: string | null; visible: boolean | null; pid: number | null;
  source: "file" | "log";
}
export interface MatchProgress { run: MatchRun | null; game: GameStatus; status: MatchStatus | null; result_ready: boolean; elapsed_s: number }
export interface TeamResult { team: string; paint_points: number; turf_permille: number }
export interface PlayerResult { name: string; self: boolean; team: string; paint: number; kills: number; assists: number; deaths: number; specials: number; disconnected: boolean }
export interface MatchResultView {
  match_id: string; role: string; error: string | null; reason: string | null; mode: string; stage_scene: string; end_utc: number; duration_s: number;
  win_team: number | null; winner: string; teams: TeamResult[]; players: PlayerResult[]; outcome: "victory" | "defeat" | "unknown"; my_team: string | null;
}

// ---- install wizard (src-tauri/src/installer/, commands.rs "install wizard") -------------------------------------
export type StepId =
  | "checks" | "prereqs" | "sources" | "python" | "exefs" | "hactool" | "romfs" | "lift" | "compile" | "xlate" | "keys" | "layout" | "selftest" | "cleanup";
export interface InstallRequest {
  install_dir: string; base_nsp: string; update_nsp: string; prod_keys: string; jobs: number; allow_prereqs: boolean; force_rebuild: boolean;
  free_build_files: boolean; low_priority: boolean;
}
/** What an update redoes to the generated C and the objects (installer/incremental.rs `UpdatePreview`). */
export interface UpdatePreview {
  kind: "none" | "runtime" | "generated" | "generator" | "full";
  reasons: ("force" | "objects_missing" | "lift_missing" | "headers" | "flags" | "toolchain")[];
  changed_files: number; changed_runtime: number; changed_generator: number; lift: boolean; compile: boolean;
  objects: number; generated_objects: number; objects_total: number; lift_secs: number; compile_secs: number; compile_secs_max: number;
}
/** The installed game against the source this launcher carries (installer/steps.rs `UpdateCheck`). */
export interface UpdateCheck {
  install_dir: string; available: boolean; installed_source: string | null; bundled_commit: string;
  redo: StepId[]; update: UpdatePreview | null; eta: number; eta_max: number;
  /** The player chose to run it when the launcher closes. */
  at_close: boolean;
}
/** Set by the Play page's "Update" button: the wizard opens straight on the plan of that install. `showResult`: the
 *  rail's installation was clicked after it ended: the wizard shows how it ended (ready, or why it stopped). */
export const wizardIntent: { updateDir: string | null; showResult: boolean } = { updateDir: null, showResult: false };
export interface InstallError { step: StepId; code: string; params: [string, string][]; log: string | null; tail: string[]; exit_code: number | null }
export interface StepView { id: StepId; status: "pending" | "running" | "done" | "skipped" | "failed" | "cancelled"; secs: number | null; reference: number; note: string | null }
export interface InstallSnapshot {
  active: boolean; outcome: "done" | "failed" | "cancelled" | "reboot" | null; install_dir: string | null; game_root: string | null;
  steps: StepView[]; current: StepId | null; overall: number; step_fraction: number | null; elapsed: number; eta: number | null;
  step_elapsed: number; step_eta: number | null; stage: string | null; detail: string | null; waiting: string | null; tail: string[];
  log: string | null; logs_dir: string | null; error: InstallError | null; footprint: number | null; cancel_requested: boolean;
  warnings: string[]; source_commit: string | null;
  /** Steps running beside the current one (the toolchain download, the RomFS merge). */
  background: BackgroundStep[];
  /** The run began with a game in place (an update): that version stays playable until the link replaces it. */
  update: boolean;
  /** The merged game data is complete: the Locker reads names, icons and colors from it, long before the end. */
  game_data: boolean;
  /** Seconds until then, while it is not. */
  game_data_eta: number | null;
  /** The game's executable is being replaced right now: a launch waits a few seconds. */
  replacing: boolean;
}
/** The words of the notification-area icon of an update that runs when the launcher closes (`{pct}` in `tip`). */
export interface TrayTexts { tip: string; open: string; quit: string }
export interface BackgroundStep { id: StepId; fraction: number | null; stage: string | null; detail: string | null; elapsed: number }
export interface BundleManifest { commit: string; files: number; bytes: number; sha256: string }
export interface WizardInfo {
  request: InstallRequest; pending: boolean; snapshot: InstallSnapshot; bundle: BundleManifest | null; bundle_error: string | null; default_jobs: number; cpus: number;
  ram: number | null; state_finished: boolean | null; needs_reboot: boolean; last_error: InstallError | null; existing: ExistingInstall[];
  interrupted_step: StepId | null; recovered: string[];
  /** A first install on this PC (installer::fresh_estimate). */
  fresh: { disk_need: number; footprint: number; eta: number; download_bytes: number };
}
export interface ExistingInstall {
  dir: string; kind: "kit" | "wizard"; game_root: string; exe: boolean; exefs: boolean; romfs: boolean; lift: boolean; objects: boolean; xlate: boolean;
  venv: boolean; source: string | null; launcher_data: string | null; needs_files: boolean; previous: boolean; saves: boolean;
}
export interface RollbackReport { now: string; previous: string }
export interface FileCheck { status: "ok" | "warn" | "bad"; code: string; params: [string, string][]; size: number | null; program_size: number | null }
export interface DirCheck { kind: "new" | "wizard" | "kit" | "other"; free: number | null; need: number; absolute: boolean; needs_files: boolean }
export interface PrereqTool { id: string; winget: string; path: string | null }
export interface Download { id: string; source: string; items: string[]; needed: boolean; bytes: number }
export interface InstallPlan {
  install_dir: string; game_root: string; tools: PrereqTool[]; missing_packages: string[]; winget: string | null; needs_admin: boolean;
  needs_prereqs: boolean; toolchain: "portable" | "msys2" | "system"; tools_dir: string; downloads: Download[]; download_bytes: number;
  steps: { id: StepId; done: boolean; reference: number }[]; disk_free: number | null; disk_needed: number; disk_peak_step: StepId | null; footprint: number; ram: number | null; cpus: number; jobs: number; source_commit: string | null;
  eta: number; eta_max: number; existing: boolean; existing_finished: boolean; foreign: boolean; needs_files: boolean; incremental: boolean;
  adopted_from_kit: boolean; build_files: number; update: UpdatePreview | null; installed_source: string | null;
}
export interface UninstallReport { removed: string[]; kept: string[]; freed: number }

export const installApi = {
  info: () => call<WizardInfo>("installer_info"),
  checkFile: (kind: "base" | "update" | "keys", path: string) => call<FileCheck>("installer_check_file", { kind, path }),
  checkDir: (path: string) => call<DirCheck>("installer_check_dir", { path }),
  plan: (request: InstallRequest) => call<InstallPlan>("installer_plan", { request }),
  updateCheck: () => call<UpdateCheck | null>("installer_update_check"),
  start: (request: InstallRequest) => call<InstallSnapshot>("installer_start", { request }),
  status: () => call<InstallSnapshot>("installer_status"),
  cancel: () => call<void>("installer_cancel"),
  openLogs: (dir: string) => call<string>("installer_open_logs", { dir }),
  uninstall: (dir: string, removeSaves: boolean) => call<UninstallReport>("installer_uninstall", { dir, removeSaves }),
  rollback: (dir: string) => call<RollbackReport>("installer_rollback", { dir }),
  close: (choice: "background" | "stop") => call<void>("installer_close", { choice }),
  /** "When the launcher closes": the update of `request` starts when the window is closed; `null` cancels. */
  atClose: (request: InstallRequest | null, texts: TrayTexts | null) => call<boolean>("installer_at_close", { request, texts }),
  /** The end of an installation while the window is in the background: taskbar flash and Windows notification. */
  attention: (title: string, body: string) => call<boolean>("notify_attention", { title, body }),
};

export function onInstallCloseRequested(cb: () => void): Promise<UnlistenFn> {
  return listen("installer-close-requested", () => cb());
}

export function onInstallProgress(cb: (s: InstallSnapshot) => void): Promise<UnlistenFn> {
  return listen<InstallSnapshot>("installer-progress", (e) => cb(e.payload));
}

// ---- community servers by name (src-tauri/src/servers.rs, online.rs) -----------------------------------------------
export interface Features {
  servers: number; private_match: boolean; ranked: boolean; friends: boolean; rankings: boolean; provider: string | null; provider_id: string | null;
}
export interface ServerView {
  id: string; name: string; api: string | null; relay_host: string | null; relay_port: number; last_ip: string | null; selected: boolean;
  /** Community service: stay connected in the background (off by default). */
  background: boolean;
}
export interface ServersView { servers: ServerView[]; selected: string | null; features: Features }
export interface ResolveView { host: string; ip: string }

export const serversApi = {
  view: () => call<ServersView>("servers_view"),
  addLine: (line: string, name: string | null, api: string | null) => call<ServersView>("server_add_line", { line, name, api }),
  remove: (id: string) => call<ServersView>("server_remove", { id }),
  select: (id: string) => call<ServersView>("server_select", { id }),
  resolve: (id: string) => call<ResolveView>("server_resolve", { id }),
  /** A server card of the first format: read it, then save it once the player has seen it. */
  cardPreview: (path: string) => call<CardPreview>("server_card_preview", { path }),
  cardImport: (path: string) => call<ServersView>("server_card_import", { path }),
};

// ---- the launcher's own updates (src-tauri/src/update/) --------------------------------------------------------------
export type UpdateChannel = "stable" | "beta";
export interface UpdatePrefs { auto_check: boolean; channel: UpdateChannel }
/** `swap`: the program file is replaced and Kalamar restarts; `installer`: the release's installer runs (an installed
 *  copy); `elsewhere`: the new program is saved in a folder the player picks (`why`: `temp` | `readonly`). */
export type UpdateMode = "swap" | "installer" | "elsewhere";
export interface UpdateAvailable {
  version: string; channel: UpdateChannel;
  /** Plain text from the signed manifest: shown as text, never as HTML or Markdown. */
  notes: string; published: string | null; size: number; mode: UpdateMode; why: "temp" | "readonly" | null;
}
export interface UpdateView {
  /** This build names an update repository: without it, nothing about updates is shown. */
  enabled: boolean; current: string; auto_check: boolean; channel: UpdateChannel;
  last_check: number | null; checking: boolean; check_error: string | null;
  available: UpdateAvailable | null; later: boolean;
  phase: "idle" | "downloading" | "ready" | "applying" | "restarting" | "saved";
  progress: { done: number; total: number } | null; error: string | null;
  blocked: "game" | "install" | "relay" | null; saved_to: string | null;
  notice: { kind: "updated" | "rolled_back" | "installer_unfinished"; from: string; to: string } | null;
}

export const updateApi = {
  status: () => call<UpdateView>("update_status"),
  /** The page is up: the witness of a new version's first start. */
  confirmStart: () => call<UpdateView>("update_confirm_start"),
  check: () => call<UpdateView>("update_check"),
  download: () => call<UpdateView>("update_download"),
  cancel: () => call<UpdateView>("update_cancel"),
  later: () => call<UpdateView>("update_later"),
  dismiss: () => call<UpdateView>("update_dismiss"),
  apply: () => call<UpdateView>("update_apply"),
  saveTo: (dir: string) => call<UpdateView>("update_save_to", { dir }),
  showSaved: () => call<void>("update_show_saved"),
  setPrefs: (prefs: UpdatePrefs) => call<ConfigView>("update_set_prefs", { prefs }),
};

export function onUpdateState(cb: (v: UpdateView) => void): Promise<UnlistenFn> {
  return listen<UpdateView>("update-state", (e) => cb(e.payload));
}
