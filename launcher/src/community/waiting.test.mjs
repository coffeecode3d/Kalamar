import test from "node:test";
import assert from "node:assert/strict";
import { SLOW_AFTER_S, gameBlocks, gameWarns, missingNames, slowPlayers, stateKey, stepTimes, trackLaunched, windowHidden } from "./waiting.ts";

const me = "u_me";
const host = { user: { id: me }, member_name: "Camille", launched: true, status: { state: "waiting-for-players" } };
const guest = (extra = {}) => ({ user: { id: "u_b" }, member_name: "Lou", launched: true, status: null, ...extra });

test("one sentence per known state, the room differs for the host and the guest", () => {
  for (const s of ["booting", "lobby", "creating-room", "waiting-for-players", "joining", "loading-stage", "in-match", "results", "finished", "error", "waiting-command"]) {
    assert.equal(stateKey(s, "host"), `m.wait.${s}`);
  }
  assert.equal(stateKey("in-room", "host"), "m.wait.in-room.host");
  assert.equal(stateKey("in-room", "join"), "m.wait.in-room.join");
  assert.equal(stateKey("in-room", null), "m.wait.in-room.join");
  assert.equal(stateKey("something-new", "host"), null);
  assert.equal(stateKey(null, "host"), null);
});

test("elapsed time and time left before the engine gives up", () => {
  const g = { state: "waiting-for-players", detail: null, since_ms: 1_000_000, present: 1, expected: 2, visible: false, limit_s: 300 };
  assert.deepEqual(stepTimes(g, 1_000_000 + 47_900), { elapsed: 47, left: 253 });
  assert.deepEqual(stepTimes(g, 1_000_000 + 400_000), { elapsed: 400, left: 0 });
  assert.deepEqual(stepTimes(g, 1_000_000 - 5_000), { elapsed: 0, left: 300 });
  // No limit known for the step (lobby, booting): the elapsed time only.
  assert.deepEqual(stepTimes({ ...g, limit_s: null }, 1_090_000), { elapsed: 90, left: null });
  // No start time in the status: nothing is claimed.
  assert.deepEqual(stepTimes({ ...g, since_ms: null }, 5), { elapsed: null, left: null });
  assert.deepEqual(stepTimes(null, 5), { elapsed: null, left: null });
});

test("who is missing: the others whose game is not in the session", () => {
  assert.deepEqual(missingNames([host, guest()], me), ["Lou"]);
  assert.deepEqual(missingNames([host, guest({ status: { state: "booting" } })], me), ["Lou"]);
  assert.deepEqual(missingNames([host, guest({ status: { state: "in-room" } })], me), []);
  const third = { user: { id: "u_c", display_name: "Max" }, launched: false, status: null };
  assert.deepEqual(missingNames([host, guest(), third], me), ["Lou", "Max"]);
  assert.deepEqual(missingNames(undefined, me), []);
});

test("the window stays hidden until the match's intro", () => {
  assert.equal(windowHidden({ state: "waiting-for-players", visible: false }, null), true);
  assert.equal(windowHidden(null, "lobby"), true);
  assert.equal(windowHidden({ state: "in-match", visible: true }, null), false);
  assert.equal(windowHidden({ state: "results", visible: false }, null), false);
  assert.equal(windowHidden({ state: "loading-stage", visible: true }, null), false);
  assert.equal(windowHidden(null, null), false);
});

test("a player stuck at launched while the host waits gets a help message after 90 s", () => {
  const t0 = 10_000_000;
  let seen = trackLaunched({}, [host, guest()], me, t0);
  assert.deepEqual(seen, { u_b: t0 });
  // The first time seen is kept.
  seen = trackLaunched(seen, [host, guest()], me, t0 + 60_000);
  assert.equal(seen.u_b, t0);
  assert.deepEqual(slowPlayers(seen, [host, guest()], "waiting-for-players", t0 + (SLOW_AFTER_S - 1) * 1000), []);
  assert.deepEqual(slowPlayers(seen, [host, guest()], "waiting-for-players", t0 + SLOW_AFTER_S * 1000), ["Lou"]);
  // Only while this game waits for the players.
  assert.deepEqual(slowPlayers(seen, [host, guest()], "in-room", t0 + 200_000), []);
  assert.deepEqual(slowPlayers(seen, [host, guest()], null, t0 + 200_000), []);
  // A player that reported a state is dropped from the tracking.
  assert.deepEqual(trackLaunched(seen, [host, guest({ status: { state: "booting" } })], me, t0 + 100_000), {});
  // So is one that has not launched yet.
  assert.deepEqual(trackLaunched({}, [host, guest({ launched: false })], me, t0), {});
});

test("the check blocks only a game proven too old, and warns for a recompilation", () => {
  const c = (level, outdated) => ({ level, outdated, installed_commit: null, bundled_commit: null, note: null, old_version: null, needed_version: "11.3.0" });
  assert.equal(gameBlocks(c("too_old", true)), true);
  assert.equal(gameBlocks(c("unknown", false)), false);
  assert.equal(gameBlocks(c("ok", true)), false);
  assert.equal(gameBlocks(null), false);
  assert.equal(gameWarns(c("ok", true)), true);
  assert.equal(gameWarns(c("ok", false)), false);
  assert.equal(gameWarns(c("too_old", true)), false);
  assert.equal(gameWarns(null), false);
});
