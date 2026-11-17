// What to show of a participant's rating beside their name (protocol 1.1.14: `participants[].rating`). The server's data
// is untrusted and older servers omit it: nothing is shown when it is absent or malformed, and a value of the wrong type
// is never rendered.
/* eslint-disable @typescript-eslint/no-explicit-any */

export interface RankView {
  /** True while the player is still in placement: no position, tier nor rating is shown then. */
  placement: boolean;
  /** Games left in placement, when the server says. */
  remaining: number | null;
  /** The ladder position, 1 or more; null when the player is not listed. */
  position: number | null;
  /** "#5", or null. */
  positionText: string | null;
  /** The tier object as sent (validated by `Tier` when drawn), or null. */
  tier: any | null;
  /** The rating as text (`display`, else the value), or null. */
  rating: string | null;
}

const isObject = (v: unknown): v is Record<string, any> => typeof v === "object" && v !== null && !Array.isArray(v);
const isNum = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const unset = (v: unknown) => v === null || v === undefined;

/** The rank to show for a participant's `rating`, or null when there is nothing (absent, malformed, or empty). */
export function rankOf(rating: unknown): RankView | null {
  if (!isObject(rating)) return null;
  const { provisional, position, value, display, tier } = rating;
  if (!unset(provisional)) {
    // Still in placement: the other fields are null by contract and ignored.
    if (!isObject(provisional)) return null;
    const left = provisional.remaining;
    if (!unset(left) && !(isNum(left) && left >= 0)) return null;
    return { placement: true, remaining: isNum(left) ? Math.floor(left) : null, position: null, positionText: null, tier: null, rating: null };
  }
  if (!unset(position) && !(isNum(position) && Number.isInteger(position) && position >= 1)) return null;
  if (!unset(value) && !isNum(value)) return null;
  if (!unset(display) && typeof display !== "string") return null;
  if (!unset(tier) && !(isObject(tier) && (typeof tier.id === "string" || isObject(tier.name)))) return null;
  const shown = typeof display === "string" && display.trim() ? display.trim().slice(0, 16) : isNum(value) ? String(value) : null;
  const view: RankView = {
    placement: false,
    remaining: null,
    position: isNum(position) ? position : null,
    positionText: isNum(position) ? `#${position}` : null,
    tier: isObject(tier) ? tier : null,
    rating: shown,
  };
  return view.positionText || view.tier || view.rating ? view : null;
}
