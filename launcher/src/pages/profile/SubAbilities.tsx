// Sub-abilities of the piece worn on one part: three slot buttons (icon and name, or "Empty"), a picker for one slot
// (every ability a sub slot can hold, "Empty", and the shortcut that fills all three), and "Clear". Shown only when the
// installed game applies them; otherwise one line says a game update is needed.
import { useState } from "react";
import type { GameCatalogue, GearAbilitiesInfo, Profile } from "../../api";
import { namedName } from "../../gamedata";
import { GameIcon } from "../../grid";
import type { TFn } from "../../i18n";
import { Modal } from "../../ui";
import { SUB_SLOTS, clearPart, fillAll, pickable, setSlot, slotsOf, type GearPart } from "./subSlots";

export function SubAbilities({ part, partLabel, pieceName, cat, profile, setProfile, lang, t, info, onUpdate }: {
  part: GearPart;
  partLabel: string;
  pieceName: string;
  cat: GameCatalogue;
  profile: Profile;
  setProfile: (f: (p: Profile) => Profile) => void;
  lang: string;
  t: TFn;
  info: GearAbilitiesInfo | null;
  /** Opens the screen that offers the game update. */
  onUpdate: () => void;
}) {
  const [open, setOpen] = useState<number | null>(null);
  const [fill, setFill] = useState(false);
  if (!info) return null;
  if (!info.supported) {
    if (!info.update_needed) return null;
    return (
      <div className="subs subs-off" role="note">
        <span>{t("profile.subs.update")}</span>
        <button type="button" className="btn btn-small" onClick={onUpdate}>{t("profile.subs.update.btn")}</button>
      </div>
    );
  }
  const nameOf = (id: string) => namedName(cat.abilities[id], id, lang);
  const slots = slotsOf(profile, part);
  const options = pickable(info.abilities, nameOf);
  const close = () => { setOpen(null); setFill(false); };
  const choose = (ability: string | null) => {
    if (open === null) return;
    const i = open;
    setProfile((p) => (fill ? fillAll(p, part, ability) : setSlot(p, part, i, ability)));
    close();
  };
  const hid = `subs-${part}`;
  return (
    <section className="subs" aria-labelledby={hid}>
      <div className="row between wrap">
        <h3 id={hid} className="subs-title">{t("profile.subs.title", { piece: pieceName })}</h3>
        <button type="button" className="btn btn-small btn-ghost" aria-label={t("profile.subs.clear.aria", { part: partLabel })} onClick={() => setProfile((p) => clearPart(p, part))}>
          {t("profile.subs.clear")}
        </button>
      </div>
      <div className="subs-slots" role="group" aria-label={t("profile.subs.group", { part: partLabel })}>
        {slots.map((s, i) => (
          <button
            key={i}
            type="button"
            className={`sub-slot ${s ? "sub-slot-on" : ""}`}
            aria-haspopup="dialog"
            aria-label={t("profile.subs.slot.aria", { n: i + 1, part: partLabel, ability: s ? nameOf(s) : t("profile.subs.empty") })}
            onClick={() => setOpen(i)}
          >
            {s ? <GameIcon icon={cat.abilities[s]?.icon} className="sub-icon" /> : <span className="sub-icon sub-icon-empty" aria-hidden="true" />}
            <span className="sub-name">{s ? nameOf(s) : t("profile.subs.empty")}</span>
          </button>
        ))}
      </div>
      <p className="hint">{t("profile.subs.stars")} {t("profile.subs.room")}</p>
      {open !== null && open < SUB_SLOTS && (
        <Modal title={t("profile.subs.pick", { n: open + 1 })} onClose={close} wide labelledBy="subs-pick-title">
          <label className="check subs-fill">
            <input type="checkbox" checked={fill} onChange={(e) => setFill(e.target.checked)} />
            <span>{t("profile.subs.fill")}</span>
          </label>
          <div className="subs-list" role="radiogroup" aria-label={t("profile.subs.picklist", { n: open + 1 })}>
            <button type="button" role="radio" aria-checked={slots[open] === null} data-autofocus={slots[open] === null ? "" : undefined} className={`sub-opt ${slots[open] === null ? "sub-opt-on" : ""}`} onClick={() => choose(null)}>
              <span className="sub-icon sub-icon-empty" aria-hidden="true" />
              <span className="sub-name">{t("profile.subs.empty")}</span>
            </button>
            {options.map((o) => (
              <button key={o.id} type="button" role="radio" aria-checked={slots[open] === o.id} data-autofocus={slots[open] === o.id ? "" : undefined} className={`sub-opt ${slots[open] === o.id ? "sub-opt-on" : ""}`} onClick={() => choose(o.id)}>
                <GameIcon icon={cat.abilities[o.id]?.icon} className="sub-icon" />
                <span className="sub-name">{o.name}</span>
              </button>
            ))}
          </div>
          <div className="row"><button type="button" className="btn btn-ghost" onClick={close}>{t("common.cancel")}</button></div>
        </Modal>
      )}
    </section>
  );
}
