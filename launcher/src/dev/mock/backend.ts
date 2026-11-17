// The made-up back end of the browser preview (vite --mode mock). Every IPC command of src/api.ts and
// src/community/api.ts answers here with plausible data; the URL chooses the situation, so that each screen can be
// opened directly (and captured by scripts/ui-shots.mjs):
//   ?lang=fr|en|es|ja  &adv=1  &page=play|profile|community|settings|setup  &section=<settings section>
//   &game=ready|none|running|restarted|crash|stopped|match   &update=1   &install=running|failed|done|pending
//   &server=none|service|relay|multi (three servers: two services, one relay)   &bg=1 (stay connected: the Play page
//   gets the live state)   &known=0 (the server's meta was never read: no description or areas yet)
//   &gl=<code>|zh-CN (a game language set in the settings file) &glfollow=windows &glfonts=0 (no extracted system fonts)
//   &pop=0 (the servers do not publish their population)
//   &login=out   &room=host|guest   &ticket=1   &match=found|run|stop|ended   &gs=<game state>
//   &profile=incomplete|refused   &launchfail=incomplete|error   &pm=wait|fail|results   &wiz=existing
//   &ver=old (the installed game is the previous game version: "game update needed" on Play and in the community page)
//   &match=draft&ds=you|them|vote|timeout|random|done|mode|auto (the stage draft, mock/draft.ts); &match=found&ds=waiting
//   &spect=room|sit|old|watch|players|late|none (the stands, 1.1.7: mock/stands.ts)   &match=done (a decided match)
//   &match=forfeit&fft=won|lost|team|mate (1.1.11: a ranked match decided by forfeit, a player left it in play: the
//        opponent left (won), the player left (lost), 2 v 2: the other team's player left (team), a team-mate left
//        (mate); the history (Standing tab) lists such matches too)
//   &match=ask&ask=not-joined|too-many (the launcher asks before it starts the game for a match it did not join)
//   &match=weapon (the window to change weapon before the launch: 20 s, then the game starts; "Ready" ends it sooner)
//        &wsave=fail (every save of the profile is refused, as for an item the game data does not have)
//   &iwarn=outside_profile,ram_low (warnings of a running install's checks)
//   &relays=1 (1.1.9: several relays; the Account tab shows their measurement in every state, ending after 3 s)
//   &stages=none (no game data: no stage pictures or names, the cards are neutral and the room form has its list)
//   &lup=uptodate|never|available|beta|downloading|ready|blocked|relay|installer|elsewhere|readonly|saved|error|integrity|
//        untrusted|rate|updated|rolledback|restarting|later (the launcher's own update; absent: a build without an update
//        repository, nothing shown; "later": put off, the bar is hidden and the Settings entry has its dot)
//        &dlg=0 (the dialog that asks once at the start was already answered: the bar alone; without it, the dialog
//        opens 1.5 s after the page is up and idle)
//   while it compiles: &ist=early|merge|compile|link|waitgame (how far a running install is: before the merge, the merge
//        beside the lift (default), the compile with the game data ready, an update putting the new executable in
//        place, an update waiting for the game to be closed)   &iupd=1 (the running install is an update of a playable
//        game)   &profile=empty (no name, no character yet)   &tasks=some|all (settings, profile and server already
//        set during the wait)   &upd=short|long|full (the game update offered on Play)   &atclose=1 (chosen for when
//        the launcher closes)
//   &dc=absent|off|closed|on|minimal|full|waiting|busy|refused (the Discord presence, Settings › Discord; see DC below)
// Nothing here talks to the network or reads a file.
/* eslint-disable @typescript-eslint/no-explicit-any */
import schemaJson from "../../../src-tauri/resources/settings-schema.json";
import { emit } from "./event";
import { markDialogDone } from "../../update/prompt";
import { MOCK_MAIN_ABILITIES, MOCK_SUB_ABILITIES, STAGE_IDS, catalogue, stagesView } from "./catalogue";
import { mockDraft, mockDraftAct, mockDraftMatch } from "./draft";
import { META_STANDS, OLD_REFUSAL, ROOM_FIELDS, ROOM_STANDS, standsDraft, standsMatch, standsRoom, standsRunner, watchable } from "./stands";
import { RANK_OF } from "./ranks";

const q = new URLSearchParams(typeof location === "undefined" ? "" : location.search);
const S = {
  lang: q.get("lang") ?? "en",
  adv: q.get("adv") === "1",
  game: q.get("game") ?? "ready",
  update: q.get("update") === "1" || !!q.get("upd"),
  install: q.get("install") ?? "none",
  server: q.get("server") ?? "none",
  bg: q.get("bg") === "1",
  known: q.get("known") !== "0",
  pop: q.get("pop") !== "0",
  gl: q.get("gl"),
  glfollow: q.get("glfollow") === "windows" ? "windows" : "launcher",
  glfonts: q.get("glfonts") !== "0",
  login: q.get("login") ?? "in",
  room: q.get("room"),
  ticket: q.get("ticket") === "1",
  match: q.get("match"),
  gs: q.get("gs") ?? "waiting-for-players",
  profile: q.get("profile"),
  gear: q.get("gear"),
  launchfail: q.get("launchfail"),
  pm: q.get("pm"),
  wiz: q.get("wiz"),
  ver: q.get("ver") === "old",
  stages: q.get("stages"),
  ds: q.get("ds"),
  spect: q.get("spect"),
  lup: q.get("lup"),
  fft: q.get("fft") ?? "won",
  ist: q.get("ist") ?? "merge",
  iupd: q.get("iupd") === "1" || q.get("ist") === "waitgame" || q.get("ist") === "link",
  tasks: q.get("tasks"),
  upd: q.get("upd"),
  atclose: q.get("atclose") === "1",
  dc: q.get("dc"),
  // the engine's error of `match=stop` and `pm=fail` (default network-unavailable), and a room start the server refuses
  // (`startfail=relay_capacity`: every relay full, 1.1.13)
  code: q.get("code"),
  startfail: q.get("startfail"),
  wsave: q.get("wsave"),
};

/** The engine's sentence for an error code of the mock (`&code=`). */
const ENGINE_REASON: Record<string, string> = {
  "network-unavailable": "relay 203.0.113.7:47313 unreachable (timed out after 20 s)",
  "relay-full": "the relay 203.0.113.7:47313 refused this player in room M-1: the relay is full",
  "relay-room-full": "the relay 203.0.113.7:47313 refused this player in room M-1: the match's room on the relay is full",
  "relay-source-limit": "the relay 203.0.113.7:47313 refused this player in room M-1: too many members of this match come from this player's Internet address",
  "relay-source-full": "the relay 203.0.113.7:47313 refused this player in room M-1: too many players on the relay come from this player's Internet address",
};
const engineCode = () => S.code ?? "network-unavailable";

/** 1.1.13: a room start when every relay of the server is full (`&startfail=relay_capacity`). */
const RELAY_CAPACITY = {
  kind: "api", code: "relay_capacity", status: 503, message: "every relay of this server is full: try again in a moment", details: [], fields: [], retry_after_s: 30, data: null,
};

const now = () => Math.floor(Date.now() / 1000);
const ROOT = `D:\\Games\\Kalamar\\work\\splatoon3-${S.ver ? "11.2.0" : "11.3.0"}`;
const DIR = "D:\\Games\\Kalamar";
const fail = (kind: string, message = "", details: string[] = []) => ({ kind, message, details, fields: [] });

// ---- configuration ------------------------------------------------------------------------------------------------
const config: any = {
  root: ROOT, translator: null, language: S.lang, lan_enabled: S.adv, apply_profile: true, skip_first_launch: true, unlock_all: true, unlock_chunks: false,
  lan: { mode: "lan", relay_host: "", relay_port: 47313, room: "", net_log: false, bind_any: false, lan_address: "" },
  relay_exe: null, relay_port: 47313, second_name: "Player2", second_dir: "userdata-p2", match_server: "",
  game_lang_follow: S.glfollow,
  ui: { full_untouched: false, adv_open: false, match_players: 2, match_mode: "turf", last_mode: "original", minimize_on_play: false, advanced: S.adv, sounds: true, sound_volume: 35, notify: true },
  install: {}, update: { auto_check: true, channel: q.get("lup") === "beta" ? "beta" : "stable" }, effective_root: ROOT, effective_translator: `${DIR}\\work\\xlate\\totk_xlate.exe`,
};
const view = () => JSON.parse(JSON.stringify(config));

const ready = S.game !== "none" && S.game !== "partial";
const partial = S.game === "partial";
const checks = [
  { id: "exe", path: `${ROOT}\\build\\splat3.exe`, present: ready || partial, required: true, size: 412_000_000, modified: now() - 86400 },
  { id: "romfs", path: `${ROOT}\\romfs.bin`, present: ready, required: true, size: 9_800_000_000, modified: now() - 86400 },
  { id: "translator", path: `${DIR}\\work\\xlate\\totk_xlate.exe`, present: ready, required: true, size: 2_400_000, modified: now() - 86400 },
  { id: "save", path: `${ROOT}\\userdata\\save`, present: ready, required: false, size: null, modified: null },
  { id: "settings", path: `${ROOT}\\userdata\\settings.ini`, present: ready, required: false, size: 2048, modified: now() - 3600 },
];
const status: any = {
  root: ready || partial ? ROOT : null, root_exists: ready || partial, checks, missing: ready ? [] : partial ? ["romfs", "translator"] : ["exe", "romfs"], ready, build_date: ready ? now() - 86400 * 2 : null,
  userdata: `${ROOT}\\userdata`, logs_dir: `${ROOT}\\userdata\\logs`,
  last_log: ready ? { path: `${ROOT}\\userdata\\logs\\run-20261111-1402.log`, name: "run-20261111-1402.log", modified: now() - 600, size: 184_000, status: S.game === "crash" ? "fatal" : "completed", problem_lines: S.game === "crash" ? ["FATAL: vk: upload submit failed (-4)", "device fault: write invalid"] : [] } : null,
};

const idle: any = { state: "idle", pid: null, mode: null, started_at: null, ended_at: null, exit_code: null, stopped_by_user: false, abnormal: false, log_path: null, stdout_path: null, tail: null, restarted: false };
function gameNow(): any {
  const log = `${ROOT}\\userdata\\logs\\run-20261111-1402.log`;
  if (S.ist === "waitgame" && S.install === "running") return { ...idle, state: "running", pid: 18432, mode: "original", started_at: now() - 900, log_path: log };
  switch (S.game) {
    case "running": return { ...idle, state: "running", pid: 18432, mode: "original", started_at: now() - 300, log_path: log };
    case "restarted": return { ...idle, state: "running", pid: 18436, mode: "original", started_at: now() - 40, log_path: log, restarted: true };
    case "match": return { ...idle, state: "running", pid: 18440, mode: "match", started_at: MATCH_START, log_path: log };
    case "crash": return { ...idle, state: "exited", pid: 18432, mode: "original", started_at: now() - 900, ended_at: now() - 60, exit_code: -1073741819, abnormal: true, log_path: log, tail: LOG_TAIL };
    case "stopped": return { ...idle, state: "exited", pid: 18432, mode: "original", started_at: now() - 900, ended_at: now() - 60, exit_code: 1, stopped_by_user: true, log_path: log };
    default: return idle;
  }
}
const MATCH_START = now() - 75;
const LOG_TAIL = [
  "[14:02:11.204] vk: device AMD Radeon RX 6700 XT (driver 2.0.302)",
  "[14:02:11.980] vk: vkCreateGraphicsPipelines failed (-4)",
  "[14:02:12.003] device fault: write invalid",
  "[14:02:12.004] FATAL: vk: upload submit failed (-4)",
].join("\n");
let game = gameNow();

// ---- servers ------------------------------------------------------------------------------------------------------
const SRV: Record<string, any> = {
  main: { id: "srv-1", name: "Open Ink", api: "https://openink.example.org/api", relay_host: "relay.openink.example.org", relay_port: 47313, last_ip: "203.0.113.7", background: !!S.match || S.bg },
  relay: { id: "srv-2", name: "Friends' relay", api: null, relay_host: "203.0.113.20", relay_port: 47313, last_ip: null, background: false },
  north: { id: "srv-3", name: "Northern Tides", api: "https://north.example.net", relay_host: "relay.north.example.net", relay_port: 47313, last_ip: null, background: false },
};
const SERVER_LIST: any[] = S.server === "service" ? [SRV.main] : S.server === "relay" ? [SRV.relay] : S.server === "multi" ? [SRV.main, SRV.north, SRV.relay] : [];
let selectedId: string | null = SERVER_LIST[0]?.id ?? null;
function serversView(): any {
  const list = SERVER_LIST.map((s) => ({ ...s, selected: s.id === selectedId }));
  const sel = list.find((s) => s.selected) ?? null;
  const any = list.length > 0;
  return { servers: list, selected: sel?.id ?? null, features: { servers: list.length, private_match: any, ranked: any, friends: any, rankings: any, provider: sel?.name ?? null, provider_id: sel?.id ?? null } };
}
/** What this computer knows of each server with a service, without a request (proto_peek). */
function peek(): any[] {
  return SERVER_LIST.filter((s) => s.api).map((s) => s.id === "srv-1"
    ? { id: s.id, session: S.login === "out" ? { logged_in: false, provider: null, user_id: null, display_name: null } : { logged_in: true, provider: "direct", user_id: "u-me", display_name: "Mira" }, description: S.known ? meta.description : {}, features: S.known ? FEATURES : [] }
    : { id: s.id, session: { logged_in: false, provider: null, user_id: null, display_name: null }, description: L("Petit serveur du nord : salons privés et soirées à thème le vendredi.", "A small server up north: private rooms and themed Friday nights.", "Un pequeño servidor del norte: salas privadas y noches temáticas los viernes.", "北の小さなサーバー：プライベートルームと金曜のテーマナイト。"), features: ["rooms", "friends", "activities", ...(S.pop ? ["population"] : [])] });
}

// `&match=weapon`: two presets to switch to from the window (the first is the profile's loadout); else none yet.
let presets: any[] = S.match === "weapon" ? [
  { name: "Roller", head: 4, clothes: 9, shoes: 2, weapon: 7, sub_abilities: { head: ["a0", "a2", "a2"], clothes: ["a1", null, null], shoes: null }, main_abilities: { head: "a5", clothes: null, shoes: null } },
  { name: "Long range", head: 1, clothes: 3, shoes: 5, weapon: 10, sub_abilities: { head: null, clothes: null, shoes: null }, main_abilities: { head: null, clothes: null, shoes: null } },
] : [];
// ---- profile ------------------------------------------------------------------------------------------------------
let profile: any = S.profile === "empty"
  ? { schema: 1, name: "", style: null, skin: null, eyes: null, hair: null, eyebrows: null, legwear: null, legwear_variation: null, head: null, clothes: null, shoes: null, weapon: null, banner: null, title: { adjective: null, subject: null }, badges: [null, null, null], sub_abilities: { head: null, clothes: null, shoes: null }, main_abilities: { head: null, clothes: null, shoes: null } }
  : {
    schema: 1, name: "Mira", style: 1, skin: 3, eyes: 5, hair: S.profile === "incomplete" ? null : 2, eyebrows: 1, legwear: S.profile === "incomplete" ? null : 3, legwear_variation: 1,
    head: 4, clothes: 9, shoes: 2, weapon: 7, banner: 5, title: { adjective: 2, subject: 4 }, badges: [3, null, 11],
    sub_abilities: { head: ["a0", "a2", "a2"], clothes: ["a1", null, null], shoes: null },
    main_abilities: { head: "a5", clothes: null, shoes: null },
  };

function preflight(): any {
  const incomplete = S.profile === "incomplete";
  return {
    deep: true, save_exists: !incomplete, userdata: `${ROOT}\\userdata`, apply_profile: config.apply_profile,
    profile: !config.apply_profile ? "disabled" : incomplete ? "incomplete" : "passed", missing: incomplete ? ["hair", "legwear"] : [], detail: null, name: profile.name, name_ok: true,
    lan: { enabled: config.lan_enabled, ready: config.lan_enabled, problem: null, mode: config.lan.mode, value: "auto", room: null },
    modes: ["hub", "range", "lan", "original"].map((id) => ({ id, enabled: true, var: "SPLAT3_START", value: id })),
    backups: { dir: `${ROOT}\\userdata\\profile\\save-profile-backup`, exists: true, count: 4, newest: "save-20261111-140211" },
    second: { name: config.second_name, dir: config.second_dir, path: `${ROOT}\\${config.second_dir}`, dir_ok: true, exists: false, save_exists: false, lan_address: "127.0.0.3/8" },
  };
}

function launchReport(): any {
  const refused = S.profile === "refused";
  return {
    record: {
      started_at: now() - 900, mode: "original", slot: "p1", profile: "passed", missing: [], detail: null, skip_first_launch: true, unlock_all: true, save_existed: true,
      log_path: `${ROOT}\\userdata\\logs\\run-20261111-1402.log`, restart_log_path: "", userdata: `${ROOT}\\userdata`, profile_dir: `${ROOT}\\userdata\\profile`,
      scene: "Plaza", lan: null, room: null, nickname: null,
      env: [["SPLAT3_START", "original"], ["TOTK_SAVE_PROFILE", `${ROOT}\\userdata\\profile\\profile.json`], ["SPLAT3_SKIP_FIRST", "1"], ["SPLAT3_UNLOCK_ALL", "1"]],
    },
    feedback: refused
      ? { state: "failed", changes: null, backup: null, reason: null, refusal: { field: "gear", code: "gear_dropped", id: 9, species: null, style: null, from_save: false, raw: "save profile: gear 9 is dropped by the game at load" }, missing: [], deleted_backups: null, keep_backups: null, lines: ["save profile: refused (gear 9 is dropped by the game at load)"] }
      : { state: "applied", changes: 6, backup: `${ROOT}\\userdata\\profile\\save-profile-backup\\save-20261111-140211`, reason: null, refusal: null, missing: [], deleted_backups: 1, keep_backups: 10, lines: ["save profile: applied 6 changes"] },
    backup_exists: true,
    backups: { dir: `${ROOT}\\userdata\\profile\\save-profile-backup`, exists: true, count: 4, newest: "save-20261111-140211" },
  };
}

// ---- install wizard -----------------------------------------------------------------------------------------------
const STEPS = ["checks", "prereqs", "sources", "python", "exefs", "hactool", "romfs", "lift", "compile", "xlate", "keys", "layout", "selftest", "cleanup"];
const REF: Record<string, number> = { checks: 1, prereqs: 30, sources: 4, python: 6, exefs: 2, hactool: 1, romfs: 156, lift: 205, compile: 540, xlate: 1, keys: 7, layout: 1, selftest: 5, cleanup: 5 };
function snapshot(at: number, outcome: string | null = null): any {
  const cur = STEPS[Math.min(at, STEPS.length - 1)];
  const done = outcome === "done";
  // An update starts from an installed game: its merged data is there from the start.
  const update = S.iupd;
  const merged = update || done || STEPS.indexOf(cur) > STEPS.indexOf("lift") || (outcome != null);
  const stage = S.ist === "link" && cur === "compile" ? "link" : cur === "compile" ? "compile" : cur === "lift" ? "emit" : cur === "python" ? "download" : "extract";
  return {
    active: !outcome, outcome, install_dir: DIR, game_root: ROOT,
    steps: STEPS.map((id, i) => ({ id, status: !outcome && cur === "lift" && id === "romfs" ? "running" : (update && ["checks", "prereqs", "sources", "python", "exefs", "hactool", "romfs", "lift"].includes(id) && i < at) ? (i === 0 || i === 2 ? "done" : "skipped") : done || i < at ? (i === 1 ? "skipped" : "done") : i === at ? (outcome === "failed" ? "failed" : outcome ? "cancelled" : "running") : "pending", secs: i < at && !(cur === "lift" && id === "romfs") ? REF[id] * 0.9 : null, reference: REF[id], note: null })),
    current: outcome ? null : cur, overall: done ? 1 : update ? (S.ist === "link" ? 0.96 : S.ist === "waitgame" ? 0.94 : 0.42) : 0.04 + at * 0.065, step_fraction: S.ist === "waitgame" ? 0.96 : 0.42,
    elapsed: update ? 250 : at * 50 + 30, eta: update ? (S.ist === "compile" ? 380 : 20) : Math.max(40, 760 - at * 60), step_elapsed: 140, step_eta: 300,
    stage, detail: stage === "compile" ? "312/737" : cur === "lift" ? "312/597" : stage === "download" ? "8.4/12.6" : null, waiting: S.ist === "waitgame" && !outcome ? "game_running" : null,
    tail: ["[311/737] CC lift/main/main_0310.c", "[312/737] CC lift/main/main_0311.c", "[313/737] CC lift/main/main_0312.c"],
    log: `${DIR}\\logs\\install-20261111.log`, logs_dir: `${DIR}\\logs`,
    error: outcome === "failed" ? { step: cur, code: "command_failed", params: [["program", "ninja"]], log: `${DIR}\\logs\\install-20261111.log`, tail: ["FAILED: gen/f_0043.c.obj", "clang: error: unable to execute command: Killed", "ninja: build stopped: subcommand failed."], exit_code: 1 } : null,
    footprint: done ? 12_940_000_000 : null, cancel_requested: false, warnings: q.get("iwarn") ? q.get("iwarn")!.split(",") : [], source_commit: "4cb0304a1e9f",
    // the RomFS merge runs beside the lift
    background: !outcome && cur === "lift" ? [{ id: "romfs", fraction: 0.55, stage: "hactool", detail: "5.5/10.1", elapsed: 61 }] : [],
    update, game_data: merged, game_data_eta: merged ? null : cur === "lift" ? 110 : 150, replacing: S.ist === "link" && !outcome,
  };
}
const AT: Record<string, number> = { early: 3, merge: 7, compile: 8, link: 8, waitgame: 8 };
// "failed" and "done" open on a running install that ends a moment later (the wizard follows the events).
let snap: any = S.install === "running" ? snapshot(AT[S.ist] ?? 7) : S.install === "failed" ? snapshot(8) : S.install === "done" ? snapshot(13) : { ...snapshot(0), active: false };
let ending = false;
function endSoon() {
  if (ending || (S.install !== "failed" && S.install !== "done")) return;
  ending = true;
  setTimeout(() => { snap = S.install === "failed" ? snapshot(8, "failed") : snapshot(14, "done"); emit("installer-progress", snap); }, 600);
}

function wizardInfo(): any {
  return {
    request: { install_dir: DIR, base_nsp: "D:\\Files\\base-game.nsp", update_nsp: "D:\\Files\\update.nsp", prod_keys: "D:\\Files\\prod.keys", jobs: 0, allow_prereqs: false, force_rebuild: false, free_build_files: false, low_priority: false },
    pending: S.install === "pending", snapshot: snap, bundle: { commit: "4cb0304a1e9f6b2d", files: 1834, bytes: 42_000_000, sha256: "" },
    bundle_error: null, default_jobs: 8, cpus: 16, ram: 32 * 2 ** 30, state_finished: ready ? true : null, needs_reboot: false, last_error: null,
    existing: S.wiz === "existing" || ready ? [{ dir: DIR, kind: "wizard", game_root: ROOT, exe: ready || S.wiz === "existing", exefs: true, romfs: true, lift: true, objects: true, xlate: true, venv: true, source: "0c924ab", launcher_data: null, needs_files: S.ver, previous: !S.ver, saves: true, old_version: S.ver ? "11.2.0" : null }] : [],
    interrupted_step: S.install === "pending" ? "compile" : null, recovered: [], game_version: "11.3.0",
    fresh: { disk_need: 14_000_000_000, footprint: 13_000_000_000, eta: 780, download_bytes: 231_619_695 },
  };
}
// The game update offered (&upd=): a short one (only the engine), a long one (part of the generated code), a full
// recompilation (a header shared by all the generated code, this morning's case); default: the engine, 3 min.
const UPD: Record<string, { preview: any; eta: number; eta_max: number }> = {
  short: { preview: { kind: "runtime", reasons: [], changed_files: 3, changed_runtime: 3, changed_generator: 0, lift: false, compile: true, objects: 4, generated_objects: 0, objects_total: 737, lift_secs: 0, compile_secs: 30, compile_secs_max: 30 }, eta: 40, eta_max: 40 },
  long: { preview: { kind: "generated", reasons: ["headers"], changed_files: 9, changed_runtime: 9, changed_generator: 0, lift: false, compile: true, objects: 214, generated_objects: 188, objects_total: 737, lift_secs: 0, compile_secs: 330, compile_secs_max: 330 }, eta: 350, eta_max: 350 },
  full: { preview: { kind: "full", reasons: ["headers"], changed_files: 96, changed_runtime: 59, changed_generator: 2, lift: true, compile: true, objects: 726, generated_objects: 597, objects_total: 737, lift_secs: 112, compile_secs: 640, compile_secs_max: 640 }, eta: 720, eta_max: 720 },
};
const offered = UPD[S.upd ?? ""] ?? { preview: { kind: "runtime", reasons: [], changed_files: 14, changed_runtime: 14, changed_generator: 0, lift: false, compile: true, objects: 22, generated_objects: 0, objects_total: 11406, lift_secs: 0, compile_secs: 150, compile_secs_max: 200 }, eta: 190, eta_max: 260 };
const updatePreview = offered.preview;
let atClose = S.atclose;
function plan(): any {
  return {
    install_dir: DIR, game_root: ROOT,
    tools: [], missing_packages: [], winget: null, needs_admin: false, needs_prereqs: false, toolchain: "portable", tools_dir: `${DIR}\\tools`,
    downloads: [
      { id: "python", source: "python.org (Python 3.14.9, embeddable)", items: ["python-3.14.9-embed-amd64.zip"], needed: !S.update, bytes: 12_609_248 },
      { id: "pypi", source: "pypi.org (numpy, lz4, cryptography, cffi, pycparser)", items: ["numpy-2.5.4-cp314-cp314-win_amd64.whl"], needed: !S.update, bytes: 16_868_868 },
      { id: "llvm", source: "github.com/mstorsjo/llvm-mingw (20261020)", items: ["llvm-mingw-20261020-ucrt-x86_64.zip"], needed: !S.update, bytes: 190_612_575 },
      { id: "ninja", source: "github.com/ninja-build/ninja (1.13.2)", items: ["ninja-win.zip"], needed: !S.update, bytes: 291_570 },
      { id: "sdl3", source: "github.com/libsdl-org/SDL (3.4.20)", items: ["SDL3-3.4.20-win32-x64.zip"], needed: !S.update, bytes: 6_454_578 },
      { id: "msys2", source: "repo.msys2.org (Opus, libvpx, Vulkan, VMA, zstd)", items: [], needed: !S.update, bytes: 4_782_856 },
      { id: "shipped", source: "Kalamar (engine a62ace6)", items: ["hactool.exe", "totk_xlate.exe"], needed: !S.update, bytes: 0 },
    ],
    download_bytes: S.update ? 0 : 231_619_695,
    steps: STEPS.map((id) => ({ id, done: id === "checks" ? false : S.update ? !["compile", "layout", "selftest", "cleanup"].includes(id) : false, reference: REF[id] })),
    disk_free: 214e9, disk_needed: S.update ? 3.4e9 : 14.0e9, disk_peak_step: "keys", footprint: 13.0e9, ram: 32 * 2 ** 30, cpus: 16, jobs: 16, source_commit: "4cb0304a1e9f6b2d",
    eta: S.update ? offered.eta : 780, eta_max: S.update ? offered.eta_max : 780, existing: S.update, existing_finished: S.update, foreign: false, needs_files: !S.update, incremental: S.update,
    adopted_from_kit: false, build_files: 0, update: S.update ? updatePreview : null, installed_source: S.update ? "0c924ab" : null,
    previous_game: S.ver ? "11.2.0" : null, previous_bytes: S.ver ? 14.2e9 : 0,
  };
}
function startInstall(): any {
  let at = 0;
  snap = snapshot(at);
  const id = window.setInterval(() => {
    at += 1;
    snap = S.install === "failed" && at >= 8 ? snapshot(8, "failed") : at >= STEPS.length ? snapshot(14, "done") : snapshot(at);
    emit("installer-progress", snap);
    if (!snap.active) window.clearInterval(id);
  }, 700);
  return snap;
}

// ---- community server ---------------------------------------------------------------------------------------------
const L = (fr: string, en: string, es: string, ja: string) => ({ fr, en, es, ja });
const me = { id: "u-me", display_name: "Mira", friend_code: "KQ7M-4TRX-9PZD", created_at: "2026-10-02T18:00:00Z", settings: { share_presence: true, allow_friend_requests: true, show_on_leaderboards: true }, restrictions: [] };
const user = (id: string, name: string) => ({ id, display_name: name });
const meta: any = {
  name: "Open Ink", description: L("Serveur communautaire européen : salons privés, files classées et tournois du dimanche.", "European community server: private rooms, ranked queues and Sunday cups.", "Servidor comunitario europeo: salas privadas, colas clasificatorias y torneos de domingo.", "ヨーロッパのコミュニティサーバー：プライベートルーム、ランク戦、日曜の大会。"),
  auth: { providers: [{ id: "direct", kind: "direct", label: L("Nom et mot de passe", "Name and password", "Nombre y contraseña", "名前とパスワード"), fields: [{ key: "display_name", type: "string", required: true, label: L("Nom affiché", "Display name", "Nombre visible", "表示名") }, { key: "password", type: "string", secret: true, required: true, label: L("Mot de passe", "Password", "Contraseña", "パスワード") }] }] },
  rules: { summary: L("Respect, pas de triche, pas de pseudo offensant.", "Be respectful, no cheating, no offensive names.", "Respeto, sin trampas, sin nombres ofensivos.", "礼儀正しく、不正なし、不快な名前は禁止。"), url: "https://openink.example.org/rules" },
  privacy_url: "https://openink.example.org/privacy",
  // The stage field: the made-up stages plus one the game data does not know (its id made readable); by mode, two are
  // left out of the default mode (shown grayed); two server labels for the case without game data.
  rooms: { options: [{ key: "teams", type: "teams", label: L("Format", "Format", "Formato", "形式"), choices: [[1, 1], [2, 2], [4, 4]], default: [1, 1] }, { key: "mode", type: "mode", label: L("Mode", "Mode", "Modo", "ルール"), values: [{ id: "turf" }, { id: "zones" }, { id: "tower" }], required: true }, { key: "stage", type: "stage", nullable: true, label: L("Stage", "Stage", "Escenario", "ステージ"), values: [...STAGE_IDS, "Vss_Mystery02"].map((id) => ({ id, ...(id === "Vss_Docks00" ? { label: L("Les Docks", "The Docks", "Los Muelles", "ドック") } : id === "Vss_Park" ? { label: L("Le Parc", "The Park", "El Parque", "パーク") } : {}) })), by_mode: { turf: [...STAGE_IDS.filter((s) => s !== "Vss_GlassTower" && s !== "Vss_Quarry"), "Vss_Mystery02"], zones: STAGE_IDS, tower: STAGE_IDS.slice(0, 6) } }, { key: "draft", type: "enum", nullable: true, null_label: L("Sans choix", "No draft", "Sin elección", "なし"), label: L("Choix des stages", "Stage draft", "Elección de escenarios", "ステージ選択"), default: null, values: [{ id: "veto", label: L("Veto des stages", "Stage veto", "Veto de escenarios", "ステージ拒否") }] }, ...ROOM_FIELDS, { key: "password", type: "string", secret: true, label: L("Mot de passe (facultatif)", "Password (optional)", "Contraseña (opcional)", "パスワード（任意）") }], min_players: 2, invites: true, result_policy: "agree", spectators: ROOM_STANDS },
  spectators: META_STANDS,
  // 1.1.9: `&relays=1` gives the server six relays for the Account tab: one that answers, one measured in 3 s, one that
  // does not answer, a private address (refused for a public server), one left unmeasured (budget), and a second id for
  // the first address (measured and shown once).
  ...(q.get("relays") ? { relays: [{ id: "eu", host: "eu.openink.example.org", port: 47313, region: "eu-west", label: L("Europe", "Europe", "Europa", "ヨーロッパ") }, { id: "us", host: "us.openink.example.org", port: 47313, region: "us-east" }, { id: "asia", host: "asia.openink.example.org", port: 47313, label: L("Asie", "Asia", "Asia", "アジア") }, { id: "lan", host: "192.168.1.20", port: 47313, label: L("Salle LAN", "LAN room", "Sala LAN", "LANルーム") }, { id: "sa", host: "sa.openink.example.org", port: 47313, label: L("Amérique du Sud", "South America", "Sudamérica", "南米") }, { id: "eu-bis", host: "eu.openink.example.org", port: 47313 }] } : {}),
  result_policies: [{ id: "agree", name: L("Accord des joueurs", "Players agree", "Acuerdo de jugadores", "プレイヤーの合意"), summary: L("Le résultat compte si les rapports concordent.", "The result counts when the reports agree.", "El resultado cuenta si los informes coinciden.", "報告が一致すれば結果が記録されます。") }],
  queues: { penalties: { summary: L("Refuser une partie trouvée bloque les files quelques minutes, de plus en plus longtemps.", "Declining a found match blocks the queues for a few minutes, longer each time.", "Rechazar una partida bloquea las colas unos minutos, cada vez más.", "見つかった試合を断ると、数分間キューに入れません（回数ごとに延長）。"), cooldowns_s: [120, 600, 1800] } },
};
const FEATURES = ["rooms", "queues", "parties", "friends", "presence", "ratings", "leaderboards", "stats", "activities", "profiles", "badges", "series", "draft", "spectators", ...(S.pop ? ["population"] : [])];
const opponent = user("u-2", "Pablo");
function participants(state: string | null): any[] {
  return [
    { user: { id: "u-me", display_name: "Mira" }, member_name: "Mira", team: "alpha", role: "host", accept: "accepted", launched: true, status: state ? { state } : null, reported: false, rating: RANK_OF["u-me"] },
    { user: opponent, member_name: "Pablo", team: "bravo", role: "join", accept: S.match === "found" ? "pending" : "accepted", launched: true, status: state === "waiting-for-players" ? null : state ? { state: state === "creating-room" ? "joining" : state } : null, reported: false, rating: RANK_OF["u-2"] },
  ];
}
// The draft of `&match=draft`, changed by the player's actions.
let draftNow: any = S.match === "draft" ? (S.spect === "watch" ? standsDraft(mockDraftMatch(S.ds ?? "them")) : mockDraftMatch(S.ds ?? "you")) : null;
// The player moved into the room's stands (`&spect=room`: "sit there" clicked).
let moved = false;
function liveMatch(): any {
  if (!S.match) return null;
  const watched = standsMatch(S.match, S.spect, S.gs);
  if (watched) return watched;
  if (S.match === "draft") return draftNow;
  if (S.match === "found" && S.ds) return { id: "m-1", state: "pending_accept", rules: { teams: [1, 1], mode: "turf", rated: true, stage: null }, draft: mockDraft("waiting"), participants: participants(null).map((p) => (p.user.id === "u-me" ? { ...p, accept: "pending" } : p)), deadlines: { accept: new Date(Date.now() + 18000).toISOString() }, origin: { kind: "queue" } };
  if (S.match === "found") return { id: "m-1", state: "pending_accept", rules: { teams: [1, 1], mode: "turf", rated: true, stage: "Vss_LanternHarbor" }, participants: participants(null).map((p) => (p.user.id === "u-me" ? { ...p, accept: "pending" } : p)), deadlines: { accept: new Date(Date.now() + 18000).toISOString() }, origin: { kind: "queue" } };
  if (S.match === "ended") return null;
  if (S.match === "forfeit") return forfeitMatch();
  if (S.match === "ask") return { id: "m-1", state: "ready", rules: { teams: [1, 1], mode: "turf", rated: false, stage: "Vss_LanternHarbor" }, participants: participants("booting"), deadlines: {}, origin: { kind: "room", room_id: "r-other" } };
  if (S.match === "weapon") return { id: "m-1", state: "ready", rules: { teams: [1, 1], mode: "turf", rated: true, stage: "Vss_LanternHarbor" }, participants: participants(null), you: { user_id: "u-me", member_name: "Mira", team: "alpha", role: "host" }, deadlines: { launch: new Date(weaponEnd() + 45_000).toISOString() }, origin: { kind: "queue", queue_id: "q-1v1" } };
  return { id: "m-1", state: "running", rules: { teams: [1, 1], mode: "turf", rated: true, stage: "Vss_LanternHarbor" }, participants: participants(S.match === "stop" ? "error" : S.gs), deadlines: {}, origin: { kind: "queue" } };
}
function runner(): any {
  return standsRunner(baseRunner(), S.match, S.spect);
}
/** `&match=weapon`: the window ends 20 s after the page first asked for the runner, then the game starts (pushed as
 *  `proto-runner`, as the backend does); "Ready" (`proto_weapon_ready`) ends it at once. */
let weaponAt: number | null = null;
let weaponTimer: number | null = null;
function weaponEnd(): number {
  if (weaponAt === null) {
    weaponAt = Date.now() + 20_000;
    weaponTimer = window.setTimeout(weaponDone, 20_000);
  }
  return weaponAt;
}
function weaponDone() {
  if (weaponTimer !== null) window.clearTimeout(weaponTimer);
  weaponTimer = null;
  if (S.match !== "weapon") return;
  S.match = "run";
  S.gs = "booting";
  emit("proto-runner", { server: serversView().servers.find((x: any) => x.selected)?.id ?? "srv-1", ...runner() });
}
function baseRunner(): any {
  if (S.match === "weapon") {
    return {
      match_id: "m-1", phase: "weapon", game_state: null, game: null, code: null, reason: null, relay_ip: null, role: "host", member_name: "Mira",
      env: [], status_path: null, result_path: null, result_uploaded: false, contract: 2, keep_open: false, seat: "player", weapon_until: new Date(weaponEnd()).toISOString(),
    };
  }
  // The launcher's question before a launch it was not asked for (`proto/consent.rs`).
  if (S.match === "ask") {
    return {
      match_id: "m-1", phase: "confirm", game_state: null, game: null, code: null, reason: null, relay_ip: null, role: "join", member_name: "Mira",
      env: [], status_path: null, result_path: null, result_uploaded: false, ask: q.get("ask") === "too-many" ? "too-many" : "not-joined", relay: "relay.openink.example.org:47313", ask_s: 90,
    };
  }
  if (S.match !== "run" && S.match !== "stop") return null;
  const stop = S.match === "stop";
  return {
    match_id: "m-1", phase: stop ? "failed" : "running", game_state: stop ? "error" : S.gs, code: stop ? engineCode() : null, reason: stop ? ENGINE_REASON[engineCode()] ?? "the engine stopped the match" : null,
    relay_ip: "203.0.113.7", role: "host", member_name: "Mira",
    game: { state: stop ? "error" : S.gs, detail: stop ? "net: relay hello timed out" : null, since_ms: Date.now() - 34_000, present: 1, expected: 2, visible: ["in-match", "results"].includes(S.gs), limit_s: 180 },
    env: [["SPLAT3_MATCH", "host"], ["SPLAT3_MATCH_ID", "m-1"], ["SPLAT3_LAN", "relay:203.0.113.7:47313"], ["SPLAT3_MATCH_RULES", "teams=1,1,mode=turf"]],
    status_path: `${ROOT}\\userdata\\match\\status.json`, result_path: `${ROOT}\\userdata\\match\\result.json`, result_uploaded: false,
  };
}
function room(): any {
  return standsRoom(baseRoom(), S.spect, moved);
}
function baseRoom(): any {
  if (!S.room) return null;
  const host = S.room === "host";
  const owner = host || S.spect === "old" ? "u-me" : "u-3";
  return {
    id: "r-1", code: "BX4K-29", owner, state: "open", options: { teams: [2, 2], mode: "zones", stage: host ? "Vss_Market01" : null }, has_password: false, closes_at: new Date(Date.now() + 3600_000).toISOString(),
    members: [
      { user: { id: "u-me", display_name: "Mira" }, team: "alpha", ready: !host },
      { user: { id: "u-3", display_name: "Sacha" }, team: host ? "bravo" : "alpha", ready: host },
      { user: opponent, team: "bravo", ready: !!S.startfail },
    ],
  };
}
function endedMatch(): any {
  return {
    id: "m-0", state: "completed", rules: { teams: [1, 1], mode: "turf", rated: true }, participants: participants("finished"),
    outcome: { winner: "alpha", counted: true, record: { stage_scene: "Vss_Docks00", teams: [{ team: "alpha", paint_points: 1043, turf_permille: 562 }, { team: "bravo", paint_points: 811, turf_permille: 391 }], players: [{ name: "Mira", paint: 1043, kills: 6, assists: 2, deaths: 3 }, { name: "Pablo", paint: 811, kills: 3, assists: 1, deaths: 6 }] }, players: [{ user_id: "u-me", verdict: "agreed" }], rating_changes: [{ user_id: "u-me", ladder: "1v1", delta: 14, after: { display: "1 214" } }] },
  };
}
/** `&match=forfeit` (1.1.11): a ranked match decided by forfeit as the server sends it (no record, the summary in
 *  English and French, the forfeit's reason, team and players). */
function forfeitMatch(): any {
  const team = S.fft === "team" || S.fft === "mate";
  const sacha = { id: "u-3", display_name: "Sacha" }, noa = { id: "u-4", display_name: "Noa" };
  const parts = team
    ? [
        { user: { id: "u-me", display_name: "Mira" }, member_name: "Mira", team: "alpha", role: "host", accept: "accepted", launched: true, status: { state: "error" }, reported: true, rating: RANK_OF["u-me"] },
        { user: sacha, member_name: "Sacha", team: "alpha", role: "join", accept: "accepted", launched: true, status: { state: "error" }, reported: true, rating: RANK_OF["u-3"] },
        { user: opponent, member_name: "Pablo", team: "bravo", role: "join", accept: "accepted", launched: true, status: { state: "error" }, reported: true, rating: RANK_OF["u-2"] },
        { user: noa, member_name: "Noa", team: "bravo", role: "join", accept: "accepted", launched: true, status: { state: "error" }, reported: true },
      ]
    : participants("error").map((p) => ({ ...p, reported: true }));
  // Who left: the opponent (won), the player (lost), a player of the other team (team), a team-mate (mate)
  const leaver = S.fft === "lost" ? "u-me" : S.fft === "mate" ? "u-3" : S.fft === "team" ? "u-4" : "u-2";
  const lost = S.fft === "lost" || S.fft === "mate";
  return {
    id: "m-ff", state: "completed", origin: { kind: "queue", queue_id: team ? "q-4v4" : "q-1v1" }, deadlines: {},
    rules: { teams: team ? [2, 2] : [1, 1], mode: "turf", rated: true, ladder: "1v1", stage: "Vss_LanternHarbor", result_policy: "ranked" },
    participants: parts, you: { user_id: "u-me", member_name: "Mira", team: "alpha", role: "host" },
    outcome: {
      decision: "forfeit", counted: true, winner: lost ? "bravo" : "alpha", rated: true, unrated_reason: null, record: null,
      summary: { en: "A team lost by forfeit: one of its players left the match while it was being played (their own launcher reported it).", fr: "Une équipe perd par forfait : un de ses joueurs a quitté le match en cours (son propre lanceur l'a signalé)." },
      forfeit: { reason: "left", team: lost ? "alpha" : "bravo", players: [{ user_id: leaver }] },
      reports: { expected: parts.length, results: 0, agreeing: 0, errors: parts.length, invalid: 0 },
      players: parts.map((p) => ({ user_id: p.user.id, verdict: "error" })),
      rating_changes: [{ user_id: "u-me", ladder: "1v1", delta: lost ? -12 : 12, after: { display: lost ? "1 188" : "1 212" } }], rematch_id: null,
    },
  };
}
const queues = [
  { id: "q-1v1", order: 1, name: L("Duel classé", "Ranked duel", "Duelo clasificatorio", "ランク 1v1"), description: L("Un contre un, mode et stage tirés au sort.", "One versus one, random mode and stage.", "Uno contra uno, modo y escenario al azar.", "1対1、ルールとステージはランダム。"), teams: [1, 1], rated: true, selection: "random", pool: [["turf", "Vss_Docks00"], ["turf", "Vss_Park"], ["zones", "Vss_Market01"], ["zones", "Vss_GlassTower"], ["turf", "Vss_LiftBridge00"], ["zones", "Vss_Quarry"]].map(([mode, stage]) => ({ mode, stage })), availability: { open: true }, estimated_wait_s: 45, searching: S.ticket ? 1 : 3, eligible: true, result_policy: "agree", requirements: [], draft: { id: "veto", label: L("Veto des stages", "Stage veto", "Veto de escenarios", "ステージ拒否"), summary: L("Chaque équipe bannit un stage, puis un stage est tiré au sort parmi les autres.", "Each team bans one stage, then a stage is drawn among the rest.", "Cada equipo veta un escenario y luego se sortea uno entre los demás.", "各チームが1つずつステージをバンし、残りから抽選します。"), required: false, turn_s: 20, pool: { stages: [], modes: [] } } },
  { id: "q-4v4", order: 2, name: L("Équipe 4 contre 4", "Team 4 vs 4", "Equipos 4 contra 4", "チーム 4v4"), description: L("Matchs d'équipe non classés.", "Unranked team matches.", "Partidas de equipo sin clasificar.", "ランクなしのチーム戦。"), teams: [4, 4], min_teams: [2, 2], rated: false, selection: "rotation", pool: [{ mode: "turf", stage: null }], rotation: [{ starts_at: new Date(Date.now() - 1800_000).toISOString(), ends_at: new Date(Date.now() + 5400_000).toISOString(), pool: [{ mode: "turf", stage: "Vss_Garden02" }, { mode: "turf", stage: "Vss_SkatePark" }] }, { starts_at: new Date(Date.now() + 5400_000).toISOString(), ends_at: new Date(Date.now() + 12600_000).toISOString(), pool: [{ mode: "turf", stage: "Vss_MistStation" }, { mode: "turf", stage: "Vss_SaltDunes03" }] }], availability: { open: true }, estimated_wait_s: 180, searching: 5, eligible: true, party: { min: 1, max: 4 }, requirements: [] },
  { id: "q-cup", order: 3, name: L("Coupe du dimanche", "Sunday cup", "Copa del domingo", "日曜カップ"), teams: [2, 2], rated: true, selection: "fixed", pool: [{ mode: "tower", stage: "Vss_Lab00" }, { mode: "tower", stage: "Vss_Mystery02" }], availability: { open: false, windows: [{ starts_at: "2026-11-15T18:00:00Z" }] }, eligible: false, requirements: [{ id: "games", met: false, label: L("10 parties jouées", "10 games played", "10 partidas jugadas", "10試合プレイ") }] },
];
function state(): any {
  return {
    me, room: room(), room_invites: [], party: null, party_invites: [], match: liveMatch(),
    ticket: S.ticket ? { id: "t-1", queue_id: "q-1v1", state: "searching", created_at: new Date(Date.now() - 95_000).toISOString(), estimated_wait_s: 45, reason: null } : null,
  };
}
/** `&relays=1`: the measurement as `proto/relayping.rs` reports it, pending for 3 s after the first overview (then
 *  pushed as `proto-relays`, as the backend does when it ends). */
let relaysDone = false;
let relaysTimer = false;
function relaysView(): any[] {
  if (!q.get("relays")) return [];
  if (!relaysTimer) {
    relaysTimer = true;
    setTimeout(() => {
      relaysDone = true;
      emit("proto-relays", { server: serversView().servers.find((x: any) => x.selected)?.id ?? "srv-1", relays: relaysView() });
    }, 3000);
  }
  return [
    { id: "eu", state: "ok", ms: 24 },
    relaysDone ? { id: "us", state: "ok", ms: 118 } : { id: "us", state: "pending", ms: null },
    relaysDone ? { id: "asia", state: "silent", ms: null } : { id: "asia", state: "pending", ms: null },
    { id: "lan", state: "refused", ms: null },
    { id: "sa", state: "unmeasured", ms: null },
  ];
}
function overview(): any {
  const s = serversView().servers.find((x: any) => x.selected);
  return {
    server: { id: s?.id ?? "srv-1", name: s?.name ?? "Open Ink", api: s?.api ?? "", relay: "relay.openink.example.org:47313" },
    meta, check: { compat: { ok: true, reason: null, version: "1.1.3" }, features: FEATURES, relay: "relay.openink.example.org:47313", name: "Open Ink", time: new Date().toISOString(), display_name_max: 10, display_name_min: 2 },
    meta_error: null, session: { logged_in: S.login !== "out", provider: "direct", user_id: "u-me", display_name: "Mira" },
    stream: { state: "open", resumed: false, retry_in_s: null, reason: null, last_seq: 12 }, runner: runner(), offset_s: 0, relays: relaysView(),
  };
}
function request(method: string, path: string, body?: any): any {
  if (method === "POST" && /\/v1\/matches\/[^/]+\/draft$/.test(path) && draftNow) {
    const r = mockDraftAct(S.ds ?? "you", body);
    S.ds = r.ds;
    draftNow = r.match;
    return draftNow;
  }
  const p = path.split("?")[0];
  if (method === "GET") {
    if (p === "/v1/state") return state();
    if (p === "/v1/spectate") return watchable(S.spect);
    if (p === "/v1/matches/m-1" && draftNow) return draftNow;
    if (p === "/v1/queues") return { items: queues };
    if (p === "/v1/friends") return { items: [{ user: opponent, presence: "online", since: "2026-10-20T18:00:00Z" }, { user: user("u-3", "Sacha"), presence: "in_match", since: "2026-10-22T18:00:00Z" }] };
    if (p === "/v1/blocks") return { items: [{ user: user("u-9", "Mako"), since: "2026-11-01T10:00:00Z" }] };
    if (p === "/v1/friends/requests") return { incoming: [{ id: "fr-1", user: user("u-4", "Noor") }], outgoing: [] };
    if (p === "/v1/ladders") return { items: [{ id: "1v1", name: L("Duel", "Duel", "Duelo", "デュエル"), primary: true, season: { id: "s1", name: L("Saison 1", "Season 1", "Temporada 1", "シーズン1") } }, { id: "4v4", name: L("Équipe", "Team", "Equipo", "チーム") }] };
    if (/\/ratings$/.test(p)) return { items: [{ ladder: "1v1", display: "1 214", tier: { id: "gold", name: L("Or", "Gold", "Oro", "ゴールド"), color: "#e0b43b" }, progress: { value: 64, max: 100, label: L("64 / 100 vers Platine", "64 / 100 to Platinum", "64 / 100 hacia Platino", "プラチナまで 64 / 100") }, last_change: { delta: 14 } }] };
    if (/\/leaderboard$/.test(p)) return { columns: [{ key: "rank", type: "rank", label: L("#", "#", "#", "#") }, { key: "player", type: "player", label: L("Joueur", "Player", "Jugador", "プレイヤー") }, { key: "tier", type: "tier", label: L("Rang", "Tier", "Rango", "ランク") }, { key: "rating", type: "number", label: L("Cote", "Rating", "Puntuación", "レート") }], items: [["1", "Sacha", 1544], ["2", "Noor", 1490], ["3", "Mira", 1214], ["4", "Pablo", 1180]].map(([r, n, v], i) => ({ user_id: n === "Mira" ? "u-me" : `x${i}`, cells: { rank: r, player: { display_name: n }, tier: { id: "gold", name: L("Or", "Gold", "Oro", "ゴールド"), color: "#e0b43b" }, rating: v } })) };
    if (/\/stats$/.test(p)) return { groups: [{ key: "all", label: L("Toutes les parties", "All matches", "Todas las partidas", "全試合"), fields: [{ key: "w", label: L("Victoires", "Wins", "Victorias", "勝利"), value: 23 }, { key: "l", label: L("Défaites", "Losses", "Derrotas", "敗北"), value: 17 }, { key: "k", label: L("Éliminations", "Splats", "Eliminaciones", "キル"), value: 214 }] }] };
    if (/\/matches$/.test(p)) return { items: [
      // 1.1.11: two matches decided by forfeit (no record: no statistics), then the usual ones
      ...(S.match === "forfeit" ? [
        { match_id: "m-ff", ended_at: "2026-11-13T01:10:00Z", rules: { teams: [1, 1], mode: "turf", rated: true }, team: "alpha", result: "win", stats: null, rating_delta: 12, forfeit: { reason: "left", team: "bravo", players: [{ user_id: "u-2" }] } },
        { match_id: "m-fl", ended_at: "2026-11-12T22:40:00Z", rules: { teams: [1, 1], mode: "turf", rated: true }, team: "alpha", result: "loss", stats: null, rating_delta: -12, forfeit: { reason: "left", team: "alpha", players: [{ user_id: "u-me" }] } },
      ] : []),
      { match_id: "m-0", ended_at: "2026-11-11T13:40:00Z", rules: { mode: "turf", rated: true }, result: "win", stats: { paint: 1043, kills: 6, deaths: 3 }, rating_delta: 14 }, { match_id: "m-x", ended_at: "2026-11-10T21:10:00Z", rules: { mode: "zones", rated: true }, result: "loss", stats: { paint: 702, kills: 2, deaths: 5 }, rating_delta: -11 }] };
    if (p === "/v1/activities") return { items: [{ id: "cup", order: 1, title: L("Coupe du dimanche", "Sunday cup", "Copa del domingo", "日曜カップ"), subtitle: L("Élimination directe en 2 contre 2.", "Single elimination, 2 vs 2.", "Eliminación directa, 2 contra 2.", "2対2のトーナメント。"), kind: L("Tournoi", "Cup", "Torneo", "大会"), icon: "trophy", color: "#ffd631", state: "upcoming", starts_at: "2026-11-15T18:00:00Z", participants: { count: 11, capacity: 16 }, join: { action: "join", label: L("S'inscrire", "Sign up", "Inscribirse", "参加登録") } }] };
    if (/\/badges$/.test(p)) return { items: [], featured_badge: null };
    if (p === "/v1/titles") return { items: [] };
    if (/\/profile$/.test(p)) return { sections: [{ type: "text", style: "heading", text: L("Mira", "Mira", "Mira", "Mira") }, { type: "fields", items: [{ label: L("Parties", "Matches", "Partidas", "試合"), value: "40" }, { label: L("Cote", "Rating", "Puntuación", "レート"), value: "1 214" }] }] };
    if (/\/v1\/activities\//.test(p)) return { title: L("Coupe du dimanche", "Sunday cup", "Copa del domingo", "日曜カップ"), state: "upcoming", revision: 1, sections: [{ type: "text", text: L("16 places, élimination directe.", "16 seats, single elimination.", "16 plazas, eliminación directa.", "16枠、トーナメント。") }] };
    return {};
  }
  if (method === "POST" && p === "/v1/rooms") { S.room = "host"; return room(); }
  if (method === "POST" && /\/v1\/rooms\/[^/]+\/start$/.test(p) && S.startfail === "relay_capacity") throw RELAY_CAPACITY;
  // 1.1.7: into the stands (refused when an older launcher plays), a place from the list.
  if (method === "PUT" && /\/v1\/rooms\/[^/]+\/members\//.test(p)) {
    if (S.spect === "old" && body?.team === "spectator") throw OLD_REFUSAL;
    moved = body?.team === "spectator";
    if (!moved && S.spect === "sit") S.spect = "room-back";
    return room() ?? {};
  }
  if (method === "POST" && /\/spectate$/.test(p)) { S.match = "found"; S.spect = "watch"; return liveMatch(); }
  if (method === "POST" && /\/tickets$/.test(p)) { S.ticket = true; return state().ticket; }
  return {};
}

// ---- private match by code (relay-only server) ------------------------------------------------------------------
const PM_CODE = "KLM1-7HQ2M-4XRTP-9KD3W-QZ8NB";
const pmPlan = (round = 0): any => ({ code: PM_CODE, spec: { relay_ip: "203.0.113.20", relay_port: 47313, room: "ROOM7H", round, host: "Mira", players: 2, mode: "turf" }, match_id: "pm-7h2q", relay: "203.0.113.20", rules: "teams=1,1", verified: true, own_code: false, relay_resolved: null, relay_known: q.get("relay") !== "unknown" });
const pmRun = () => ({ match_id: "pm-7h2q", role: "host", code: PM_CODE, started_at: MATCH_START, status_path: "", result_path: "", log_path: `${ROOT}\\userdata\\logs\\match.log`, host: "Mira", players: 2, mode: "turf" });
function pmPoll(): any {
  const st = S.pm === "fail" ? { state: "error", error: engineCode(), reason: ENGINE_REASON[engineCode()] ?? "relay unreachable", detail: "net: relay hello timed out" } : { state: S.gs, error: null, reason: null, detail: null };
  return { run: pmRun(), game: game, status: { match_id: "pm-7h2q", role: "host", since_unix: now() - 20, present: 1, expected: 2, visible: false, pid: 18440, source: "file", ...st }, result_ready: S.pm === "results", elapsed_s: 75 };
}
const pmResult = () => ({
  match_id: "pm-7h2q", role: "host", error: null, reason: null, mode: "turf", stage_scene: "Vss_Docks00", end_utc: now() - 10, duration_s: 183, win_team: 0, winner: "Alpha",
  teams: [{ team: "Alpha", paint_points: 1043, turf_permille: 562 }, { team: "Bravo", paint_points: 811, turf_permille: 391 }],
  players: [{ name: "Mira", self: true, team: "Alpha", paint: 1043, kills: 6, assists: 2, deaths: 3, specials: 2, disconnected: false }, { name: "Pablo", self: false, team: "Bravo", paint: 811, kills: 3, assists: 1, deaths: 6, specials: 1, disconnected: false }],
  outcome: "victory", my_team: "Alpha",
});

// ---- the launcher's own update ------------------------------------------------------------------------------------
// The notes are the signed manifest's plain text: markup and links in it must show as typed, never interpreted.
const LUP_NOTES = [
  "Kalamar 0.2.0",
  "",
  "- The launcher updates itself from its published (signed) releases.",
  "- Stage cards show up faster.",
  "- <b>This is not HTML</b>: the text is shown as it is.",
  "- Details: https://github.com/owner/kalamar/issues/12 (a link is never clickable here)",
].join("\n");
const lupKind = S.lup;
if (q.get("dlg") === "0") markDialogDone();
const lup: any = (() => {
  const k = lupKind;
  const on = k !== null && k !== "off";
  const offered = (on && !["uptodate", "never", "error", "untrusted", "rate", "updated", "rolledback"].includes(k!)) || k === "rolledback";
  const mode = k === "installer" ? "installer" : k === "elsewhere" || k === "readonly" || k === "saved" ? "elsewhere" : "swap";
  return {
    enabled: on, current: "0.1.0", auto_check: true, channel: k === "beta" ? "beta" : "stable",
    last_check: k === "never" ? null : now() - 3 * 3600, checking: false,
    check_error: k === "error" ? "network" : k === "untrusted" ? "untrusted" : k === "rate" ? "rate_limited" : null,
    available: offered ? { version: k === "beta" ? "0.3.0-beta.1" : "0.2.0", channel: k === "beta" ? "beta" : "stable", notes: LUP_NOTES, published: "2026-11-12T10:00:00Z", size: mode === "installer" ? 39_800_000 : 41_900_000, mode, why: k === "readonly" ? "readonly" : mode === "elsewhere" ? "temp" : null } : null,
    later: k === "later",
    phase: k === "downloading" ? "downloading" : ["ready", "blocked", "relay", "installer", "elsewhere", "readonly", "rolledback"].includes(k!) ? "ready" : k === "saved" ? "saved" : k === "restarting" ? "restarting" : "idle",
    progress: k === "downloading" ? { done: 17_600_000, total: 41_900_000 } : null,
    error: k === "integrity" ? "integrity" : null,
    blocked: k === "blocked" ? "game" : k === "relay" ? "relay" : null,
    saved_to: k === "saved" ? "D:\\Games\\Kalamar\\Kalamar.exe" : null,
    notice: k === "updated" ? { kind: "updated", from: "0.1.0", to: "0.2.0" } : k === "rolledback" ? { kind: "rolled_back", from: "0.1.0", to: "0.2.0" } : null,
  };
})();
const lupView = () => JSON.parse(JSON.stringify(lup));
function lupDownload() {
  if (!lup.available) return lupView();
  lup.phase = "downloading";
  lup.error = null;
  lup.progress = { done: 0, total: lup.available.size };
  const id = window.setInterval(() => {
    lup.progress.done = Math.min(lup.progress.total, lup.progress.done + 6_000_000);
    if (lup.progress.done >= lup.progress.total) {
      window.clearInterval(id);
      lup.phase = "ready";
      lup.progress = null;
    }
    emit("update-state", lupView());
  }, 250);
  return lupView();
}

// ---- Discord presence (&dc=) ---------------------------------------------------------------------------------------
// absent (default, like a development build): no Discord application compiled in, no section; off: available, off,
// Discord open; closed: off, Discord not open; on: on and connected (activity level); minimal | full (mode, stage and
// server): on at that level; waiting: on, Discord not open; busy: on, another Kalamar shows it; refused: on, Discord
// refused the application.
const DC = {
  available: !!S.dc && S.dc !== "absent",
  detected: !["closed", "waiting"].includes(S.dc ?? ""),
  link: (["on", "minimal", "full"].includes(S.dc ?? "") ? "connected" : S.dc === "waiting" ? "waiting" : S.dc === "busy" ? "busy" : S.dc === "refused" ? "refused" : "off") as string,
  sent: ["on", "minimal", "full"].includes(S.dc ?? "") ? 3 : 0,
  published: null as any,
};
config.discord_available = DC.available;
config.discord = {
  enabled: DC.link !== "off",
  detail: S.dc === "minimal" ? "minimal" : "activity",
  mode_stage: S.dc === "full",
  server: S.dc === "full",
};
const dcView = () => ({
  available: DC.available, prefs: { ...config.discord }, detected: DC.detected,
  status: { link: DC.link, code: DC.link === "refused" ? 4000 : null, sent: DC.sent },
});

// ---- the dispatcher -----------------------------------------------------------------------------------------------
// The back end fills the members a field leaves out (settings.rs); the file itself only has what differs.
const schema = { ...(schemaJson as any), fields: (schemaJson as any).fields.map((f: any) => ({ options: [], presets: [], min: null, max: null, step: null, slider: false, default: null, help: {}, ...f })) };
// During an installation (&install=) nothing is set yet, unless &tasks= says what was set during the wait.
const settingsValues: Record<string, string> = S.tasks === "some" ? { mouse_gyro_cm360: "34.5", mouse_gyro_dpi: "1600" }
  : S.install !== "none" && S.tasks !== "all" ? {}
  : { mouse_gyro_sens: "0.035", mouse_gyro_dpi: "1600", fullscreen: "1", fps_limit: "60", bind_zr: "mouse_l", bind_b: "key:space" };
if (S.gl) settingsValues.lang = S.gl;

export async function handle(cmd: string, args: any): Promise<any> {
  await new Promise((r) => setTimeout(r, 30));
  switch (cmd) {
    case "boot":
      return { info: { name: "Kalamar", version: "0.1.0", data_dir: "C:\\Users\\user\\AppData\\Roaming\\dev.kalamar.launcher" }, config: view(), status, game, game2: idle, last_run: game.state === "exited" ? game : null, install_active: ["running", "failed", "done"].includes(S.install), migration: null, servers: serversView(), profile, cli: { play: null, minimized: false, invalid: [] } };
    case "app_info": return { name: "Kalamar", version: "0.1.0", data_dir: "C:\\Users\\user\\AppData\\Roaming\\dev.kalamar.launcher" };
    case "get_config": return view();
    case "set_language": config.language = args.language; return view();
    case "set_game_lang_follow": config.game_lang_follow = args.follow; return view();
    case "game_language_info": return { fonts_ok: S.glfonts, fonts_dir: S.glfonts ? `${ROOT}\build\shared-fonts` : null, like_launcher: ({ fr: "fr", en: "en-US", es: "es", ja: "ja" } as Record<string, string>)[config.language] ?? "en-US" };
    case "set_ui_prefs": config.ui = { ...args.ui }; return view();
    case "set_launch_options": Object.assign(config, args.options); return view();
    case "set_lan_enabled": config.lan_enabled = args.enabled; return view();
    case "set_lan_prefs": config.lan = { ...args.request }; return view();
    case "set_second_player": config.second_name = args.name; config.second_dir = args.dir; return view();
    case "set_root": config.root = args.path; return view();
    case "set_translator": config.translator = args.path; return view();
    case "set_relay_exe": config.relay_exe = args.path; return view();
    case "install_status": return status;
    case "last_run": return game.state === "exited" ? game : null;
    case "game_status": return game;
    case "game2_status": return idle;
    case "play_preflight": return preflight();
    case "launch_report": return launchReport();
    case "backups_info": return preflight().backups;
    case "launch_mode":
      if (S.launchfail === "incomplete") throw fail("profile_incomplete", "", ["hair", "legwear"]);
      if (S.launchfail === "error") throw fail("io", "D:\\Games\\Kalamar\\work\\splatoon3-11.2.0\\build\\splat3.exe: Access is denied. (os error 5)");
      game = { ...idle, state: "running", pid: 20112, mode: args.request.mode, started_at: now(), log_path: `${ROOT}\\userdata\\logs\\run-now.log` };
      setTimeout(() => emit("game-status", game), 10);
      return game;
    case "game_stop":
      game = { ...game, state: "exited", ended_at: now(), exit_code: 1, stopped_by_user: true };
      setTimeout(() => emit("game-status", game), 10);
      return null;
    case "game_focus": return true;
    case "open_logs_folder": return status.logs_dir;
    case "read_log_tail": return LOG_TAIL;
    case "settings_schema": return schema;
    case "settings_load": return { path: `${ROOT}\\userdata\\settings.ini`, exists: true, values: { ...settingsValues }, unknown_keys: [] };
    case "settings_save":
      for (const c of args.changes) { if (c.value === null) delete settingsValues[c.key]; else settingsValues[c.key] = c.value; }
      return { path: `${ROOT}\\userdata\\settings.ini`, exists: true, values: { ...settingsValues }, unknown_keys: [] };
    // Before a running install's merge is complete the Locker has only the neutral list.
    case "profile_catalogue": return snap.active && !snap.game_data ? { ...catalogue(), origin: "neutral", reason: "the game data is not merged yet" } : catalogue();
    case "gamedata_stages": return stagesView(S.stages === "none");
    // ?gear=old: an engine from before both; ?gear=sub: one that applies sub-abilities but not the main ability; ?gear=none: no game
    case "gear_abilities_info": {
      const off = { supported: false, update_needed: S.gear === "old", abilities: [] };
      const subs = S.gear === "old" || S.gear === "none" ? off : { supported: true, update_needed: false, abilities: MOCK_SUB_ABILITIES };
      const main = S.gear === "old" || S.gear === "sub" ? { main_supported: false, main_update_needed: true, main_abilities: [] }
        : S.gear === "none" ? { main_supported: false, main_update_needed: false, main_abilities: [] }
        : { main_supported: true, main_update_needed: false, main_abilities: MOCK_MAIN_ABILITIES };
      return { ...subs, ...main };
    }
    case "profile_load": return profile;
    case "profile_save":
      if (S.wsave === "fail") throw { ...fail("validation", "invalid profile"), fields: [{ key: "weapon", message: "not in the game data" }] };
      profile = { ...args.profile };
      return profile;
    // Loadout presets: kept in memory, with the same rules as the backend (trimmed unique names, at most 30).
    case "loadouts_list": return presets;
    case "loadouts_save": {
      const name = String(args.name).trim();
      if (!name || [...name].length > 24) throw { ...fail("validation", "invalid name"), fields: [{ key: "name", message: "name must be 1 to 24 characters" }] };
      const i = presets.findIndex((p) => p.name.toLowerCase() === name.toLowerCase());
      if (i >= 0) presets[i] = { ...args.loadout, name };
      else if (presets.length >= 30) throw fail("invalid_input", "at most 30 presets can be saved");
      else presets.push({ ...args.loadout, name });
      return presets;
    }
    case "loadouts_rename": {
      const to = String(args.to).trim();
      const i = presets.findIndex((p) => p.name.toLowerCase() === String(args.from).toLowerCase());
      if (i < 0) throw fail("invalid_input", "no such preset");
      if (!to || [...to].length > 24 || presets.some((p, k) => k !== i && p.name.toLowerCase() === to.toLowerCase())) throw { ...fail("validation", "invalid name"), fields: [{ key: "name", message: "name is empty, too long or already used" }] };
      presets[i] = { ...presets[i], name: to };
      return presets;
    }
    case "loadouts_delete": presets = presets.filter((p) => p.name.toLowerCase() !== String(args.name).toLowerCase()); return presets;
    case "gamedata_icon_cache": return { files: 1832, bytes: 61_000_000, warming: false, dir: "C:\\Users\\user\\AppData\\Roaming\\dev.kalamar.launcher\\icons" };
    case "servers_view": return serversView();
    case "server_select": selectedId = args.id; return serversView();
    case "server_remove": case "server_add_line": case "server_card_import": return serversView();
    case "proto_peek": return peek();
    case "proto_population": return !S.pop ? null : args.server === "srv-1" ? { online: 12, in_match: 4, searching: 2, updated_at: new Date().toISOString() } : args.server === "srv-3" ? { online: 3, in_match: null, searching: null, updated_at: null } : null;
    case "server_resolve": return { host: "relay.openink.example.org", ip: "203.0.113.7" };
    case "migration_status": case "migration_last": return null;
    case "relay_status": return { running: false, pid: null, port: null, exe: null, log_path: "", tail: null, exited: null };
    case "lan_preview": return [["SPLAT3_LAN", args.request.mode === "relay" ? `relay:${args.request.relay_host || "?"}:${args.request.relay_port}` : "lan"], ["SPLAT3_NET_LOG", args.request.net_log ? "1" : "0"]];
    case "lan_generate_room": return "QX7-PLAZA-42";
    case "installer_info": endSoon(); return wizardInfo();
    case "installer_status": return snap;
    case "installer_update_check": if (S.ver) return { install_dir: DIR, available: true, installed_source: "0c924ab", bundled_commit: "4cb0304a1e9f", redo: ["lift", "compile", "keys"], update: null, eta: 780, eta_max: 780, at_close: false, game_from: "11.2.0", game_to: "11.3.0", disk_need: 14e9, old_bytes: 14.2e9 };
      return { game_from: null, game_to: "11.3.0", disk_need: 0, old_bytes: 0, install_dir: DIR, available: S.update, installed_source: "0c924ab", bundled_commit: "4cb0304a1e9f", redo: updatePreview.lift ? ["lift", "compile"] : ["compile"], update: S.update ? updatePreview : null, eta: offered.eta, eta_max: offered.eta_max, at_close: atClose };
    case "installer_at_close": atClose = !!args.request; return atClose;
    case "notify_attention": return false;
    case "installer_check_file": return { status: "ok", code: args.kind === "keys" ? "keys_ok" : args.kind === "base" ? "base_ok" : "update_ok", params: [["count", "142"]], size: 6e9, program_size: 1e8 };
    case "installer_check_dir": return { kind: "new", free: 214e9, need: 65e9, absolute: true, needs_files: true };
    case "installer_plan": return plan();
    case "installer_start": return startInstall();
    case "installer_cancel": case "installer_close": case "installer_open_logs": return null;
    case "installer_uninstall": return { removed: [], kept: [], freed: 11.4e9 };
    case "installer_rollback": return { now: "0c924ab", previous: "4cb0304" };
    case "proto_connect": case "proto_overview": return overview();
    case "proto_login": S.login = "in"; return overview();
    case "proto_logout": S.login = "out"; return overview();
    case "proto_export": return "D:\\Files\\kalamar-export-openink.example.org-2026-11-14.json";
    case "proto_erase": S.login = "out"; return overview();
    case "proto_request": return request(args.method, args.path, args.body);
    case "proto_runner": return runner();
    case "proto_engine": return { listed: true, note: null, caps: { contract_versions: [1, 2], rounds: true }, names: { stages: S.stages === "none" ? {} : { Vss_Docks00: "Les Docks" }, modes: {} } };
    case "proto_game_check": return S.ver
      ? { level: "too_old", outdated: true, installed_commit: "4cb0304", bundled_commit: "4cb0304", note: null, old_version: "11.2.0", needed_version: "11.3.0" }
      : { level: "ok", outdated: false, installed_commit: "4cb0304", bundled_commit: "4cb0304", note: null, old_version: null, needed_version: "11.3.0" };
    case "proto_discover": return { kind: "service", preview: { name: "Open Ink", description: meta.description, api: "https://openink.example.org/api", api_host: "openink.example.org", typed_host: null, relay: "relay.openink.example.org:47313", operator_name: "Open Ink Association", operator_contact: null, region: "EU", languages: ["fr", "en"], rules_url: null, privacy_url: null, rules_summary: {}, features: FEATURES, features_mismatch: false, compat: { ok: true, reason: null, version: "1.1.3" }, insecure_loopback: false, providers: [] } };
    case "proto_set_background": case "proto_disconnect": case "proto_forget": case "proto_pick_loadout": case "proto_open_link": return null;
    case "proto_confirm_launch": S.match = args.start ? "run" : "ended"; return null;
    case "proto_weapon_ready": setTimeout(weaponDone, 10); return null;
    case "match_create": return pmPlan();
    case "match_parse": return pmPlan();
    case "match_next_round": return pmPlan(1);
    case "match_start": return pmRun();
    case "match_last": return pmRun();
    case "match_poll": return pmPoll();
    case "match_result": return pmResult();
    case "discord_view": return dcView();
    case "discord_set_prefs":
      if (!DC.available) return dcView();
      config.discord = { ...args.prefs };
      DC.link = config.discord.enabled ? (S.dc === "busy" ? "busy" : S.dc === "refused" ? "refused" : DC.detected ? "connected" : "waiting") : "off";
      setTimeout(() => emit("discord-status", dcView().status), 10);
      return dcView();
    case "discord_publish":
      // kept on the window too, to look at what the page hands over (`__dcPublished` in the browser's console)
      DC.published = args.activity;
      (globalThis as any).__dcPublished = [...((globalThis as any).__dcPublished ?? []), args.activity];
      if (DC.link === "connected") DC.sent++;
      return null;
    case "update_status": case "update_confirm_start": return lupView();
    case "update_check":
      if (!lup.enabled) return lupView();
      lup.checking = true;
      emit("update-state", lupView());
      await new Promise((r) => setTimeout(r, 700));
      lup.checking = false;
      lup.last_check = now();
      return lupView();
    case "update_download": return lupDownload();
    case "update_cancel": lup.phase = "idle"; lup.progress = null; return lupView();
    case "update_later": lup.later = true; return lupView();
    case "update_dismiss": lup.notice = null; lup.error = null; if (lup.phase === "saved") lup.phase = "idle"; return lupView();
    case "update_apply":
      if (lup.blocked) { lup.error = `blocked_${lup.blocked}`; return lupView(); }
      lup.phase = "restarting";
      return lupView();
    case "update_save_to": lup.phase = "saved"; lup.saved_to = `${args.dir}\\Kalamar.exe`; return lupView();
    case "update_show_saved": return null;
    case "update_set_prefs": config.update = { ...args.prefs }; lup.auto_check = args.prefs.auto_check; lup.channel = args.prefs.channel; emit("update-state", lupView()); return view();
    default:
      console.warn(`[mock] no answer for ${cmd}`, args);
      return null;
  }
}

/** For the overlay of a match: the ended match the community page shows after a game. */
export const mockEnded = endedMatch;
