import test from "node:test";
import assert from "node:assert/strict";
import { IDLE, launcherState, plan, restartBlock } from "./prompt.ts";

const avail = (over = {}) => ({ version: "0.4.0", channel: "stable", notes: "n", published: null, size: 1e7, mode: "swap", why: null, ...over });
const view = (over = {}) => ({
  enabled: true, current: "0.3.0", auto_check: true, channel: "stable", last_check: 1, checking: false, check_error: null,
  available: avail(), later: false, phase: "idle", progress: null, error: null, blocked: null, saved_to: null, notice: null, ...over,
});
const base = { launcher: view(), game: null, page: "play", busy: IDLE, dialogDone: false };
const p = (over) => plan({ ...base, ...over });
const PAGES = ["play", "profile", "community", "settings", "setup"];

test("a launcher update shows the bar on every page, with its state", () => {
  for (const page of PAGES) {
    assert.deepEqual(p({ page }).bar, { source: "launcher", state: "available", compact: false }, page);
  }
  const states = [
    [{ phase: "downloading", progress: { done: 1, total: 2 } }, "downloading"],
    [{ phase: "ready" }, "ready"],
    [{ phase: "ready", available: avail({ mode: "installer" }) }, "ready"],
    [{ phase: "ready", available: avail({ mode: "elsewhere", why: "temp" }) }, "elsewhere"],
    [{ phase: "applying" }, "working"],
    [{ phase: "restarting" }, "working"],
    [{ phase: "saved", saved_to: "C:\\x" }, "saved"],
    [{ phase: "idle", error: "network" }, "error"],
    [{ available: null, notice: { kind: "updated", from: "0.3.0", to: "0.4.0" } }, "notice"],
    [{ phase: "ready", error: "blocked_game" }, "ready"],
  ];
  for (const [over, state] of states) {
    for (const page of PAGES) assert.equal(p({ launcher: view(over), page }).bar?.state, state, `${state} on ${page}`);
  }
});

test("no bar without an update, in a build without the feature, or before the first answer", () => {
  assert.equal(p({ launcher: null }).bar, null);
  assert.equal(p({ launcher: view({ enabled: false }) }).bar, null);
  assert.equal(p({ launcher: view({ available: null }) }).bar, null);
  assert.equal(p({ launcher: view({ available: null, check_error: "network" }) }).bar, null, "a failed check stays silent");
  assert.deepEqual(p({ launcher: view({ enabled: false }) }), { bar: null, dialog: false, badge: null });
});

test("ready waits while a game, an installation or a relay runs", () => {
  const ready = view({ phase: "ready" });
  assert.equal(launcherState(ready, IDLE), "ready");
  assert.equal(launcherState(ready, { ...IDLE, game: true }), "blocked");
  assert.equal(launcherState(ready, { ...IDLE, install: true }), "blocked");
  assert.equal(launcherState(view({ phase: "ready", blocked: "relay" }), IDLE), "blocked");
  assert.equal(launcherState(ready, { ...IDLE, match: true }), "ready", "a search alone does not block (no game runs yet)");
  assert.equal(restartBlock(ready, { game: true, install: true, match: false }), "game");
  assert.equal(restartBlock(ready, { game: false, install: true, match: false }), "install");
  assert.equal(restartBlock(view({ blocked: "relay" }), IDLE), "relay");
  assert.equal(restartBlock(ready, IDLE), null);
});

test("Later hides the bar and the dialog, leaves a badge on the other pages, and never hides a failure or a download", () => {
  const later = view({ later: true });
  for (const page of PAGES) {
    const r = p({ launcher: later, page });
    assert.equal(r.bar, null, page);
    assert.equal(r.dialog, false, page);
    assert.equal(r.badge, page === "settings" ? null : "about", page);
  }
  assert.equal(p({ launcher: view({ later: true, error: "network" }) }).bar?.state, "error");
  assert.equal(p({ launcher: view({ later: true, phase: "downloading" }) }).bar?.state, "downloading");
  assert.equal(p({ launcher: view({ later: true, phase: "ready" }) }).bar?.state, "ready");
  assert.equal(p({ launcher: view({ later: true, phase: "ready" }) }).badge, null, "being downloaded, it is not put off");
});

test("the start dialog: once, only when idle, never over a match, an installation, a game or the wizard", () => {
  for (const page of ["play", "profile", "community", "settings"]) assert.equal(p({ page }).dialog, true, page);
  assert.equal(p({ page: "setup" }).dialog, false);
  assert.equal(p({ dialogDone: true }).dialog, false, "answered once in this run");
  for (const k of ["game", "install", "match"]) {
    const r = p({ busy: { ...IDLE, [k]: true } });
    assert.equal(r.dialog, false, k);
    assert.equal(r.bar?.state === "available" || r.bar?.state === "ready", true, `the bar stays (${k})`);
  }
  // it appears once the launcher is idle again
  assert.equal(p({ busy: { ...IDLE, match: false } }).dialog, true);
  // only an update waiting for the player's answer asks
  for (const over of [{ phase: "downloading" }, { phase: "ready" }, { error: "network" }, { later: true }, { available: null }, { notice: { kind: "updated", from: "a", to: "b" } }]) {
    assert.equal(p({ launcher: view(over) }).dialog, false, JSON.stringify(over));
  }
});

test("during a match the bar is compact", () => {
  assert.equal(p({ busy: { ...IDLE, match: true } }).bar?.compact, true);
  assert.equal(p({ busy: { ...IDLE, game: true } }).bar?.compact, false);
  assert.equal(p({ busy: { ...IDLE, match: true }, launcher: view({ phase: "downloading" }) }).bar?.compact, true);
});

test("the game update shows on the other pages, after the launcher's", () => {
  const game = { kind: "available", putOff: false };
  const needed = { kind: "needed", putOff: false };
  // alone: every page but Play, which carries it in full
  for (const page of PAGES) {
    const r = p({ launcher: view({ available: null }), game, page });
    assert.deepEqual(r.bar, page === "play" ? null : { source: "game", state: "available", compact: false }, page);
  }
  assert.equal(p({ launcher: null, game: needed, page: "community" }).bar?.state, "needed");
  assert.equal(p({ launcher: null, game: needed, page: "play" }).bar, null);
  // with a launcher update, the launcher's comes first
  for (const page of PAGES) assert.equal(p({ game, page }).bar?.source, "launcher", page);
  // the launcher's put off: the game's shows (and the badge says the launcher's is waiting)
  const r = p({ launcher: view({ later: true }), game, page: "profile" });
  assert.equal(r.bar?.source, "game");
  assert.equal(r.badge, "about");
  // the game's put off: nothing on the bar, a badge pointing to its section
  const off = p({ launcher: null, game: { kind: "available", putOff: true }, page: "profile" });
  assert.deepEqual(off, { bar: null, dialog: false, badge: "installed" });
  assert.equal(p({ launcher: null, game: { kind: "available", putOff: true }, page: "settings" }).badge, null);
  // an installation has the game check answer nothing; if one runs, no game bar
  assert.equal(p({ launcher: null, game, page: "community", busy: { ...IDLE, install: true } }).bar, null);
  // compact in a match, for the game too
  assert.equal(p({ launcher: null, game, page: "community", busy: { ...IDLE, match: true } }).bar?.compact, true);
  // the game's bar never asks with the dialog
  assert.equal(p({ launcher: null, game: needed, page: "profile" }).dialog, false);
});
