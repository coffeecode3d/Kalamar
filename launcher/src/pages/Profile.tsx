import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { play } from "../sound/engine";
import { EMPTY_PROFILE, api, onIconProgress, toAppError, type CatalogueView, type GearAbilitiesInfo, type IconCacheInfo, type IconProgress, type LaunchReport, type Preflight, type Profile as ProfileT } from "../api";
import type { Notice } from "../ctx";
import { useApp } from "../ctx";
import { errorText, useI18n, type Key, type TFn } from "../i18n";
import { fmtMB } from "../ctx";
import { Card, NoticeBar, Page, useErrorNotice } from "../ui";
import { fieldLabel, refusalField, refusalTab, refusalText } from "../refusal";
import { byId, entryName } from "../gamedata";
import { GameIcon } from "../grid";
import { Character } from "./profile/Character";
import { NeedsGameData, type FieldNoteData } from "./profile/common";
import { Gear } from "./profile/Gear";
import { Presets } from "./profile/Presets";
import { applyPreset, type Preset } from "./profile/presetLogic";
import { GEAR_PARTS, mainChoices } from "./profile/subSlots";
import { Splashtag, TagPreview } from "./profile/Splashtag";
import { Weapon } from "./profile/Weapon";

type TabId = "character" | "gear" | "weapon" | "splashtag";
const TABS: TabId[] = ["character", "gear", "weapon", "splashtag"];

/** "About 3 min", "40 s". */
const rough = (secs: number) => (secs < 90 ? `${Math.max(10, Math.round(secs / 10) * 10)} s` : `${Math.round(secs / 60)} min`);

/** During an installation, before its merge is complete: when the Locker's game data arrives (the name can be chosen
 *  already, in the side card). */
function DataWait({ eta, fraction, t }: { eta: number | null; fraction: number | null; t: TFn }) {
  return (
    <div className="card data-wait" role="status">
      <h2 className="card-title">{t("profile.wait.title")}</h2>
      <p>{eta != null && eta > 0 ? t("profile.wait.body", { t: rough(eta) }) : t("profile.wait.body0")}</p>
      {fraction != null && <div className="data-wait-bar" aria-hidden="true"><span style={{ width: `${Math.round(fraction * 100)}%` }} /></div>}
      <p className="hint">{t("profile.wait.name")}</p>
    </div>
  );
}

export default function Profile({ notify }: { notify: (n: Notice | null) => void }) {
  const { t, lang } = useI18n();
  const { config, setConfig, go, reloadPlayer, advanced, install } = useApp();
  const errNotice = useErrorNotice();
  const [view, setView] = useState<CatalogueView | null>(null);
  const [p, setP] = useState<ProfileT>(EMPTY_PROFILE);
  const [saved, setSaved] = useState<ProfileT>(EMPTY_PROFILE);
  const [tab, setTab] = useState<TabId>("character");
  const [notice, setNotice] = useState<Notice | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [pre, setPre] = useState<Preflight | null>(null);
  const [report, setReport] = useState<LaunchReport | null>(null);
  const [cache, setCache] = useState<IconCacheInfo | null>(null);
  const [progress, setProgress] = useState<IconProgress | null>(null);
  const [subs, setSubs] = useState<GearAbilitiesInfo | null>(null);
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({});

  const load = useCallback(async (force: boolean) => {
    setBusy(true);
    try {
      const [c, loaded] = await Promise.all([api.profileCatalogue(force), api.profileLoad()]);
      setView(c);
      if (!force) {
        setP({ ...EMPTY_PROFILE, ...loaded, badges: [...(loaded.badges ?? []), null, null, null].slice(0, 3) });
        setSaved({ ...EMPTY_PROFILE, ...loaded });
      }
      void reloadPlayer(); // the rail's splashtag follows (the catalogue is loaded now)
      setCache(await api.iconCache());
      setPre(await api.playPreflight());
      setReport(await api.launchReport());
    } catch (e) {
      setNotice(errNotice(e));
    } finally {
      setBusy(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    void load(false);
    let off: (() => void) | undefined;
    void onIconProgress((pr) => {
      setProgress(pr);
      if (!pr.running) void api.iconCache().then(setCache);
    }).then((u) => (off = u));
    return () => off?.();
  }, [load]);

  useEffect(() => { void api.iconCache().then(setCache).catch(() => {}); }, [tab]);
  // Does the installed game apply sub-abilities and main abilities? Asked apart from the catalogue: the first answer may
  // run the game's capability listing (once per executable, no window).
  useEffect(() => {
    void api.gearAbilitiesInfo().then(setSubs)
      .catch(() => setSubs({ supported: false, update_needed: false, abilities: [], main_supported: false, main_update_needed: false, main_abilities: [] }));
  }, []);
  // During an installation the game data becomes readable when its merge ends (long before the compile does): the
  // catalogue is read again then, without touching what the player is editing.
  useEffect(() => {
    if (!install?.game_data || view?.origin !== "neutral") return;
    void api.profileCatalogue(false).then((c) => { setView(c); void reloadPlayer(); }).catch(() => {});
  }, [install?.game_data]); // eslint-disable-line react-hooks/exhaustive-deps
  const dataWait = view?.origin === "neutral" && !!install?.active && !install.game_data;

  const setProfile = useCallback((f: (p: ProfileT) => ProfileT) => setP((cur) => f(cur)), []);
  const dirty = useMemo(() => JSON.stringify({ ...p, schema: 1 }) !== JSON.stringify({ ...saved, schema: 1 }), [p, saved]);
  const game = view?.origin === "game";
  const max = view?.name_max_chars ?? 10;

  const wrap = async (f: () => Promise<void>) => {
    setFieldErrors({});
    try {
      await f();
    } catch (e) {
      const err = toAppError(e);
      if (err.kind === "validation") {
        setFieldErrors(Object.fromEntries(err.fields.map((x) => [x.key, x.message])));
        setNotice({ kind: "error", text: errorText(err, t) });
      } else {
        setNotice(errNotice(e));
      }
    }
  };

  const saveProfile = (next: ProfileT, okText = t("profile.saved.launch")) => wrap(async () => {
    const s = await api.profileSave(next);
    setP(s);
    setSaved(s);
    setNotice(null);
    play("success");
    notify({ kind: "ok", text: okText });
    void reloadPlayer();
    void api.playPreflight().then(setPre).catch(() => {});
  });
  const save = () => saveProfile(p);
  // A loadout preset is copied into the profile being edited, then saved through the same path as the Save button
  // (same validation, same feedback). Items the game data no longer has are left as they were.
  const applyLoadout = (preset: Preset) => {
    if (!cat) return;
    const has = (field: "head" | "clothes" | "shoes" | "weapon", id: number) => (cat.categories[field] ?? []).some((e) => e.id === id && e.selectable);
    const mainAbilities = Object.fromEntries(GEAR_PARTS.map((part) => [part, mainChoices(subs?.main_abilities ?? [], cat.categories[part] ?? [])]));
    const { profile: next, skipped } = applyPreset(p, preset, { has, subs: !!subs?.supported, abilities: subs?.abilities, main: !!subs?.main_supported, mainAbilities });
    setP(next);
    void saveProfile(next, skipped > 0 ? t("presets.applied.partial", { name: preset.name, n: skipped }) : t("presets.applied", { name: preset.name }));
  };
  const warm = async () => {
    try {
      setProgress({ done: 0, total: 0, failed: 0, running: true });
      await api.iconsWarm();
    } catch (e) {
      setProgress(null);
      setNotice(errNotice(e));
    }
  };
  const clearCache = async () => {
    try {
      await api.cacheClear();
      setProgress(null);
      await load(true);
    } catch (e) {
      setNotice(errNotice(e));
    }
  };

  const onTabKey = (e: React.KeyboardEvent, i: number) => {
    const d = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    const next = TABS[(i + d + TABS.length) % TABS.length];
    setTab(next);
    tabRefs.current[next]?.focus();
  };

  const cat = view?.catalogue;
  const summary = useMemo(() => {
    if (!cat) return [];
    const one = (id: "head" | "clothes" | "shoes" | "weapon", value: number | null) => {
      const e = byId(cat.categories[id]).get(value ?? -1);
      return e ? { id, entry: e } : null;
    };
    return [one("head", p.head), one("clothes", p.clothes), one("shoes", p.shoes), one("weapon", p.weapon)].filter((x): x is NonNullable<typeof x> => x !== null);
  }, [cat, p.head, p.clothes, p.shoes, p.weapon]);

  // The runtime's refusal at the last launch, shown next to the field it is about (translated, with its own sentence).
  const refusal = report?.feedback.state === "failed" ? report.feedback.refusal : null;
  const refusalFieldKey = refusal ? refusalField(refusal, p) : null;
  const notes: Record<string, FieldNoteData> = useMemo(
    () => (refusal && refusalFieldKey ? { [refusalFieldKey]: { text: refusalText(refusal, t), raw: advanced ? refusal.raw : "" } } : {}),
    [refusal, refusalFieldKey, t, advanced],
  );
  const refusalTabId = refusalFieldKey ? refusalTab(refusalFieldKey) : null;
  const tabProps = cat ? { cat, profile: p, setProfile, lang, t, game, notes, advanced, subs } : null;
  const nameLen = [...p.name.trim()].length;
  const setOption = (k: "apply_profile" | "skip_first_launch" | "unlock_all" | "unlock_chunks", v: boolean) => {
    if (!config) return;
    void api.setLaunchOptions({ apply_profile: config.apply_profile, skip_first_launch: config.skip_first_launch, unlock_all: config.unlock_all, unlock_chunks: config.unlock_chunks, [k]: v })
      .then((c) => { setConfig(c); void api.playPreflight().then(setPre).catch(() => {}); })
      .catch((e) => setNotice(errNotice(e)));
  };
  const missing = (list: string[]) => list.map((k) => t(`profile.cat.${k}` as "profile.cat.style").toLocaleLowerCase(lang)).join(", ");
  // The one sentence about what happens at the next launch.
  const readiness = !pre ? null
    : !config?.apply_profile ? { ok: null, text: t("profile.next.off") }
    : pre.profile === "incomplete" ? { ok: false, text: t("profile.next.missing", { fields: missing(pre.missing) }) }
    : pre.profile === "invalid" ? { ok: false, text: t("play.report.invalid", { detail: pre.detail ?? "" }) }
    : !pre.name_ok ? { ok: false, text: t("profile.ready.name") }
    : { ok: true, text: pre.save_exists ? t("profile.next.ok") : t("profile.next.new") };

  return (
    <Page title={t("profile.title")} subtitle={t("profile.subtitle2")} wide>
      <div className="profile-layout">
        <div className="profile-main">
          {refusal && refusalFieldKey && (
            <Card title={t("refusal.title")} tone="bad">
              <p><strong>{fieldLabel(refusalFieldKey, t)}</strong> : {refusalText(refusal, t)}</p>
              {advanced && <p className="tiny muted">{t("play.report.verbatim")} <code>{refusal.raw}</code></p>}
              {refusalTabId && refusalTabId !== tab && <button className="btn btn-small" onClick={() => setTab(refusalTabId)}>{t(`profile.tab.${refusalTabId}` as Key)}</button>}
            </Card>
          )}
          <div className="tabs" role="tablist" aria-label={t("profile.title")}>
            {TABS.map((id, i) => (
              <button
                key={id}
                ref={(el) => { tabRefs.current[id] = el; }}
                role="tab"
                id={`ptab-${id}`}
                aria-selected={tab === id}
                aria-controls="ppanel"
                tabIndex={tab === id ? 0 : -1}
                className={`tab ${tab === id ? "tab-on" : ""}`}
                onClick={() => setTab(id)}
                onKeyDown={(e) => onTabKey(e, i)}
              >
                {t(`profile.tab.${id}` as Key)}
              </button>
            ))}
          </div>
          <div id="ppanel" role="tabpanel" aria-labelledby={`ptab-${tab}`} className="tabpanel">
            {!view && <p className="muted">{t("common.loading")}</p>}
            {dataWait && install && <DataWait eta={install.game_data_eta} fraction={install.background.find((b) => b.id === "romfs")?.fraction ?? null} t={t} />}
            {!dataWait && tabProps && tab === "character" && <Character {...tabProps} />}
            {!dataWait && tabProps && tab !== "character" && !game && <NeedsGameData t={t} reason={view?.reason ?? null} />}
            {tabProps && game && (tab === "gear" || tab === "weapon") && <Presets cat={tabProps.cat} profile={p} subs={subs} t={t} onApply={applyLoadout} />}
            {tabProps && game && tab === "gear" && <Gear {...tabProps} />}
            {tabProps && game && tab === "weapon" && <Weapon {...tabProps} />}
            {tabProps && game && tab === "splashtag" && <Splashtag {...tabProps} />}
          </div>
        </div>

        <aside className="profile-side" aria-label={t("profile.preview")}>
          <Card className="side-card">
            {cat && <TagPreview cat={cat} profile={p} lang={lang} t={t} />}
            <label className="field-label" htmlFor="pname">{t("profile.name")}</label>
            <input id="pname" value={p.name} maxLength={max * 2} onChange={(e) => setP({ ...p, name: e.target.value })} aria-invalid={!!fieldErrors.name} />
            <div className={`hint ${nameLen > max ? "bad-text" : ""}`}>{nameLen}/{max}{advanced ? ` — ${t("profile.name.hint", { max })}` : ""}</div>
            {fieldErrors.name && <div className="bad-text">{fieldErrors.name}</div>}
            {summary.length > 0 && (
              <div className="summary">
                {summary.map((s) => (
                  <div key={s.id} className="summary-item" title={entryName(s.entry, lang)}>
                    <GameIcon icon={s.entry.icon} className="summary-icon" />
                    <span className="tiny">{entryName(s.entry, lang)}</span>
                  </div>
                ))}
              </div>
            )}
            {Object.entries(fieldErrors).filter(([k]) => k !== "name").map(([k, m]) => <div key={k} className="bad-text small">{k}: {m}</div>)}
            <div className="row wrap side-save">
              <button className="btn" onClick={() => void save()} disabled={!dirty && !!p.name}>{t("common.save")}</button>
              {dirty && <span className="hint">{t("settings.dirty")}</span>}
            </div>
            <NoticeBar notice={notice} />
          </Card>

          <Card title={t("profile.opts.title")} className="side-card">
            {config && (
              <div className="opts">
                {([["apply_profile", "profile.opts.apply"], ["skip_first_launch", "profile.opts.skip2"], ["unlock_all", "profile.opts.unlock2"]] as const).map(([k, label]) => (
                  <label key={k} className="check">
                    <input type="checkbox" checked={config[k]} disabled={k !== "apply_profile" && !config.apply_profile} onChange={(e) => setOption(k, e.target.checked)} />
                    <span>{t(label)}</span>
                  </label>
                ))}
                {subs?.supported && (
                  <label className="check">
                    <input type="checkbox" checked={config.unlock_chunks} disabled={!config.apply_profile} onChange={(e) => setOption("unlock_chunks", e.target.checked)} aria-describedby="opt-chunks-hint" />
                    <span>{t("profile.opts.chunks")}</span>
                  </label>
                )}
                {subs?.supported && <p id="opt-chunks-hint" className="hint opt-hint">{t("profile.opts.chunks.hint")}</p>}
              </div>
            )}
            {readiness && <p className={`next-line ${readiness.ok === false ? "bad" : readiness.ok ? "ok" : ""}`}><span className="hero-dot" />{readiness.text}</p>}
            {advanced && (
              <>
                {pre && <p className="small muted">{pre.save_exists ? t("profile.ready.partial") : t("profile.ready.newsave")}</p>}
                <p className="hint">{t("profile.opts.unlock.hint")}</p>
                <p className="hint">{t("profile.opts.how")}</p>
              </>
            )}
          </Card>

          {advanced && <Card title={t("profile.backups.title")} className="side-card">
            {pre?.backups.exists ? (
              <>
                <p className="small">{t("profile.backups.count", { n: pre.backups.count })}{pre.backups.newest ? ` · ${t("profile.backups.newest", { name: pre.backups.newest })}` : ""}</p>
                <p className="tiny muted mono wrapany">{pre.backups.dir}</p>
              </>
            ) : (
              <p className="small muted">{t("profile.backups.none")}</p>
            )}
            <p className="hint">{t("profile.backups.setting")} <button className="link" onClick={() => go("settings", "save")}>{t("nav.settings")}</button></p>
          </Card>}

          {advanced && <Card title={t("profile.data.title")} className="side-card">
            {game ? (
              <>
                <p className="small">{view?.from_cache ? t("profile.data.cached") : t("profile.data.built", { ms: cat?.build_ms ?? 0 })}</p>
                <p className="tiny muted mono wrapany">{view?.image}</p>
                <p className="small">{t("profile.data.icons", { n: cache?.files ?? 0, mb: fmtMB(cache?.bytes ?? 0) || "0" })}</p>
                {progress?.running && <progress max={progress.total || 1} value={progress.done} aria-label={t("profile.data.extracting")} />}
                {progress && !progress.running && progress.total > 0 && <p className="tiny muted">{t("profile.data.done", { n: progress.done, failed: progress.failed })}</p>}
                <div className="row wrap">
                  <button className="btn btn-small btn-ghost" disabled={busy || progress?.running} onClick={() => void warm()}>{t("profile.data.extract")}</button>
                  {progress?.running && <button className="btn btn-small btn-ghost" onClick={() => void api.iconsCancel()}>{t("profile.data.cancel")}</button>}
                  <button className="btn btn-small btn-ghost" disabled={busy} onClick={() => void clearCache()}>{t("profile.data.rebuild")}</button>
                </div>
                <p className="hint">{t("profile.data.local")}</p>
              </>
            ) : (
              <>
                <p className="small">{t("profile.data.neutral")}</p>
                <button className="btn btn-small btn-ghost" disabled={busy} onClick={() => void load(true)}>{t("common.refresh")}</button>
              </>
            )}
          </Card>}
        </aside>
      </div>
    </Page>
  );
}
