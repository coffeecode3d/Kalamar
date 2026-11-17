// Weapon: main weapons with their sub and special weapon.
import { useMemo, useState } from "react";
import type { GameEntry } from "../../api";
import { GameIcon, VirtualGrid } from "../../grid";
import { entryName, namedName, normalise } from "../../gamedata";
import { FieldNote, FilterBar, SearchBox, Select, sortEntries, type TabProps } from "./common";

/** `keep`: the choice is never emptied (the window before a match's launch): a click on the chosen weapon keeps it,
 *  and there is no "Clear" button. */
export function Weapon({ cat, profile, setProfile, lang, t, notes, advanced, keep }: TabProps & { keep?: boolean }) {
  const [q, setQ] = useState("");
  const [klass, setKlass] = useState("");
  const [sub, setSub] = useState("");
  const [special, setSpecial] = useState("");
  const [sort, setSort] = useState("order");

  const all = useMemo(() => (cat.categories.weapon ?? []).filter((e) => e.selectable), [cat]);
  const nameOf = useMemo(() => (e: GameEntry) => entryName(e, lang), [lang]);
  const subName = (s: string) => namedName(cat.subs[s], s, lang);
  const spName = (s: string) => namedName(cat.specials[s], s, lang);
  const className = (c: string) => namedName(cat.classes[c], c, lang);

  const classes = useMemo(() => [...new Set(all.map((e) => e.class).filter((c): c is string => !!c))].sort((a, b) => className(a).localeCompare(className(b))), [all, lang]); // eslint-disable-line react-hooks/exhaustive-deps
  const subs = useMemo(() => [...new Set(all.map((e) => e.sub).filter((c): c is string => !!c))].sort((a, b) => subName(a).localeCompare(subName(b))), [all, lang]); // eslint-disable-line react-hooks/exhaustive-deps
  const specials = useMemo(() => [...new Set(all.map((e) => e.special).filter((c): c is string => !!c))].sort((a, b) => spName(a).localeCompare(spName(b))), [all, lang]); // eslint-disable-line react-hooks/exhaustive-deps

  const items = useMemo(() => {
    const nq = normalise(q.trim());
    const f = all.filter((e) => {
      if (klass && e.class !== klass) return false;
      if (sub && e.sub !== sub) return false;
      if (special && e.special !== special) return false;
      if (nq && !normalise(`${nameOf(e)} ${e.sub ? subName(e.sub) : ""} ${e.special ? spName(e.special) : ""}`).includes(nq)) return false;
      return true;
    });
    return sortEntries(f, sort, nameOf);
  }, [all, q, klass, sub, special, sort, nameOf, lang]); // eslint-disable-line react-hooks/exhaustive-deps

  const chosen = profile.weapon;
  const chosenEntry = all.find((e) => e.id === chosen);
  const none = [{ id: "", label: t("profile.filter.all") }];

  return (
    <div className="tab-body">
      <FieldNote note={notes.weapon} t={t} />
      <FilterBar>
        <SearchBox value={q} onChange={setQ} label={t("profile.search")} />
        <Select label={t("profile.filter.class")} value={klass} onChange={setKlass} options={[...none, ...classes.map((c) => ({ id: c, label: className(c) }))]} />
        <Select label={t("profile.filter.sub")} value={sub} onChange={setSub} options={[...none, ...subs.map((c) => ({ id: c, label: subName(c) }))]} />
        <Select label={t("profile.filter.special")} value={special} onChange={setSpecial} options={[...none, ...specials.map((c) => ({ id: c, label: spName(c) }))]} />
        {advanced && <Select label={t("profile.sort")} value={sort} onChange={setSort} options={[
          { id: "order", label: t("profile.sort.order") }, { id: "name", label: t("profile.sort.name") }, { id: "id", label: t("profile.sort.id") }]} />}
      </FilterBar>
      <div className="row between small muted">
        <span>{t("profile.count", { n: items.length, total: all.length })}</span>
        <span>
          {chosenEntry ? <>{t("profile.selected")} <strong>{nameOf(chosenEntry)}</strong> {!keep && <button className="btn btn-small btn-ghost" onClick={() => setProfile((p) => ({ ...p, weapon: null }))}>{t("profile.clear")}</button>}</> : t("profile.noselection")}
        </span>
      </div>
      <VirtualGrid
        className="vg-fill"
        items={items}
        keyOf={(e) => e.id}
        label={t("profile.tab.weapon")}
        cellWidth={150}
        cellHeight={214}
        isSelected={(e) => e.id === chosen}
        onPick={(e) => setProfile((p) => ({ ...p, weapon: p.weapon === e.id && !keep ? null : e.id }))}
        empty={t("profile.empty")}
        render={(e) => (
          <>
            <GameIcon icon={e.icon} className="cell-icon" />
            <span className="cell-name" title={nameOf(e)}>{nameOf(e)}</span>
            <span className="cell-meta">
              {e.sub && <GameIcon icon={cat.subs[e.sub]?.icon} className="mini-icon" alt={subName(e.sub)} />}
              {e.special && <GameIcon icon={cat.specials[e.special]?.icon} className="mini-icon" alt={spName(e.special)} />}
            </span>
            <span className="cell-sub tiny muted">{e.class ? className(e.class) : ""}</span>
            <span className="cell-sub tiny muted">{[e.sub && subName(e.sub), e.special && spName(e.special)].filter(Boolean).join(" · ")}</span>
          </>
        )}
      />
    </div>
  );
}
