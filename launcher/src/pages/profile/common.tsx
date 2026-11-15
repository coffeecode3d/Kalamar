// Shared bits of the customisation tabs.
import type { ReactNode } from "react";
import type { GameCatalogue, GameEntry, GearAbilitiesInfo, Profile } from "../../api";
import type { TFn } from "../../i18n";

export interface TabProps {
  cat: GameCatalogue;
  profile: Profile;
  setProfile: (f: (p: Profile) => Profile) => void;
  lang: string;
  t: TFn;
  /** True when the catalogue comes from the player's game data (names, icons, gear...). */
  game: boolean;
  /** What the runtime refused at the last launch, by profile field (`hair`, `head`, `weapon`...). */
  notes: Record<string, FieldNoteData>;
  /** Advanced mode: identifiers, sorting by id, the notes about how icons are matched. */
  advanced: boolean;
  /** Whether the installed game applies sub-abilities and which abilities a sub slot can hold (null: not known yet). */
  subs?: GearAbilitiesInfo | null;
}

/** `raw`: the runtime's own sentence, empty in the simple mode. */
export interface FieldNoteData { text: string; raw: string }

/** The runtime's refusal, translated, next to the field it is about (with its own sentence below, in advanced mode). */
export function FieldNote({ note, t }: { note?: FieldNoteData; t: TFn }) {
  if (!note) return null;
  return (
    <div className="field-note" role="alert">
      <strong>{t("refusal.lastlaunch")}</strong> {note.text}
      {note.raw && <div className="tiny muted">{t("play.report.verbatim")} <code>{note.raw}</code></div>}
    </div>
  );
}

export function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: { id: T; label: string }[]; onChange: (v: T) => void; label: string }) {
  return (
    <div className="seg" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          role="radio"
          aria-checked={o.id === value}
          className={`seg-btn ${o.id === value ? "seg-on" : ""}`}
          tabIndex={o.id === value ? 0 : -1}
          onClick={() => onChange(o.id)}
          onKeyDown={(e) => {
            const i = options.findIndex((x) => x.id === value);
            const n = e.key === "ArrowRight" || e.key === "ArrowDown" ? i + 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? i - 1 : null;
            if (n == null) return;
            e.preventDefault();
            onChange(options[(n + options.length) % options.length].id);
          }}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function FilterBar({ children }: { children: ReactNode }) {
  return <div className="filters row wrap">{children}</div>;
}

export function Select({ label, value, onChange, options }: { label: string; value: string; onChange: (v: string) => void; options: { id: string; label: string }[] }) {
  return (
    <label className="filter">
      <span className="filter-label">{label}</span>
      <select value={value} onChange={(e) => onChange(e.target.value)}>
        {options.map((o) => <option key={o.id} value={o.id}>{o.label}</option>)}
      </select>
    </label>
  );
}

export function SearchBox({ value, onChange, label }: { value: string; onChange: (v: string) => void; label: string }) {
  return (
    <label className="filter filter-search">
      <span className="filter-label">{label}</span>
      <input type="search" value={value} onChange={(e) => onChange(e.target.value)} placeholder={label} />
    </label>
  );
}

export function NeedsGameData({ t, reason }: { t: TFn; reason: string | null }) {
  return (
    <div className="card card-warn">
      <h2>{t("profile.nogame.title")}</h2>
      <p>{t("profile.nogame.body")}</p>
      {reason && <p className="hint">{reason}</p>}
    </div>
  );
}

export function sortEntries(list: GameEntry[], key: string, nameOf: (e: GameEntry) => string): GameEntry[] {
  const out = [...list];
  const cmp: Record<string, (a: GameEntry, b: GameEntry) => number> = {
    name: (a, b) => nameOf(a).localeCompare(nameOf(b)),
    rarity: (a, b) => (b.rarity ?? -1) - (a.rarity ?? -1) || nameOf(a).localeCompare(nameOf(b)),
    season: (a, b) => (a.season ?? -1) - (b.season ?? -1) || a.id - b.id,
    id: (a, b) => a.id - b.id,
    order: (a, b) => a.order - b.order || a.id - b.id,
  };
  return out.sort(cmp[key] ?? cmp.id);
}
