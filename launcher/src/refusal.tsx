// The runtime's refusal sentences (docs/save-profile.md, runtime/save_profile.c) mapped to a translated message and to
// the tab that holds the offending field. The runtime's own sentence is always kept next to the translation.
import type { Profile, Refusal } from "./api";
import type { Key, TFn } from "./i18n";

export type ProfileTab = "character" | "gear" | "weapon" | "splashtag";

/** The profile field a refusal is about, as one of the keys of the notes the tabs show. */
export function refusalField(r: Refusal, p: Profile): string {
  if (r.field === "sub_abilities") {
    for (const slot of ["head", "clothes", "shoes"] as const) if (r.id != null && p[slot] === r.id) return `sub_abilities.${slot}`;
    return r.field;
  }
  if (r.field !== "gear") return r.field;
  // the runtime names the gear id, not the slot
  for (const slot of ["head", "clothes", "shoes"] as const) if (r.id != null && p[slot] === r.id) return slot;
  return "gear";
}

export function refusalTab(field: string): ProfileTab | null {
  switch (field) {
    case "style": case "skin": case "eyes": case "hair": case "eyebrows": case "legwear": case "legwear_variation": case "appearance":
      return "character";
    case "head": case "clothes": case "shoes": case "gear": case "sub_abilities":
    case "sub_abilities.head": case "sub_abilities.clothes": case "sub_abilities.shoes":
      return "gear";
    case "weapon":
      return "weapon";
    case "splashtag": case "badges":
      return "splashtag";
    default:
      return null;
  }
}

const SPECIES: Record<string, Key> = { inkling: "profile.species.inkling", octoling: "profile.species.octoling" };

export function refusalText(r: Refusal, t: TFn): string {
  const species = r.species ? t(SPECIES[r.species]) : "";
  const style = r.style ?? "?";
  const vars = { id: r.id ?? "?", species, style };
  const key = `refusal.${r.code}` as Key;
  const base = t(key, vars);
  // an unknown code falls back to the key itself: show the runtime's sentence instead
  const text = base === key ? t("refusal.other") : base;
  return r.from_save ? `${text} ${t("refusal.from_save")}` : text;
}

/** Field label for a refusal (shown before the message when it is not next to the field itself). */
export function fieldLabel(field: string, t: TFn): string {
  const k = `refusal.field.${field}` as Key;
  const s = t(k);
  return s === k ? field : s;
}
