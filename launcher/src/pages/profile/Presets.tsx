// Loadout presets: a compact panel above the gear and weapon tabs. Each preset shows its name and the icons of its
// weapon and three gear pieces, with Apply, Rename and Delete; "Save" stores the loadout being edited under a name.
// Overwriting and deleting ask first. Applying belongs to the page: `onApply` copies the fields into the profile and
// saves it through the page's own save path.
import { useEffect, useMemo, useState } from "react";
import { api, toAppError, type GameCatalogue, type GearAbilitiesInfo, type Profile } from "../../api";
import type { Notice } from "../../ctx";
import { byId } from "../../gamedata";
import { GameIcon } from "../../grid";
import { errorText, type TFn } from "../../i18n";
import { Modal, NoticeBar } from "../../ui";
import { MAX_PRESETS, PRESET_NAME_MAX, loadoutOf, matches, sameName, type Preset } from "./presetLogic";

type Dialog = { kind: "overwrite"; name: string } | { kind: "delete"; preset: Preset } | { kind: "rename"; preset: Preset; value: string };

export function Presets({ cat, profile, subs, t, onApply }: {
  cat: GameCatalogue;
  profile: Profile;
  subs: GearAbilitiesInfo | null;
  t: TFn;
  onApply: (p: Preset) => void;
}) {
  const [list, setList] = useState<Preset[]>([]);
  const [name, setName] = useState("");
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState(false);
  const withSubs = !!subs?.supported;
  const withMain = !!subs?.main_supported;

  // A list that cannot be read is an empty list: the Locker is never blocked by it.
  useEffect(() => { void api.loadoutsList().then(setList).catch(() => setList([])); }, []);

  const maps = useMemo(() => ({ head: byId(cat.categories.head), clothes: byId(cat.categories.clothes), shoes: byId(cat.categories.shoes), weapon: byId(cat.categories.weapon) }), [cat]);

  const run = async (f: () => Promise<Preset[]>, okText: string) => {
    setBusy(true);
    try {
      setList(await f());
      setNotice({ kind: "ok", text: okText });
      return true;
    } catch (e) {
      setNotice({ kind: "error", text: errorText(toAppError(e), t) });
      return false;
    } finally {
      setBusy(false);
    }
  };

  // Sub-abilities and main abilities are stored only when the installed game applies them.
  const store = async (n: string) => {
    const l = loadoutOf(profile);
    const none = { head: null, clothes: null, shoes: null };
    const loadout = { ...l, sub_abilities: withSubs ? l.sub_abilities : none, main_abilities: withMain ? l.main_abilities : none };
    if (await run(() => api.loadoutsSave(n, loadout), t("presets.saved", { name: n.trim() }))) setName("");
  };
  const onSave = () => {
    const n = name.trim();
    if (!n) return;
    if (list.some((p) => sameName(p.name, n))) setDialog({ kind: "overwrite", name: n });
    else void store(n);
  };

  const full = list.length >= MAX_PRESETS;
  return (
    <section className="presets" aria-label={t("presets.title")}>
      <div className="row between wrap">
        <h3 className="presets-title">{t("presets.title")}</h3>
        <span className="hint">{t("presets.count", { n: list.length, max: MAX_PRESETS })}</span>
      </div>
      <form className="presets-save row wrap" onSubmit={(e) => { e.preventDefault(); onSave(); }}>
        <label className="sr-only" htmlFor="preset-name">{t("presets.name")}</label>
        <input id="preset-name" value={name} maxLength={PRESET_NAME_MAX * 2} placeholder={t("presets.name")} onChange={(e) => setName(e.target.value)} />
        <button className="btn btn-small" type="submit" disabled={busy || !name.trim() || (full && !list.some((p) => sameName(p.name, name)))}>{t("presets.save")}</button>
      </form>
      {full && <p className="hint">{t("presets.full", { max: MAX_PRESETS })}</p>}
      <NoticeBar notice={notice} />
      {list.length === 0 ? (
        <p className="hint">{t("presets.empty")}</p>
      ) : (
        <ul className="preset-list">
          {list.map((p) => {
            const current = matches(p, profile, withSubs, withMain);
            return (
              <li key={p.name} className={`preset ${current ? "preset-on" : ""}`}>
                <div className="preset-icons" aria-hidden="true">
                  {(["weapon", "head", "clothes", "shoes"] as const).map((f) => <GameIcon key={f} icon={maps[f].get(p[f] ?? -1)?.icon} className="preset-icon" />)}
                </div>
                <div className="preset-name" title={p.name}>
                  <strong>{p.name}</strong>
                  {current && <span className="preset-current">{t("presets.current")}</span>}
                </div>
                <div className="preset-actions row wrap">
                  <button className="btn btn-small" disabled={busy} aria-label={t("presets.apply.aria", { name: p.name })} onClick={() => onApply(p)}>{t("presets.apply")}</button>
                  <button className="btn btn-small btn-ghost" disabled={busy} aria-label={t("presets.rename.aria", { name: p.name })} onClick={() => setDialog({ kind: "rename", preset: p, value: p.name })}>{t("presets.rename")}</button>
                  <button className="btn btn-small btn-ghost" disabled={busy} aria-label={t("presets.delete.aria", { name: p.name })} onClick={() => setDialog({ kind: "delete", preset: p })}>{t("presets.delete")}</button>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      {dialog?.kind === "overwrite" && (
        <Modal title={t("presets.overwrite.title")} onClose={() => setDialog(null)} labelledBy="preset-dlg">
          <p>{t("presets.overwrite.body", { name: dialog.name })}</p>
          <div className="row">
            <button className="btn btn-ghost" onClick={() => setDialog(null)}>{t("common.cancel")}</button>
            <button className="btn" data-autofocus onClick={() => { const n = dialog.name; setDialog(null); void store(n); }}>{t("presets.overwrite.ok")}</button>
          </div>
        </Modal>
      )}
      {dialog?.kind === "delete" && (
        <Modal title={t("presets.delete.title")} onClose={() => setDialog(null)} labelledBy="preset-dlg">
          <p>{t("presets.delete.body", { name: dialog.preset.name })}</p>
          <div className="row">
            <button className="btn btn-ghost" data-autofocus onClick={() => setDialog(null)}>{t("common.cancel")}</button>
            <button className="btn btn-danger" onClick={() => { const n = dialog.preset.name; setDialog(null); void run(() => api.loadoutsDelete(n), t("presets.deleted", { name: n })); }}>{t("presets.delete")}</button>
          </div>
        </Modal>
      )}
      {dialog?.kind === "rename" && (
        <Modal title={t("presets.rename.title")} onClose={() => setDialog(null)} labelledBy="preset-dlg">
          <form onSubmit={(e) => {
            e.preventDefault();
            const { preset, value } = dialog;
            if (!value.trim()) return;
            setDialog(null);
            void run(() => api.loadoutsRename(preset.name, value), t("presets.renamed", { name: value.trim() }));
          }}>
            <label className="field-label" htmlFor="preset-rename">{t("presets.name")}</label>
            <input id="preset-rename" data-autofocus value={dialog.value} maxLength={PRESET_NAME_MAX * 2} onChange={(e) => setDialog({ ...dialog, value: e.target.value })} />
            <div className="row">
              <button type="button" className="btn btn-ghost" onClick={() => setDialog(null)}>{t("common.cancel")}</button>
              <button type="submit" className="btn" disabled={!dialog.value.trim()}>{t("presets.rename")}</button>
            </div>
          </form>
        </Modal>
      )}
    </section>
  );
}
