// The ranks of the browser preview's players (`participants[].rating`, protocol 1.1.14): a player #1, one at a two-digit
// position, one placed but not listed, one in placement; a player without `rating` is a participant of an older server.
/* eslint-disable @typescript-eslint/no-explicit-any */
const tier = (id: string, en: string, fr: string, color: string, order: number) => ({ id, name: { en, fr, es: en, ja: en }, color, order });
const placed = (t: any, value: number, position: number | null) => ({ ladder: "duel", value, display: String(value), tier: t, position, provisional: null });

export const RANKS = {
  first: placed(tier("diamond", "Diamond", "Diamant", "#6FD3F5", 5), 2105, 1),
  twelve: placed(tier("gold", "Gold", "Or", "#D4AF37", 3), 1423, 12),
  unlisted: placed(tier("silver", "Silver", "Argent", "#B8C0CC", 2), 1187, null),
  placement: { ladder: "duel", value: null, display: null, tier: null, position: null, provisional: { remaining: 2 } },
};

/** By user id: Mira #12, Pablo in placement, Sacha placed without a position, the fourth player #1. */
export const RANK_OF: Record<string, any> = { "u-me": RANKS.twelve, "u-2": RANKS.placement, "u-3": RANKS.unlisted, "u-4": RANKS.first };
