// The logic of the Locker's sub-ability editor, kept free of React so a test can run it: the three slots of each worn
// piece, the edits (one slot, all three, clear) and the list the picker offers. The state is the profile's
// `sub_abilities`: per part null (never edited: the game keeps what the save has) or three slots, each an ability name
// of the player's game or null (empty). Clearing is an edit too: three nulls, which the game writes as three empty slots.
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
