// Gear: head / clothes / shoes grids with icons, names, brand and main ability, search, filters and sorting.
import { useMemo, useState } from "react";
import type { GameEntry } from "../../api";
import { GameIcon, VirtualGrid } from "../../grid";
import { entryName, namedName, normalise, rarityStars } from "../../gamedata";
import { useApp } from "../../ctx";
import { SubAbilities } from "./SubAbilities";
import { FieldNote, FilterBar, Segmented, SearchBox, Select, sortEntries, type TabProps } from "./common";

type Slot = "head" | "clothes" | "shoes";

export function Gear({ cat, profile, setProfile, lang, t, notes, advanced, subs }: TabProps) {
  const { go } = useApp();
  const [slot, setSlot] = useState<Slot>("head");
  const [q, setQ] = useState("");
  const [brand, setBrand] = useState("");
  const [ability, setAbility] = useState("");
  const [rarity, setRarity] = useState("");
  const [sort, setSort] = useState("name");
  const [hidden, setHidden] = useState(false);

  const all = cat.categories[slot] ?? [];
  const nameOf = useMemo(() => (e: GameEntry) => entryName(e, lang), [lang]);
  const brandName = (b: string) => namedName(cat.brands[b], b, lang);
  const abilityName = (a: string) => namedName(cat.abilities[a], a, lang);

  const brands = useMemo(() => [...new Set(all.map((e) => e.brand).filter((b): b is string => !!b))].sort((a, b) => brandName(a).localeCompare(brandName(b))), [all, lang]); // eslint-disable-line react-hooks/exhaustive-deps
  const abilities = useMemo(() => [...new Set(all.map((e) => e.ability).filter((b): b is string => !!b))].sort((a, b) => abilityName(a).localeCompare(abilityName(b))), [all, lang]); // eslint-disable-line react-hooks/exhaustive-deps

  const items = useMemo(() => {
    const nq = normalise(q.trim());
    const f = all.filter((e) => {
      if (!hidden && !e.selectable) return false;
      if (brand && e.brand !== brand) return false;
      if (ability && e.ability !== ability) return false;
      if (rarity && String(e.rarity) !== rarity) return false;
      if (nq) {
        const hay = normalise(`${nameOf(e)} ${e.brand ? brandName(e.brand) : ""} ${e.ability ? abilityName(e.ability) : ""} ${e.row}`);
        if (!hay.includes(nq)) return false;
      }
      return true;
    });
    return sortEntries(f, sort, nameOf);
  }, [all, q, brand, ability, rarity, sort, hidden, nameOf, lang]); // eslint-disable-line react-hooks/exhaustive-deps

  const chosen = profile[slot];
  const chosenEntry = all.find((e) => e.id === chosen);
  const none = [{ id: "", label: t("profile.filter.all") }];

  return (
    <div className="tab-body">
      {(["head", "clothes", "shoes", "gear", "sub_abilities", `sub_abilities.${slot}`, "main_abilities", `main_abilities.${slot}`] as const).map((k) => <FieldNote key={k} note={notes[k]} t={t} />)}
      <FilterBar>
        <Segmented<Slot> value={slot} onChange={(v) => { setSlot(v); setBrand(""); setAbility(""); }} label={t("profile.tab.gear")}
          options={[{ id: "head", label: t("profile.gear.head") }, { id: "clothes", label: t("profile.gear.clothes") }, { id: "shoes", label: t("profile.gear.shoes") }]} />
        <SearchBox value={q} onChange={setQ} label={t("profile.search")} />
        <Select label={t("profile.filter.brand")} value={brand} onChange={setBrand} options={[...none, ...brands.map((b) => ({ id: b, label: brandName(b) }))]} />
        <Select label={t("profile.filter.ability")} value={ability} onChange={setAbility} options={[...none, ...abilities.map((a) => ({ id: a, label: abilityName(a) }))]} />
        {advanced && (
          <>
            <Select label={t("profile.filter.rarity")} value={rarity} onChange={setRarity} options={[...none, ...[0, 1, 2, 3].map((r) => ({ id: String(r), label: rarityStars(r) || "0" }))]} />
            <Select label={t("profile.sort")} value={sort} onChange={setSort} options={[
              { id: "name", label: t("profile.sort.name") }, { id: "rarity", label: t("profile.sort.rarity") },
              { id: "season", label: t("profile.sort.season") }, { id: "id", label: t("profile.sort.id") }]} />
            <label className="check"><input type="checkbox" checked={hidden} onChange={(e) => setHidden(e.target.checked)} /> {t("profile.showhidden")}</label>
          </>
        )}
      </FilterBar>
      <div className="row between small muted">
        <span>{t("profile.count", { n: items.length, total: all.filter((e) => e.selectable).length })}</span>
        <span>
          {chosenEntry ? <>{t("profile.selected")} <strong>{nameOf(chosenEntry)}</strong> <button className="btn btn-small btn-ghost" onClick={() => setProfile((p) => ({ ...p, [slot]: null }))}>{t("profile.clear")}</button></> : t("profile.noselection")}
        </span>
      </div>
      {chosenEntry && (
        <SubAbilities part={slot} partLabel={t(`profile.gear.${slot}` as "profile.gear.head")} pieceName={nameOf(chosenEntry)} own={chosenEntry.ability} pieces={all} cat={cat} profile={profile} setProfile={setProfile} lang={lang} t={t} info={subs ?? null} onUpdate={() => go("play")} />
      )}
      <VirtualGrid
        className="vg-fill"
        items={items}
        keyOf={(e) => e.id}
        label={t(`profile.gear.${slot}` as "profile.gear.head")}
        cellWidth={146}
        cellHeight={188}
        isSelected={(e) => e.id === chosen}
        onPick={(e) => { if (e.selectable) setProfile((p) => ({ ...p, [slot]: p[slot] === e.id ? null : e.id })); }}
        empty={t("profile.empty")}
        render={(e) => (
          <>
            <GameIcon icon={e.icon} className="cell-icon" />
            <span className="cell-name" title={nameOf(e)}>{nameOf(e)}</span>
            <span className="cell-meta">
              {e.brand && <GameIcon icon={cat.brands[e.brand]?.icon} className="mini-icon" alt={brandName(e.brand)} />}
              {e.ability && <GameIcon icon={cat.abilities[e.ability]?.icon} className="mini-icon" alt={abilityName(e.ability)} />}
              <span className="stars">{rarityStars(e.rarity)}</span>
            </span>
            <span className="cell-sub tiny muted">{e.unavailable ? <span className="bad-text" title={t(`profile.unavail.${e.unavailable}` as "profile.unavail.game_drops")}>{t(`profile.unavail.${e.unavailable}` as "profile.unavail.game_drops")}</span> : e.brand ? brandName(e.brand) : ""}</span>
          </>
        )}
      />
    </div>
  );
}
