import test from "node:test";
import assert from "node:assert/strict";
import { opensToClose } from "./openGameLogic.ts";

const g = (state, mode, restarted = false) => ({ state, mode, restarted });

test("a game started for solo play is closed when a match starts", () => {
  for (const mode of ["hub", "range", "original", "lan"]) assert.equal(opensToClose(g("running", mode)), true, mode);
});

test("a game that restarted itself is still closed", () => {
  assert.equal(opensToClose(g("exited", "original", true)), true);
});

test("the launcher's own match in progress is not announced as closing", () => {
  assert.equal(opensToClose(g("running", "match")), false);
  assert.equal(opensToClose(g("exited", "match", true)), false);
});

test("no game open: nothing to say", () => {
  assert.equal(opensToClose(g("idle", null)), false);
  assert.equal(opensToClose(g("exited", "hub")), false);
});
