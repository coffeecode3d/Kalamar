import test from "node:test";
import assert from "node:assert/strict";
import { chosenMain, clearPart, fillAll, isEdited, mainChoices, mainOf, pickable, setMain, setSlot, slotsOf, withSlots } from "./subSlots.ts";

const base = { schema: 1, name: "K", head: 1, clothes: 2, shoes: 3 };

test("a profile that never edited a part shows three empty slots and is not edited", () => {
  assert.deepEqual(slotsOf(base, "head"), [null, null, null]);
  assert.equal(isEdited(base, "head"), false);
  assert.deepEqual(slotsOf({ ...base, sub_abilities: { head: null, clothes: null, shoes: null } }, "shoes"), [null, null, null]);
});

test("one slot changes, the others and the other parts stay", () => {
  const a = setSlot(base, "head", 1, "Alpha");
  assert.deepEqual(slotsOf(a, "head"), [null, "Alpha", null]);
  assert.equal(isEdited(a, "head"), true);
  assert.equal(isEdited(a, "clothes"), false);
  const b = setSlot(a, "head", 0, "Beta");
  assert.deepEqual(slotsOf(b, "head"), ["Beta", "Alpha", null]);
  assert.deepEqual(slotsOf(setSlot(b, "head", 1, null), "head"), ["Beta", null, null], "Empty in the picker empties the slot");
  assert.equal(setSlot(base, "head", 3, "Alpha"), base, "no fourth slot");
  assert.equal(base.sub_abilities, undefined, "the profile is never changed in place");
});

test("fill all three, and clearing is an edit that stays three nulls", () => {
  const f = fillAll(base, "clothes", "Alpha");
  assert.deepEqual(slotsOf(f, "clothes"), ["Alpha", "Alpha", "Alpha"]);
  const c = clearPart(f, "clothes");
  assert.deepEqual(c.sub_abilities.clothes, [null, null, null]);
  assert.equal(isEdited(c, "clothes"), true, "an explicit empty is sent so that it clears the save's abilities");
  assert.equal(c.sub_abilities.head, null);
});

test("the picker lists each ability once, by name", () => {
  const names = { a1: "Zoom", a2: "Aim", a3: "Aim" };
  const list = pickable(["a1", "a2", "a3", "a1"], (id) => names[id]);
  assert.deepEqual(list.map((x) => x.id), ["a2", "a3", "a1"]);
});

test("a stored list is cut to three slots", () => {
  assert.deepEqual(slotsOf(withSlots(base, "shoes", ["A", "B", "C", "D"]), "shoes"), ["A", "B", "C"]);
});

test("the main slot shows the piece's own until a main ability is chosen", () => {
  assert.equal(chosenMain(base, "head"), null);
  assert.equal(mainOf(base, "head", "Own"), "Own");
  assert.equal(mainOf({ ...base, main_abilities: { head: null, clothes: null, shoes: null } }, "head", "Own"), "Own");
  const m = setMain(base, "head", "Alpha");
  assert.equal(mainOf(m, "head", "Own"), "Alpha");
  assert.equal(chosenMain(m, "clothes"), null, "the other parts stay unchosen");
  assert.equal(base.main_abilities, undefined, "the profile is never changed in place");
});

test("choosing the piece's own main ability is an explicit choice", () => {
  // null would leave the save as an earlier launch left it; the own ability written back undoes that launch's choice
  const back = setMain(setMain(base, "shoes", "Alpha"), "shoes", "Own");
  assert.equal(chosenMain(back, "shoes"), "Own");
  assert.equal(chosenMain(setMain(back, "shoes", null), "shoes"), null);
});

test("a part offers the main abilities its own pieces have, in the enum's order", () => {
  const pieces = [
    { ability: "HeadOnly", selectable: true }, { ability: "Beta", selectable: true }, { ability: "Alpha", selectable: true },
    { ability: "Hidden", selectable: false }, { ability: null, selectable: true }, { ability: "Beta", selectable: true },
  ];
  assert.deepEqual(mainChoices(["Alpha", "Beta", "Gamma", "HeadOnly", "ShoesOnly", "Hidden"], pieces), ["Alpha", "Beta", "HeadOnly"]);
  assert.deepEqual(mainChoices([], pieces), [], "no list from the game: nothing to offer");
  assert.deepEqual(mainChoices(["Alpha"], []), [], "no game data: nothing to offer");
});
