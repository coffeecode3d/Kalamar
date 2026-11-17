// The window to change weapon before a community match's launch (runner phase `weapon`, `proto/runner.rs`): a
// full-frame panel once the mode and the stage are final. The countdown, the weapon the game will start with, the
// Locker's weapon grid and the saved presets; a choice is saved to the profile at once through the Locker's own save
// (the game reads the profile at the launch), and "Ready" lets the launch go on before the countdown ends. The runner
// decides when the window ends: this panel never holds the match.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { play } from "../sound/engine";
import { BRAND } from "../brand";
import { api, toAppError, type CatalogueView, type GearAbilitiesInfo, type Profile } from "../api";
import { useApp } from "../ctx";
import { errorText } from "../i18n";
import { byId, entryName, namedName } from "../gamedata";
import { GameIcon } from "../grid";
import { IconCheck } from "../icons";
import { StageCard } from "../stages";
import { Ring, clock } from "../match/StageLayout";
import { Weapon } from "../pages/profile/Weapon";
import { applyPreset, matches, type Preset } from "../pages/profile/presetLogic";
import { GEAR_PARTS, mainChoices } from "../pages/profile/subSlots";
import { proto, type J, type RunnerStatus } from "./api";
import { serverNow, useLive } from "./store";
import { useC } from "./texts";
import { ErrorLine, stageFallback, useAction, useModeName, vs } from "./ui";
import { windowFraction, windowLeft } from "./weaponState";
import "./weapon.css";

const PRESET_ICONS = ["weapon", "head", "clothes", "shoes"] as const;

/** The window's countdown by the server's clock: seconds left, and the ring's fraction of the window as first seen.
 *  Re-renders every 250 ms. */
function useWindowClock(r: RunnerStatus): { left: number; fraction: number } | null {
  const [, tick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => tick((n) => n + 1), 250);
    return () => window.clearInterval(id);
  }, []);
  const total = useRef<{ key: string; s: number } | null>(null);
  const left = windowLeft(r.weapon_until, serverNow());
  if (left === null) return null;
  const key = `${r.match_id}:${r.weapon_until}`;
  if (total.current?.key !== key) total.current = { key, s: Math.max(1, left) };
  return { left, fraction: windowFraction(left, total.current.s) };
}

export function WeaponWindow({ r, match }: { r: RunnerStatus; match: J | null }) {
  const { c, L, t, lang } = useC();
  const { advanced } = useApp();
  const live = useLive();
  const meta = live.overview?.meta;
  const modeName = useModeName();
  const a = useAction();
  const tclock = useWindowClock(r);
  const [view, setView] = useState<CatalogueView | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  const [p, setP] = useState<Profile | null>(null);
  const [subs, setSubs] = useState<GearAbilitiesInfo | null>(null);
  const [presets, setPresets] = useState<Preset[]>([]);
  const [refused, setRefused] = useState<string | null>(null);
  const [sent, setSent] = useState(false);
  // The profile as last saved, and the saves in order (a choice made while another is being saved follows it).
  const saved = useRef<Profile | null>(null);
  const chain = useRef<Promise<void>>(Promise.resolve());
  const readyRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    let alive = true;
    void Promise.all([api.profileCatalogue(), api.profileLoad()])
      .then(([v, prof]) => {
        if (!alive) return;
        setView(v);
        saved.current = prof;
        setP(prof);
      })
      .catch(() => { if (alive) setUnavailable(true); });
    void api.gearAbilitiesInfo().then((s) => { if (alive) setSubs(s); }).catch(() => {});
    void api.loadoutsList().then((l) => { if (alive) setPresets(l); }).catch(() => {});
    return () => { alive = false; };
  }, []);

  // The primary action has the focus when the panel opens (Enter starts the game); it goes back where it was after.
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null;
    readyRef.current?.focus();
    return () => { if (before && document.contains(before)) before.focus(); };
  }, [r.match_id]);

  /** Saves the profile with one change, through the Locker's save (same checks); a refusal keeps the previous choice
   *  and says why. */
  const save = useCallback((change: (p: Profile) => Profile) => {
    chain.current = chain.current.then(async () => {
      const cur = saved.current;
      if (!cur) return;
      const next = change(cur);
      if (JSON.stringify(next) === JSON.stringify(cur)) return;
      try {
        const s = await api.profileSave(next);
        saved.current = s;
        setP(s);
        setRefused(null);
      } catch (e) {
        const err = toAppError(e);
        setRefused([errorText(err, t), ...err.fields.map((f) => `${f.key}: ${f.message}`)].join(" "));
        setP(cur);
      }
    });
  }, [t]);

  const cat = view?.origin === "game" ? view.catalogue : null;
  const maps = useMemo(() => (cat ? { head: byId(cat.categories.head), clothes: byId(cat.categories.clothes), shoes: byId(cat.categories.shoes), weapon: byId(cat.categories.weapon) } : null), [cat]);
  const current = p?.weapon != null ? maps?.weapon.get(p.weapon) : undefined;
  // A preset is copied into the profile as the Locker does it (items the game data no longer has are left as they were).
  const apply = (preset: Preset) => {
    if (!cat) return;
    const has = (field: "head" | "clothes" | "shoes" | "weapon", id: number) => (cat.categories[field] ?? []).some((e) => e.id === id && e.selectable);
    const mainAbilities = Object.fromEntries(GEAR_PARTS.map((part) => [part, mainChoices(subs?.main_abilities ?? [], cat.categories[part] ?? [])]));
    save((cur) => applyPreset(cur, preset, { has, subs: !!subs?.supported, abilities: subs?.abilities, main: !!subs?.main_supported, mainAbilities }).profile);
  };
  const ready = () => void a.run(async () => {
    if (!live.server || sent) return;
    play("confirm");
    // A choice still being saved is in the profile before the game reads it.
    await chain.current;
    await proto.weaponReady(live.server, r.match_id);
    setSent(true);
  });

  const rules = match?.rules ?? {};
  const stage: string | null = typeof rules.stage === "string" ? rules.stage : null;
  const facts = [vs(c, rules.teams), modeName(rules.mode, meta), rules.rated ? c("q.rated") : ""].filter(Boolean).join(" · ");

  return (
    <div className="stage stage2 wp" role="dialog" aria-modal="true" aria-labelledby="wp-title">
      <header className="st-head">
        <div className="st-head-text">
          <div className="eyebrow">{c("wp.eyebrow")} · {BRAND.name}</div>
          <h1 id="wp-title" className="display stage-title">{c("wp.title")}</h1>
          <p className="wp-sub">{c("wp.sub")}</p>
        </div>
        <div className="st-head-actions">
          <button ref={readyRef} type="button" className="btn btn-big" disabled={a.busy || sent} onClick={ready}>
            <IconCheck /> {sent ? c("m.runner.launching") : c("wp.ready")}
          </button>
        </div>
      </header>
      <div className="wp-cols">
        <aside className="wp-side">
          <section className="wp-now">
            {tclock ? <Ring fraction={tclock.fraction} value={clock(tclock.left)} caption={c("wp.ring")} /> : <Ring fraction={null} spin value="…" />}
            {cat && p && (
              <div className="wp-current" aria-live="polite">
                <span className="st-block-title">{c("wp.current")}</span>
                {current ? (
                  <>
                    <span className="wp-weapon">
                      <GameIcon icon={current.icon} className="wp-weapon-icon" />
                      <strong className="wp-weapon-name">{entryName(current, lang)}</strong>
                    </span>
                    {(current.sub || current.special) && (
                      <span className="small muted">{[current.sub && namedName(cat.subs[current.sub], current.sub, lang), current.special && namedName(cat.specials[current.special], current.special, lang)].filter(Boolean).join(" · ")}</span>
                    )}
                  </>
                ) : (
                  <span className="small muted">{c("wp.none")}</span>
                )}
              </div>
            )}
          </section>
          <ErrorLine text={refused ? c("wp.refused", { reason: refused }) : a.error} />
          {match && (
            <section className="st-block" aria-label={c("m.recap.title")}>
              <h2 className="st-block-title">{c("m.recap.title")}</h2>
              <StageCard id={stage} fallback={stage ? stageFallback(L, stage, meta) : c("field.null")} size="sm" sub={facts || undefined} />
            </section>
          )}
          {cat && maps && presets.length > 0 && (
            <section className="st-block" aria-label={t("presets.title")}>
              <h2 className="st-block-title">{t("presets.title")}</h2>
              <ul className="wp-presets">
                {presets.map((x) => {
                  const on = !!p && matches(x, p, !!subs?.supported, !!subs?.main_supported);
                  return (
                    <li key={x.name}>
                      <button type="button" className={`wp-preset ${on ? "on" : ""}`} aria-pressed={on} title={x.name} onClick={() => apply(x)}>
                        <span className="preset-icons" aria-hidden="true">
                          {PRESET_ICONS.map((f) => <GameIcon key={f} icon={maps[f].get(x[f] ?? -1)?.icon} className="preset-icon" />)}
                        </span>
                        <span className="wp-preset-name">{x.name}</span>
                        {on && <IconCheck className="wp-preset-on" />}
                      </button>
                    </li>
                  );
                })}
              </ul>
            </section>
          )}
          {cat && <p className="hint">{c("wp.hint")}</p>}
        </aside>
        <main className="wp-main" aria-label={t("profile.tab.weapon")}>
          {unavailable || (view && !cat) ? (
            <p className="notice notice-info">{c("wp.nocat")}</p>
          ) : !cat || !p ? (
            <p className="muted">{t("common.loading")}</p>
          ) : (
            <Weapon cat={cat} profile={p} setProfile={save} lang={lang} t={t} game notes={{}} advanced={advanced} subs={subs} keep />
          )}
        </main>
      </div>
    </div>
  );
}
