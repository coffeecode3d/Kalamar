import test from "node:test";
import assert from "node:assert/strict";
import { weaponWanted, windowFraction, windowLeft } from "./weaponState.ts";

const runner = (over = {}) => ({ match_id: "m-1", phase: "weapon", seat: "player", weapon_until: "2026-11-16T12:00:20.000Z", ...over });
const match = (over = {}) => ({ id: "m-1", state: "ready", you: { team: "alpha" }, ...over });

test("the panel is shown to a player while the runner waits in phase weapon, never to a spectator", () => {
  assert.equal(weaponWanted(runner(), match()), true);
  assert.equal(weaponWanted(runner(), null), true, "the match object is not needed");
  for (const phase of ["checking", "confirm", "choosing", "resolving", "launching", "running", "closed"]) {
    assert.equal(weaponWanted(runner({ phase }), match()), false, phase);
  }
  assert.equal(weaponWanted(null, match()), false);
  assert.equal(weaponWanted(runner({ seat: "spectator" }), match()), false);
  assert.equal(weaponWanted(runner(), match({ you: { team: "spectator" } })), false);
  assert.equal(weaponWanted(runner(), match({ you: { team: "alpha", seat: "spectator" } })), false);
  // Another match's spectator seat says nothing about this runner's match.
  assert.equal(weaponWanted(runner(), match({ id: "m-0", you: { team: "spectator" } })), true);
});

test("the countdown runs on the server's clock, rounded up, and stops at zero", () => {
  const end = Date.parse("2026-11-16T12:00:20.000Z");
  assert.equal(windowLeft("2026-11-16T12:00:20.000Z", end - 20_000), 20);
  assert.equal(windowLeft("2026-11-16T12:00:20.000Z", end - 19_001), 20);
  assert.equal(windowLeft("2026-11-16T12:00:20.000Z", end - 500), 1);
  assert.equal(windowLeft("2026-11-16T12:00:20.000Z", end), 0);
  assert.equal(windowLeft("2026-11-16T12:00:20.000Z", end + 3_000), 0);
  assert.equal(windowLeft("2026-11-16T12:00:20Z", end - 5_000), 5, "whole seconds as well");
  for (const bad of [undefined, null, "", "soon", 20]) assert.equal(windowLeft(bad, end), null, String(bad));
});

test("the ring empties from 1 to 0 over the window", () => {
  assert.equal(windowFraction(20, 20), 1);
  assert.equal(windowFraction(5, 20), 0.25);
  assert.equal(windowFraction(0, 20), 0);
  assert.equal(windowFraction(25, 20), 1, "never above 1");
  assert.equal(windowFraction(3, 0), 0, "no length known");
});
