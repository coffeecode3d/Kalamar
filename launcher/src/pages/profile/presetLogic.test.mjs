import test from "node:test";
import assert from "node:assert/strict";
import { applyPreset, loadoutOf, matches, sameName } from "./presetLogic.ts";

const base = { schema: 1, name: "K", hair: 4, head: 1, clothes: 2, shoes: 3, weapon: 10, sub_abilities: { head: ["A", null, null], clothes: null, shoes: null } };
const preset = { name: "P", head: 5, clothes: 6, shoes: null, weapon: 20, sub_abilities: { head: ["B", "C", null], clothes: null, shoes: null } };
const all = { has: () => true, subs: true };

test("a preset copies the five loadout fields and nothing else", () => {
  const { profile, skipped } = applyPreset(base, preset, all);
  assert.equal(skipped, 0);
  assert.deepEqual([profile.head, profile.clothes, profile.shoes, profile.weapon], [5, 6, null, 20]);
  assert.deepEqual(profile.sub_abilities.head, ["B", "C", null]);
  assert.equal(profile.hair, 4);
  assert.equal(profile.name, "K");
});

test("without sub-ability support they are left alone", () => {
  const { profile } = applyPreset(base, preset, { ...all, subs: false });
  assert.deepEqual(profile.sub_abilities, base.sub_abilities);
  assert.equal(profile.head, 5);
});

test("an item the game data no longer has is skipped with its sub-abilities", () => {
  const { profile, skipped } = applyPreset(base, preset, { has: (f, id) => !(f === "head" && id === 5), subs: true });
  assert.equal(skipped, 1);
  assert.equal(profile.head, 1);
  assert.deepEqual(profile.sub_abilities.head, ["A", null, null]);
  assert.equal(profile.weapon, 20);
});

test("an ability the game cannot put in a sub slot is emptied", () => {
  const { profile } = applyPreset(base, preset, { ...all, abilities: ["B"] });
  assert.deepEqual(profile.sub_abilities.head, ["B", null, null]);
});

test("the current loadout is recognised", () => {
  const applied = applyPreset(base, preset, all).profile;
  assert.equal(matches(preset, applied, true), true);
  assert.equal(matches(preset, base, true), false);
  // Never edited and three empty slots are the same thing.
  assert.equal(matches(loadoutOf(base), { ...base, sub_abilities: { head: ["A", null, null], clothes: [null, null, null], shoes: null } }, true), true);
  // Sub-abilities are ignored when unsupported.
  assert.equal(matches({ ...loadoutOf(base), sub_abilities: { head: ["Z", null, null], clothes: null, shoes: null } }, base, false), true);
});

test("main abilities are stored, applied and compared when the game supports them", () => {
  const withMain = { ...base, main_abilities: { head: "Own", clothes: null, shoes: null } };
  const p = { ...preset, main_abilities: { head: "M1", clothes: "M2", shoes: "M3" } };
  assert.deepEqual(loadoutOf(withMain).main_abilities, { head: "Own", clothes: null, shoes: null });
  assert.deepEqual(loadoutOf(base).main_abilities, { head: null, clothes: null, shoes: null }, "a profile without the member");
  const { profile } = applyPreset(withMain, p, { ...all, main: true, mainAbilities: { head: ["M1"], clothes: ["Other"] } });
  assert.deepEqual(profile.main_abilities, { head: "M1", clothes: null, shoes: "M3" }, "a name the part cannot take is dropped; no list: kept");
  assert.equal(matches(p, applyPreset(withMain, p, { ...all, main: true }).profile, true, true), true);
  assert.equal(matches({ ...p, head: 1, clothes: 2, shoes: 3, weapon: 10 }, withMain, false, true), false, "same items, other main abilities");
  assert.equal(matches({ ...p, head: 1, clothes: 2, shoes: 3, weapon: 10 }, withMain, false, false), true, "ignored when unsupported");
});

test("without main ability support they are left alone, and an old preset chooses none", () => {
  const withMain = { ...base, main_abilities: { head: "Own", clothes: null, shoes: null } };
  const p = { ...preset, main_abilities: { head: "M1", clothes: null, shoes: null } };
  assert.deepEqual(applyPreset(withMain, p, { ...all, main: false }).profile.main_abilities, withMain.main_abilities);
  assert.deepEqual(applyPreset(withMain, preset, { ...all, main: true }).profile.main_abilities, { head: null, clothes: null, shoes: null });
  // a piece skipped (not in the game data) keeps the profile's main ability with it
  const { profile } = applyPreset(withMain, p, { has: (f, id) => !(f === "head" && id === 5), subs: true, main: true });
  assert.equal(profile.main_abilities.head, "Own");
});

test("names compare trimmed and case-insensitively", () => {
  assert.equal(sameName(" Ranked", "ranked "), true);
  assert.equal(sameName("a", "b"), false);
});
