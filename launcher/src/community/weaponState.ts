// The window to change weapon before a community match's launch (runner phase `weapon`, `proto/runner.rs`) read as
// data: whether the panel is wanted, and its countdown against the server's clock. Pure functions, no React: the panel
// (WeaponWindow.tsx) draws what they say, the tests run them in Node.

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type J = any;

/** The panel is wanted: the runner waits in phase `weapon`, for a player (a spectator never plays a weapon). `match`:
 *  the current match, when known; seen from the stands, no panel either. */
export function weaponWanted(r: J, match: J): boolean {
  if (!r || r.phase !== "weapon" || r.seat === "spectator") return false;
  const same = !!match && match.id === r.match_id;
  return !(same && (match.you?.team === "spectator" || match.you?.seat === "spectator"));
}

/** Whole seconds left in the window (rounded up, never below 0) at `now` (ms, the server's clock); `null` when its end
 *  is unknown. */
export function windowLeft(until: unknown, now: number): number | null {
  const end = typeof until === "string" ? Date.parse(until) : NaN;
  if (!Number.isFinite(end)) return null;
  return Math.max(0, Math.ceil((end - now) / 1000));
}

/** What the ring shows of the window (1 when it opens, 0 at its end): `left` of `total` seconds. */
export function windowFraction(left: number, total: number): number {
  return total > 0 ? Math.max(0, Math.min(1, left / total)) : 0;
}
