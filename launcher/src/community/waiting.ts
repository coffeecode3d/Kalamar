// What the "Your match" block says while the game on this machine goes through its steps: pure functions (no
// imports, so that `npm run test:ui` runs them with Node alone). The texts live in `texts.ts` (`m.wait.*`).

/** The engine's status as the backend reduces it (`proto/progress.rs`). */
export interface GameProgress {
  state: string; detail: string | null; since_ms: number | null; present: number | null; expected: number | null;
  visible: boolean | null; limit_s: number | null;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type J = any;

/** A player whose game stays at "launched" this long (s) while the host waits is probably not coming. */
export const SLOW_AFTER_S = 90;

/** The states in which the player's own window is still hidden (it appears at the stage's intro). */
const HIDDEN_STATES = ["booting", "lobby", "creating-room", "waiting-for-players", "joining", "in-room", "loading-stage"];

/** Game states of another player that show the game is in the session already. */
const IN_SESSION = ["in-room", "loading-stage", "in-match", "results", "finished", "waiting-command"];

const KNOWN = ["booting", "lobby", "creating-room", "waiting-for-players", "joining", "in-room", "loading-stage", "in-match", "results", "finished", "error", "waiting-command"];

/** The key of the sentence for a game state (`m.wait.<state>`); the room differs for the host and the guest. */
export function stateKey(state: string | null | undefined, role: string | null | undefined): string | null {
  if (!state || !KNOWN.includes(state)) return null;
  return state === "in-room" ? `m.wait.in-room.${role === "host" ? "host" : "join"}` : `m.wait.${state}`;
}

/** Seconds in the current step and seconds before the engine gives up on it (`null`: not known). */
export function stepTimes(g: GameProgress | null | undefined, nowMs: number): { elapsed: number | null; left: number | null } {
  if (!g || g.since_ms === null || !Number.isFinite(g.since_ms)) return { elapsed: null, left: null };
  const elapsed = Math.max(0, Math.floor((nowMs - g.since_ms) / 1000));
  return { elapsed, left: g.limit_s !== null && g.limit_s > 0 ? Math.max(0, g.limit_s - elapsed) : null };
}

function nameOf(p: J): string {
  return String(p?.member_name ?? p?.user?.display_name ?? "?").slice(0, 32);
}

/** Names of the other players whose game is not in the session yet. */
export function missingNames(participants: J[] | null | undefined, me: string | null): string[] {
  return (participants ?? []).filter((p) => p?.user?.id !== me && !IN_SESSION.includes(p?.status?.state)).map(nameOf);
}

/** The window is still hidden: the game has not reached the intro of the match. */
export function windowHidden(g: Pick<GameProgress, "state" | "visible"> | null | undefined, gameState: string | null | undefined): boolean {
  const s = g?.state ?? gameState;
  if (!s || !HIDDEN_STATES.includes(s)) return false;
  return g?.visible !== true;
}

/** When each other player was first seen at "launched" with no game state yet (key: user id), kept by the caller. */
export type SeenLaunched = Record<string, number>;

export function trackLaunched(prev: SeenLaunched, participants: J[] | null | undefined, me: string | null, nowMs: number): SeenLaunched {
  const next: SeenLaunched = {};
  for (const p of participants ?? []) {
    const id = p?.user?.id;
    if (typeof id !== "string" || id === me) continue;
    if (p.launched && !p.status?.state) next[id] = prev[id] ?? nowMs;
  }
  return next;
}

/** Names of the players that stay at "launched" for more than `afterS` seconds while this game waits for the players. */
export function slowPlayers(seen: SeenLaunched, participants: J[] | null | undefined, myState: string | null | undefined, nowMs: number, afterS = SLOW_AFTER_S): string[] {
  if (myState !== "waiting-for-players") return [];
  return (participants ?? []).filter((p) => typeof p?.user?.id === "string" && seen[p.user.id] !== undefined && nowMs - seen[p.user.id] >= afterS * 1000).map(nameOf);
}

/** The installed game's check, as `proto_game_check` answers. */
export interface GameCheck {
  level: "ok" | "too_old" | "unknown" | "missing"; outdated: boolean; installed_commit: string | null; bundled_commit: string | null; note: string | null;
  /** The installed game is for this other game version (`level` is then `too_old`): the game update to `needed_version` is needed. */
  old_version: string | null; needed_version: string;
}

/** The game cannot take part in an automatic match: the player may not declare ready, start or accept. */
export function gameBlocks(check: GameCheck | null | undefined): boolean {
  return check?.level === "too_old";
}

/** A recompilation is available and does not block anything. */
export function gameWarns(check: GameCheck | null | undefined): boolean {
  return !!check && check.level !== "too_old" && check.outdated;
}
