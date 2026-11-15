import test from "node:test";
import assert from "node:assert/strict";
import { clearPart, fillAll, isEdited, pickable, setSlot, slotsOf, withSlots } from "./subSlots.ts";

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
