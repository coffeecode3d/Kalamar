// Shared pieces of the community page: the server-declared form fields, names, countdowns, tiers.
import { useEffect, useState, type ReactNode } from "react";
import { DICTS, formatDateTime, useI18n, type Key } from "../i18n";
import { StageField, useStageInfo, useStagePictures } from "../stages";
import type { J } from "./api";
import { useC, duration, failText, type CFn } from "./texts";
import { getLive, myId, serverNow } from "./store";
import { ProtoIcon, safeColor } from "./icons";
import { toFail, type Fail } from "./api";
import { IconCrown } from "../icons";
import { rankOf } from "./rank";

export const KNOWN_TYPES = ["string", "integer", "boolean", "enum", "teams", "mode", "stage"];

/** A required field of a type this launcher does not know: the form cannot be sent (`api.md`, Field). */
export function unsupportedField(fields: J[]): string | null {
  const f = (fields ?? []).find((x: J) => !KNOWN_TYPES.includes(x?.type) && x?.required);
  return f ? String(f.key ?? "?") : null;
}

export function fieldDefaults(fields: J[]): Record<string, unknown> {
  const v: Record<string, unknown> = {};
  for (const f of fields ?? []) {
    if (!KNOWN_TYPES.includes(f?.type)) continue;
    if (f.default !== undefined) v[f.key] = f.default;
    else if (f.type === "boolean") v[f.key] = false;
    else if (f.type === "teams" && Array.isArray(f.choices) && f.choices.length) v[f.key] = f.choices[0];
    else if ((f.type === "enum" || f.type === "mode") && Array.isArray(f.values) && f.values.length && f.required) v[f.key] = f.values[0].id;
    else if (f.type === "stage" && f.nullable) v[f.key] = null;
  }
  return v;
}

/** The request member for a form: empty optional strings are left out. */
export function fieldBody(fields: J[], values: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const f of fields ?? []) {
    if (!KNOWN_TYPES.includes(f?.type) || !(f.key in values)) continue;
    const v = values[f.key];
    if ((v === "" || v === undefined) && !f.required) continue;
    out[f.key] = v;
  }
  return out;
}

/** Names of a mode id (`match-contract.md` 2: names come from the player's own game data first): the engine's label,
 *  else the launcher's generic word for the automatic match's key, else the server's catalog or label, else the id. */
export function useModeName() {
  const { L, t } = useC();
  return (id: string | null | undefined, meta: J, label?: J): string => {
    if (!id) return "";
    const own = getLive().names.modes[id];
    if (own) return own;
    const key = `mode.${id}` as Key;
    if (key in DICTS.en) return t(key);
    const cat = (meta?.catalog?.modes ?? []).find((m: J) => m?.id === id);
    return L(cat?.label) || L(label) || id;
  };
}

/** A stage id's name when the catalogue of the player's game data has none: the engine's label (also the player's game
 *  data), else the server's catalog or label; undefined: the card shows the id made readable. */
export function stageFallback(L: (x: J) => string, id: string, meta: J, label?: J): string | undefined {
  const cat = (meta?.catalog?.stages ?? []).find((m: J) => m?.id === id);
  return getLive().names.stages[id] || L(cat?.label) || L(label) || undefined;
}

/** Names of a stage id (the game's scene), `match-contract.md` 2: the game's own name in the interface language (the
 *  catalogue read from the player's files), else the engine's label, else the server's, else the id made readable. */
export function useStageName() {
  const { L } = useC();
  const info = useStageInfo(false);
  return (id: string | null | undefined, meta: J, label?: J): string => (id ? info(id, stageFallback(L, id, meta, label)).name : "");
}

export function vs(c: CFn, t: J): string {
  return Array.isArray(t) && t.length === 2 ? c("fmt.vs", { a: t[0], b: t[1] }) : "";
}

/** The form of a server's declared fields (login forms, room options). `nameMax` caps a `display_name` field. */
export function FieldForm({ fields, values, set, meta, idPrefix, nameMax }: {
  fields: J[]; values: Record<string, unknown>; set: (k: string, v: unknown) => void; meta: J; idPrefix: string; nameMax?: number;
}) {
  const { c, L } = useC();
  const { t } = useI18n();
  const modeName = useModeName();
  const stageName = useStageName();
  // The stages as the game shows them (its pictures and names, from the player's files): a grid instead of the list.
  const pictures = useStagePictures((fields ?? []).some((f: J) => f?.type === "stage"));
  return (
    <div className="stack cm-form">
      {(fields ?? []).filter((f: J) => KNOWN_TYPES.includes(f?.type)).map((f: J) => {
        const id = `${idPrefix}-${f.key}`;
        const label = L(f.label) || f.key;
        const help = L(f.help);
        const v = values[f.key];
        let control: ReactNode = null;
        switch (f.type) {
          case "string": {
            const max = f.key === "display_name" && nameMax ? Math.min(nameMax, f.max_len ?? nameMax) : f.max_len;
            control = <input id={id} type={f.secret ? "password" : "text"} value={String(v ?? "")} maxLength={max ?? undefined} autoComplete="off" onChange={(e) => set(f.key, e.target.value)} />;
            break;
          }
          case "integer":
            control = <input id={id} type="number" value={v === undefined || v === null ? "" : String(v)} min={f.min ?? undefined} max={f.max ?? undefined} onChange={(e) => set(f.key, e.target.value === "" ? undefined : Math.trunc(Number(e.target.value)))} />;
            break;
          case "boolean":
            control = (
              <label className="switch cm-switch">
                <input id={id} type="checkbox" checked={!!v} onChange={(e) => set(f.key, e.target.checked)} />
                <span className="switch-ui" aria-hidden="true" />
                <span>{v ? c("field.yes") : c("field.no")}</span>
              </label>
            );
            break;
          case "teams":
            control = (
              <div className="seg" role="radiogroup" aria-label={label}>
                {(f.choices ?? []).filter((x: J) => Array.isArray(x) && x.length === 2).map((ch: number[]) => {
                  const on = Array.isArray(v) && v[0] === ch[0] && v[1] === ch[1];
                  return <button key={ch.join("v")} type="button" role="radio" aria-checked={on} className={`seg-btn ${on ? "on" : ""}`} onClick={() => set(f.key, ch)}>{vs(c, ch)}</button>;
                })}
              </div>
            );
            break;
          case "enum":
          case "mode":
          case "stage": {
            const mode = values["mode"] as string | undefined;
            const list: J[] = f.type === "stage" && f.by_mode && mode && Array.isArray(f.by_mode[mode]) ? f.by_mode[mode].map((s: string) => ({ id: s })) : (f.values ?? []);
            if (f.type === "stage" && pictures) {
              // Every stage the server offers, those the chosen mode does not allow shown grayed.
              const allowed = list.map((x: J) => String(x.id));
              const ids = [...new Set([...(f.values ?? []).map((x: J) => String(x.id)), ...allowed])];
              const labelOf = (x: string) => (f.values ?? []).find((y: J) => String(y.id) === x)?.label;
              control = (
                <StageField id={id} ids={ids} value={v === null || v === undefined ? null : String(v)} label={label}
                  onChange={(nv) => set(f.key, nv === null ? (f.nullable ? null : undefined) : nv)}
                  nullable={!!f.nullable || !f.required} nullLabel={L(f.null_label) || c("field.null")}
                  unavailable={Object.fromEntries(ids.filter((x) => !allowed.includes(x)).map((x) => [x, t("stage.unavailable.mode")]))}
                  fallback={(x) => stageFallback(L, x, meta, labelOf(x))} />
              );
              break;
            }
            const text = (x: J) => (f.type === "mode" ? modeName(x.id, meta, x.label) : f.type === "stage" ? stageName(x.id, meta, x.label) : L(x.label) || x.id);
            control = (
              <select id={id} value={v === null || v === undefined ? "" : String(v)} onChange={(e) => set(f.key, e.target.value === "" ? (f.nullable ? null : undefined) : e.target.value)}>
                {(f.nullable || !f.required) && <option value="">{f.nullable ? L(f.null_label) || c("field.null") : "—"}</option>}
                {list.map((x: J) => <option key={String(x.id)} value={String(x.id)}>{text(x)}</option>)}
              </select>
            );
            break;
          }
        }
        return (
          <div key={f.key} className="cm-field">
            <label className="field-label" htmlFor={id}>{label}</label>
            {control}
            {help && <p className="hint">{help}</p>}
          </div>
        );
      })}
    </div>
  );
}

export function Who({ user, me, crown, children }: { user: J; me?: boolean; crown?: boolean; children?: ReactNode }) {
  const { c, L } = useC();
  const name = String(user?.display_name ?? "?");
  const badgeColor = safeColor(user?.badge?.color);
  const title = user?.title ? L(user.title).slice(0, 60) : "";
  return (
    <span className={`cm-who ${me ? "cm-me" : ""}`}>
      <span className="cm-avatar" aria-hidden="true">{name.slice(0, 1).toUpperCase()}</span>
      <span className="cm-name">{name}</span>
      {user?.badge && <ProtoIcon name={user.badge.icon} className="cm-ubadge" style={badgeColor ? { color: badgeColor } : undefined} />}
      {title && <span className="cm-utitle">{title}</span>}
      {crown && <IconCrown className="cm-crown" aria-label={c("room.owner")} />}
      {me && <span className="cm-tag">{c("room.you")}</span>}
      {children}
    </span>
  );
}

/** Seconds left until an RFC 3339 deadline, by the server's clock; re-renders every second. */
export function useLeft(deadline: string | null | undefined): number | null {
  const [, tick] = useState(0);
  useEffect(() => {
    if (!deadline) return;
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [deadline]);
  if (!deadline) return null;
  const t = Date.parse(deadline);
  return Number.isFinite(t) ? Math.max(0, Math.ceil((t - serverNow()) / 1000)) : null;
}

/** Seconds since an RFC 3339 time, by the server's clock. */
export function useSince(at: string | null | undefined): number | null {
  const [, tick] = useState(0);
  useEffect(() => {
    if (!at) return;
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [at]);
  if (!at) return null;
  const t = Date.parse(at);
  return Number.isFinite(t) ? Math.max(0, Math.floor((serverNow() - t) / 1000)) : null;
}

export function Left({ deadline }: { deadline: string | null | undefined }) {
  const { c } = useC();
  const s = useLeft(deadline);
  return <>{s === null ? "" : duration(c, s)}</>;
}

export function Tier({ tier }: { tier: J }) {
  const { L } = useC();
  if (!tier) return null;
  const color = typeof tier.color === "string" && /^#[0-9a-fA-F]{6}$/.test(tier.color) ? tier.color : null;
  return (
    <span className="cm-tier">
      {color && <span className="cm-tier-dot" style={{ background: color }} aria-hidden="true" />}
      {L(tier.name) || tier.id}
    </span>
  );
}

/** A player's rank on one line beside their name (`participants[].rating`, protocol 1.1.14): the ladder position
 *  ("#5"), the tier chip and the rating; "Placement" while the player is still being placed. Nothing when the server
 *  sent no usable rating (older servers, malformed data). */
export function RankBadge({ rating }: { rating: J }) {
  const { c, L } = useC();
  const r = rankOf(rating);
  if (!r) return null;
  if (r.placement) {
    const label = r.remaining !== null ? c("r.placement.left", { n: r.remaining }) : c("r.placement");
    return <span className="cm-rank cm-rank-placement" role="img" title={label} aria-label={label}><span aria-hidden="true">{c("r.placement")}</span></span>;
  }
  const parts = [
    r.position !== null ? c("r.badge.pos", { pos: r.position }) : "",
    r.tier ? String(L(r.tier.name) || r.tier.id) : "",
    r.rating !== null ? c("r.badge.rating", { rating: r.rating }) : "",
  ].filter((x) => x);
  const label = parts.join(c("r.badge.sep"));
  return (
    <span className="cm-rank" role="img" title={label} aria-label={label}>
      {r.positionText && <span className="cm-rank-pos" aria-hidden="true">{r.positionText}</span>}
      {r.tier && <span aria-hidden="true"><Tier tier={r.tier} /></span>}
      {r.rating !== null && <span className="cm-rank-value" aria-hidden="true">{r.rating}</span>}
    </span>
  );
}

export function fmtDay(at: string | null | undefined, lang: string): string {
  if (!at) return "";
  const d = new Date(at);
  return Number.isNaN(d.getTime()) ? at : formatDateTime(d, lang);
}

/** Runs an action and turns its failure into a sentence for the caller's notice. */
export function useAction() {
  const { c } = useC();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async <T,>(f: () => Promise<T>, extra?: (e: Fail) => Record<string, string | number>): Promise<T | undefined> => {
    setBusy(true);
    setError(null);
    try {
      return await f();
    } catch (e) {
      const fail = toFail(e);
      setError(failText(fail, c, extra ? extra(fail) : {}, myId()));
      return undefined;
    } finally {
      setBusy(false);
    }
  };
  return { busy, error, setError, run };
}

export function ErrorLine({ text }: { text: string | null }) {
  return text ? <p className="bad-text" role="alert">{text}</p> : null;
}

export function signed(n: number): string {
  return n > 0 ? `+${n}` : String(n);
}
