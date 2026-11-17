import test from "node:test";
import assert from "node:assert/strict";
import { rankOf } from "./rank.ts";

const tier = { id: "gold", name: { en: "Gold" }, color: "#D4AF37", order: 3 };
const placed = (extra = {}) => ({ ladder: "duel", value: 1423, display: "1423", tier, position: 5, provisional: null, ...extra });

test("a placed and listed player: position, tier and rating", () => {
  assert.deepEqual(rankOf(placed()), { placement: false, remaining: null, position: 5, positionText: "#5", tier, rating: "1423" });
  assert.equal(rankOf(placed({ position: 12 })).positionText, "#12");
});

test("no position: the tier and the rating only", () => {
  const r = rankOf(placed({ position: null }));
  assert.equal(r.positionText, null);
  assert.equal(r.position, null);
  assert.equal(r.tier, tier);
  assert.equal(r.rating, "1423");
});

test("the rating text is the display, else the value, else nothing", () => {
  assert.equal(rankOf(placed({ display: "1 423" })).rating, "1 423");
  assert.equal(rankOf(placed({ display: null })).rating, "1423");
  assert.equal(rankOf(placed({ display: "  " })).rating, "1423");
  assert.equal(rankOf(placed({ display: null, value: null })).rating, null);
  assert.equal(rankOf(placed({ display: null, value: 0 })).rating, "0");
});

test("a player in placement shows no position, tier nor rating", () => {
  const r = rankOf({ ladder: "duel", value: null, display: null, tier: null, position: null, provisional: { remaining: 2 } });
  assert.deepEqual(r, { placement: true, remaining: 2, position: null, positionText: null, tier: null, rating: null });
  // Whatever else the server sent with it is ignored.
  assert.equal(rankOf({ provisional: { remaining: 1 }, position: 3, value: 9, tier }).positionText, null);
  // The games left may be unknown.
  assert.equal(rankOf({ provisional: {} }).remaining, null);
  assert.equal(rankOf({ provisional: { remaining: 0 } }).remaining, 0);
});

test("nothing when the rating is absent, empty or malformed", () => {
  for (const bad of [undefined, null, 5, "1423", [], {}, { ladder: "duel", value: null, display: null, tier: null, position: null, provisional: null }]) {
    assert.equal(rankOf(bad), null, JSON.stringify(bad));
  }
  assert.equal(rankOf(placed({ position: 0 })), null);
  assert.equal(rankOf(placed({ position: -3 })), null);
  assert.equal(rankOf(placed({ position: "5" })), null);
  assert.equal(rankOf(placed({ position: 2.5 })), null);
  assert.equal(rankOf(placed({ position: Infinity })), null);
  assert.equal(rankOf(placed({ position: NaN })), null);
  assert.equal(rankOf(placed({ value: "1423" })), null);
  assert.equal(rankOf(placed({ value: NaN })), null);
  assert.equal(rankOf(placed({ display: 1423 })), null);
  assert.equal(rankOf(placed({ tier: "gold" })), null);
  assert.equal(rankOf(placed({ tier: {} })), null);
  assert.equal(rankOf(placed({ provisional: "yes" })), null);
  assert.equal(rankOf(placed({ provisional: { remaining: "2" } })), null);
  assert.equal(rankOf(placed({ provisional: { remaining: -1 } })), null);
});
