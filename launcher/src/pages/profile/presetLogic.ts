// The logic of the Locker's loadout presets, kept free of React so a test can run it: taking the loadout part of a
// profile, telling whether a preset equals it, and applying a preset to a profile. A preset is a name plus `head`,
// `clothes`, `shoes`, `weapon`, the sub-abilities and the main abilities; appearance, name and splashtag are never part
// of it.
import type { Profile } from "../../api";

export type LoadoutField = "head" | "clothes" | "shoes" | "weapon";
export const LOADOUT_FIELDS: LoadoutField[] = ["head", "clothes", "shoes", "weapon"];
const PARTS = ["head", "clothes", "shoes"] as const;
type Part = (typeof PARTS)[number];
type Subs = Profile["sub_abilities"];
type Mains = Profile["main_abilities"];

export interface Loadout {
  head: number | null; clothes: number | null; shoes: number | null; weapon: number | null;
  sub_abilities: Subs;
  /** Absent in a preset saved before main abilities: none chosen. */
  main_abilities?: Mains;
}
export interface Preset extends Loadout { name: string }

export const MAX_PRESETS = 30;
export const PRESET_NAME_MAX = 24;

const noSubs = (): Subs => ({ head: null, clothes: null, shoes: null });
const noMains = (): Mains => ({ head: null, clothes: null, shoes: null });
const mainOf = (m: Mains | undefined | null, part: Part): string | null => m?.[part] ?? null;

/** Three slots for a part; "never edited" (null) and "three empty slots" read the same here. */
function slots(s: Subs | undefined | null, part: Part): (string | null)[] {
  const v = s?.[part];
  return [0, 1, 2].map((i) => (Array.isArray(v) ? v[i] ?? null : null));
}

/** The loadout part of a profile (what a preset stores). */
export function loadoutOf(p: Profile): Loadout {
  return {
    head: p.head ?? null, clothes: p.clothes ?? null, shoes: p.shoes ?? null, weapon: p.weapon ?? null,
    sub_abilities: { head: slots(p.sub_abilities, "head"), clothes: slots(p.sub_abilities, "clothes"), shoes: slots(p.sub_abilities, "shoes") },
    main_abilities: { head: mainOf(p.main_abilities, "head"), clothes: mainOf(p.main_abilities, "clothes"), shoes: mainOf(p.main_abilities, "shoes") },
  };
}

/** The same trimmed, case-insensitive comparison the backend uses for names. */
export const sameName = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

/** Whether the preset equals the profile's loadout. Sub-abilities and main abilities count only when the game supports
 *  them. */
export function matches(preset: Loadout, p: Profile, withSubs: boolean, withMain = false): boolean {
  if (LOADOUT_FIELDS.some((f) => (preset[f] ?? null) !== (p[f] ?? null))) return false;
  if (withMain && PARTS.some((part) => mainOf(preset.main_abilities, part) !== mainOf(p.main_abilities, part))) return false;
  if (!withSubs) return true;
  return PARTS.every((part) => JSON.stringify(slots(preset.sub_abilities, part)) === JSON.stringify(slots(p.sub_abilities, part)));
}

export interface ApplyOptions {
  /** Whether an id exists (and can be chosen) in the installed game's data. */
  has: (field: LoadoutField, id: number) => boolean;
  /** The installed game applies sub-abilities; otherwise they are left alone. */
  subs: boolean;
  /** The abilities a sub slot can hold; a name outside it is emptied. Empty or absent: not checked. */
  abilities?: string[];
  /** The installed game applies main abilities; otherwise they are left alone. */
  main?: boolean;
  /** Per part, the main abilities it can take; a name outside it is dropped (none chosen). Absent: not checked. */
  mainAbilities?: Partial<Record<Part, string[]>>;
}

/**
 * Copies the preset's loadout into the profile. An item the game data no longer has is skipped (the profile keeps what
 * it had for that field, and its sub-abilities) and counted in `skipped`.
 */
export function applyPreset(p: Profile, preset: Loadout, o: ApplyOptions): { profile: Profile; skipped: number } {
  let skipped = 0;
  const next: Profile = { ...p };
  const kept = new Set<LoadoutField>();
  for (const f of LOADOUT_FIELDS) {
    const id = preset[f] ?? null;
    if (id !== null && !o.has(f, id)) {
      skipped++;
      kept.add(f);
    } else {
      next[f] = id;
    }
  }
  if (o.subs) {
    const subs: Subs = { ...(p.sub_abilities ?? noSubs()) };
    for (const part of PARTS) {
      if (kept.has(part)) continue;
      const raw = preset.sub_abilities?.[part];
      subs[part] = Array.isArray(raw) ? slots(preset.sub_abilities, part).map((n) => (n && (!o.abilities?.length || o.abilities.includes(n)) ? n : null)) : null;
    }
    next.sub_abilities = subs;
  }
  if (o.main) {
    const mains: Mains = { ...(p.main_abilities ?? noMains()) };
    for (const part of PARTS) {
      if (kept.has(part)) continue;
      const name = mainOf(preset.main_abilities, part);
      const allowed = o.mainAbilities?.[part];
      mains[part] = name && (!allowed || allowed.includes(name)) ? name : null;
    }
    next.main_abilities = mains;
  }
  return { profile: next, skipped };
}
