// The logic of the Locker's ability editor, kept free of React so a test can run it: the three sub slots of each worn
// piece, the edits (one slot, all three, clear) and the list the picker offers. The state is the profile's
// `sub_abilities`: per part null (never edited: the game keeps what the save has) or three slots, each an ability name
// of the player's game or null (empty). Clearing is an edit too: three nulls, which the game writes as three empty slots.
// The main slot is the profile's `main_abilities`: per part null (never chosen: the save keeps the piece's main
// ability) or an ability name; a main slot is never empty.
import type { Profile } from "../../api";

export type GearPart = "head" | "clothes" | "shoes";
export const GEAR_PARTS: GearPart[] = ["head", "clothes", "shoes"];
export const SUB_SLOTS = 3;
export type Slots = (string | null)[];

/** The three slots shown for a part: what was chosen, or three empty slots when nothing was ever chosen. */
export function slotsOf(p: Profile, part: GearPart): Slots {
  const s = p.sub_abilities?.[part];
  return Array.from({ length: SUB_SLOTS }, (_, i) => (Array.isArray(s) ? s[i] ?? null : null));
}

/** True when the player edited this part (even to empty). */
export function isEdited(p: Profile, part: GearPart): boolean {
  return Array.isArray(p.sub_abilities?.[part]);
}

export function withSlots(p: Profile, part: GearPart, slots: Slots): Profile {
  return { ...p, sub_abilities: { ...(p.sub_abilities ?? { head: null, clothes: null, shoes: null }), [part]: slots.slice(0, SUB_SLOTS) } };
}

/** One slot set to an ability (or emptied with null). */
export function setSlot(p: Profile, part: GearPart, index: number, ability: string | null): Profile {
  if (index < 0 || index >= SUB_SLOTS) return p;
  const slots = slotsOf(p, part);
  slots[index] = ability;
  return withSlots(p, part, slots);
}

/** The shortcut: the same ability in all three slots (null empties all three). */
export function fillAll(p: Profile, part: GearPart, ability: string | null): Profile {
  return withSlots(p, part, Array.from({ length: SUB_SLOTS }, () => ability));
}

/** "Clear": three empty slots, an explicit edit that takes earlier sub-abilities off the piece at the next launch. */
export function clearPart(p: Profile, part: GearPart): Profile {
  return fillAll(p, part, null);
}

export interface AbilityOption { id: string; name: string }

/** What the picker lists: the abilities a sub slot can hold, by localized name (the name is the enum name when the
 *  game's files give none). */
export function pickable(abilities: string[], nameOf: (id: string) => string): AbilityOption[] {
  return [...new Set(abilities)].map((id) => ({ id, name: nameOf(id) })).sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id));
}

// ---- the main slot ---------------------------------------------------------------------------------------------

/** The main ability the player chose for a part, or null when none was ever chosen. */
export function chosenMain(p: Profile, part: GearPart): string | null {
  return p.main_abilities?.[part] ?? null;
}

/** The main ability shown for a part: the chosen one, else the piece's own (`own`, from the catalogue). */
export function mainOf(p: Profile, part: GearPart, own: string | null): string | null {
  return chosenMain(p, part) ?? own;
}

/** One part's main ability. Choosing is always explicit, the piece's own included: that is what undoes a main ability an
 *  earlier launch wrote (null would leave the save as it is). */
export function setMain(p: Profile, part: GearPart, ability: string | null): Profile {
  return { ...p, main_abilities: { ...(p.main_abilities ?? { head: null, clothes: null, shoes: null }), [part]: ability } };
}

/** The main abilities a part can take: those of `main` (what a main slot can hold, in the game's enum order) that a
 *  selectable piece of that part has as its own main ability in the game, as the engine requires. */
export function mainChoices(main: string[], pieces: { ability: string | null; selectable: boolean }[]): string[] {
  const has = new Set(pieces.filter((e) => e.selectable && e.ability).map((e) => e.ability as string));
  return [...new Set(main)].filter((a) => has.has(a));
}
