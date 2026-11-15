// Typed wrappers around the protocol client's commands (src-tauri/src/proto/glue.rs) and its pushes. The page never
// sees a token: it gets the server's declarations and data as the server sent them, and renders them as data.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { GameCheck, GameProgress } from "./waiting";
import { cleanText } from "./cleanText";

/** Localised text of the protocol: language tag -> string. */
export type LText = Record<string, string>;
// The server's documents are rendered as data: their members are read defensively (`any` on purpose, unknown members
// are ignored).
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type J = any;

export interface Fail {
  kind: string; message: string; details: string[]; fields: { key: string; message: string }[];
  code: string; status: number; retry_after_s: number | null; data: J;
}

export function toFail(e: unknown): Fail {
  if (e && typeof e === "object" && "kind" in e) {
    const o = e as Partial<Fail>;
    return {
      kind: String(o.kind), message: String(o.message ?? ""), details: o.details ?? [], fields: o.fields ?? [],
      code: String(o.code ?? o.kind), status: o.status ?? 0, retry_after_s: o.retry_after_s ?? null, data: o.data ?? null,
    };
  }
  return { kind: "internal", message: String(e), details: [], fields: [], code: "internal", status: 0, retry_after_s: null, data: null };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return cleanText(await invoke<T>(cmd, args));
  } catch (e) {
    throw toFail(cleanText(e));
  }
}

export interface Compat { ok: boolean; reason: string | null; version: string | null }
export interface MetaCheck { compat: Compat; features: string[]; relay: string | null; name: string; time: string | null; display_name_max: number; display_name_min: number }
export interface Preview {
  name: string; description: LText; api: string; api_host: string; typed_host: string | null; relay: string | null;
  operator_name: string | null; operator_contact: string | null; region: string | null; languages: string[];
  rules_url: string | null; privacy_url: string | null; rules_summary: LText; features: string[]; features_mismatch: boolean;
  compat: Compat; insecure_loopback: boolean; providers: J[];
}
export type Discovered =
  | { kind: "service"; preview: Preview }
  | { kind: "relay_only"; line: string; relay: string | null; reason: string; problems: string[] }
  | { kind: "legacy" };

export interface SessionInfo { logged_in: boolean; provider: string | null; user_id: string | null; display_name: string | null }
export interface StreamInfo { state: "connecting" | "open" | "retrying" | "stopped"; resumed: boolean; retry_in_s: number | null; reason: string | null; last_seq: number }
export interface RunnerStatus {
  match_id: string; phase: "checking" | "confirm" | "resolving" | "choosing" | "closing" | "launching" | "continuing" | "running" | "uploading" | "done" | "waiting" | "refused" | "failed" | "closed" | "late";
  game_state: string | null; game: GameProgress | null; code: string | null; reason: string | null; relay_ip: string | null; role: string | null; member_name: string | null;
  env: [string, string][]; status_path: string | null; result_path: string | null; result_uploaded: boolean;
  /** `choosing`: slot -> constraint; the imposed loadout (slot -> id); the result's `loadout.ok`. */
  choices?: Record<string, { fixed?: string; allowed?: string[]; denied?: string[] }>; loadout?: Record<string, string>; loadout_ok?: boolean;
  /** 1.1.7: `spectator` when the player watches from the stands. */
  seat?: "player" | "spectator";
  /** `confirm`: why the launcher asks (`not-joined`, `too-many`), the relay the match names, the seconds left to answer. */
  ask?: "not-joined" | "too-many"; relay?: string; ask_s?: number;
}
export interface Overview {
  server: { id: string; name: string; api: string; relay: string | null };
  meta: J | null; check: MetaCheck | null; meta_error: Fail | null; session: SessionInfo; stream: StreamInfo | null;
  runner: RunnerStatus | null; offset_s: number;
  /** 1.1.9: the relays of `meta.relays` that are measured (one per address, 16 at most) and where each stands; empty
   *  with fewer than two relays. Updated by the `proto-relays` push. */
  relays?: RelayView[];
}

/** One measured relay (`proto/relayping.rs`, `RelayView`): `ok` with `ms`; `silent`: no answer (or a name that does not
 *  resolve); `refused`: an address the launcher does not measure; `unmeasured`: not yet. */
export interface RelayView { id: string; state: "pending" | "ok" | "silent" | "refused" | "unmeasured"; ms: number | null }

/** A saved server with a service as this computer knows it, without any request: the kept login (if any) and the
 *  description and areas of its last meta (empty until it was read once). */
export interface Peek { id: string; session: SessionInfo; description: LText; features: string[] }

/** `GET /v1/population` as the launcher checked it (`proto/population.rs`). */
export interface Population { online: number; in_match: number | null; searching: number | null; updated_at: string | null }

export const proto = {
  /** For the Play page: never contacts a server. */
  peek: () => call<Peek[]>("proto_peek"),
  /** `null` when the server does not declare `population`, or on any failure. */
  population: (server: string) => call<Population | null>("proto_population", { server }),
  discover: (line: string) => call<Discovered>("proto_discover", { line }),
  card: (path: string) => call<Discovered>("proto_card", { path }),
  add: (input: { name: string; api: string; relay: string | null }) => call<string>("proto_add", { input }),
  overview: (server: string, refresh = false) => call<Overview>("proto_overview", { server, refresh }),
  connect: (server: string) => call<Overview>("proto_connect", { server }),
  login: (server: string, provider: string, fields: Record<string, unknown>) => call<Overview>("proto_login", { server, provider, fields }),
  oauth: (server: string, provider: string) => call<Overview>("proto_oauth", { server, provider }),
  logout: (server: string, all = false) => call<Overview>("proto_logout", { server, all }),
  /** Saves the player's data in a file they choose (the system's dialog); null when they closed the dialog. */
  exportData: (server: string) => call<string | null>("proto_export", { server }),
  /** Erases the account on the server; `typedName` is the display name the player typed to confirm. */
  erase: (server: string, typedName: string) => call<Overview>("proto_erase", { server, typedName }),
  request: <T = J>(server: string, method: string, path: string, body?: unknown) => call<T>("proto_request", { server, method, path, body: body ?? null }),
  runner: (server: string) => call<RunnerStatus | null>("proto_runner", { server }),
  pickLoadout: (server: string, matchId: string, picks: Record<string, string>) => call<void>("proto_pick_loadout", { server, matchId, picks }),
  /** The player's answer to the launcher's question before a launch (runner phase `confirm`). */
  confirmLaunch: (server: string, matchId: string, start: boolean) => call<void>("proto_confirm_launch", { server, matchId, start }),
  disconnect: (server: string) => call<void>("proto_disconnect", { server }),
  forget: (server: string) => call<void>("proto_forget", { server }),
  /** Opens an https link in the system browser (on the player's click only). */
  openLink: (url: string) => call<void>("proto_open_link", { url }),
  setBackground: (server: string, on: boolean) => call<void>("proto_set_background", { server, on }),
  /** Is the installed game recent enough for the automatic match, and is it the embedded runtime's build? */
  gameCheck: () => call<GameCheck>("proto_game_check"),
  engine: () => call<{ listed: boolean; note: string | null; caps: J; names?: { stages?: Record<string, string>; modes?: Record<string, string> } }>("proto_engine"),
};

export interface Push { server: string; [k: string]: J }

export function onProto(channel: "proto-event" | "proto-stream" | "proto-session" | "proto-runner" | "proto-relays", cb: (p: Push) => void): Promise<UnlistenFn> {
  return listen<Push>(channel, (e) => cb(cleanText(e.payload)));
}
