// Abilities of the piece worn on one part: the main slot and three sub slot buttons (icon and name, or "Empty"), one
// picker per kind of slot (for a sub slot every ability a sub slot can hold, "Empty", and the shortcut that fills all
// three; for the main slot the main abilities the game's pieces of that part have), and "Clear". Each kind is shown only
// when the installed game applies it; otherwise one line says a game update is needed.
import { useState } from "react";
import type { GameCatalogue, GameEntry, GearAbilitiesInfo, Profile } from "../../api";
import { namedName } from "../../gamedata";
import { GameIcon } from "../../grid";
import type { TFn } from "../../i18n";
import { Modal } from "../../ui";
import { SUB_SLOTS, chosenMain, clearPart, fillAll, mainChoices, mainOf, pickable, setMain, setSlot, slotsOf, type GearPart } from "./subSlots";

export function SubAbilities({ part, partLabel, pieceName, own, pieces, cat, profile, setProfile, lang, t, info, onUpdate }: {
  part: GearPart;
  partLabel: string;
  pieceName: string;
  /** The worn piece's own main ability (the game's), null when it has none. */
  own: string | null;
  /** Every piece of this part in the catalogue: the main abilities they have are the ones the main slot offers. */
  pieces: GameEntry[];
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
  const [mainOpen, setMainOpen] = useState(false);
  const [fill, setFill] = useState(false);
  if (!info) return null;
  const subsOn = info.supported;
  const mainOn = info.main_supported;
  // The update line: on its own when no slot is shown, else one line inside the section for the kind not applied yet.
  const update = (text: string, inline = false) => (
    <div className={inline ? "subs-off subs-off-line" : "subs subs-off"} role="note">
      <span>{text}</span>
      <button type="button" className="btn btn-small" onClick={onUpdate}>{t("profile.subs.update.btn")}</button>
    </div>
  );
  if (!subsOn && !mainOn) {
    if (info.update_needed && info.main_update_needed) return update(t("profile.abil.update"));
    if (info.update_needed) return update(t("profile.subs.update"));
    return info.main_update_needed ? update(t("profile.main.update")) : null;
  }
  const nameOf = (id: string) => namedName(cat.abilities[id], id, lang);
  const slots = slotsOf(profile, part);
  const options = pickable(info.abilities, nameOf);
  const mainOptions = pickable(mainChoices(info.main_abilities, pieces), nameOf);
  const chosen = chosenMain(profile, part);
  const main = mainOf(profile, part, own);
  const close = () => { setOpen(null); setFill(false); };
  const choose = (ability: string | null) => {
    if (open === null) return;
    const i = open;
    setProfile((p) => (fill ? fillAll(p, part, ability) : setSlot(p, part, i, ability)));
    close();
  };
  const chooseMain = (ability: string) => {
    setProfile((p) => setMain(p, part, ability));
    setMainOpen(false);
  };
  const mainName = main ? nameOf(main) : t("profile.main.none");
  const hid = `subs-${part}`;
  return (
    <section className="subs" aria-labelledby={hid}>
      <div className="row between wrap">
        <h3 id={hid} className="subs-title">{t(mainOn ? "profile.abil.title" : "profile.subs.title", { piece: pieceName })}</h3>
        <div className="row wrap">
          {mainOn && own && chosen !== null && chosen !== own && (
            <button type="button" className="btn btn-small btn-ghost" aria-label={t("profile.main.reset.aria", { part: partLabel })} onClick={() => chooseMain(own)}>
              {t("profile.main.reset")}
            </button>
          )}
          {subsOn && (
            <button type="button" className="btn btn-small btn-ghost" aria-label={t("profile.subs.clear.aria", { part: partLabel })} onClick={() => setProfile((p) => clearPart(p, part))}>
              {t("profile.subs.clear")}
            </button>
          )}
        </div>
      </div>
      <div className="abil-row">
        {mainOn && (
          <div className="abil-group abil-main">
            <span className="abil-label" aria-hidden="true">{t("profile.main.label")}</span>
            <button
              type="button"
              className={`sub-slot main-slot ${chosen !== null && chosen !== own ? "sub-slot-on" : ""}`}
              aria-haspopup="dialog"
              aria-label={t("profile.main.slot.aria", { part: partLabel, ability: mainName })}
              onClick={() => setMainOpen(true)}
            >
              {main ? <GameIcon icon={cat.abilities[main]?.icon} className="sub-icon" /> : <span className="sub-icon sub-icon-empty" aria-hidden="true" />}
              <span className="sub-name">{mainName}</span>
              {main !== null && main === own && <span className="main-own tiny muted">{t("profile.main.own")}</span>}
            </button>
          </div>
        )}
        {subsOn && (
          <div className="abil-group abil-subs">
            {mainOn && <span className="abil-label" aria-hidden="true">{t("profile.subs.label")}</span>}
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
          </div>
        )}
      </div>
      {mainOn && <p className="hint">{t("profile.main.hint")}</p>}
      {subsOn && <p className="hint">{t("profile.subs.stars")} {t("profile.subs.room")}</p>}
      {subsOn && !mainOn && info.main_update_needed && update(t("profile.main.update"), true)}
      {!subsOn && info.update_needed && update(t("profile.subs.update"), true)}
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
      {mainOpen && (
        <Modal title={t("profile.main.pick", { part: partLabel })} onClose={() => setMainOpen(false)} wide labelledBy="main-pick-title">
          <div className="subs-list" role="radiogroup" aria-label={t("profile.main.picklist", { part: partLabel })}>
            {mainOptions.map((o) => (
              <button key={o.id} type="button" role="radio" aria-checked={main === o.id} data-autofocus={main === o.id ? "" : undefined} className={`sub-opt ${main === o.id ? "sub-opt-on" : ""}`} onClick={() => chooseMain(o.id)}>
                <GameIcon icon={cat.abilities[o.id]?.icon} className="sub-icon" />
                <span className="sub-name">{o.name}</span>
                {o.id === own && <span className="main-own tiny muted">{t("profile.main.own")}</span>}
              </button>
            ))}
          </div>
          <div className="row"><button type="button" className="btn btn-ghost" onClick={() => setMainOpen(false)}>{t("common.cancel")}</button></div>
        </Modal>
      )}
    </section>
  );
}
