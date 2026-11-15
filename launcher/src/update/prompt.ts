// Which update prompt shows, where and when: the bar at the top of every page, the dialog at the start, the badge on the
// rail's Settings entry. Pure logic (no React, no backend call): the components feed it the launcher's update view, the
// game's update check, the page and what the launcher is busy with, and draw the answer. Tested in prompt.test.mjs.
//
// The rules:
// - A launcher update comes first; the game update (the Play page's "Game update available" / "Game update needed")
//   shows only when there is no launcher bar. Why this order: the new launcher usually carries the new engine code and
//   sometimes the new game version, so what the game update would do is only known once the launcher is up to date.
// - "Later" (the bar's or the dialog's) is kept for this run of the launcher only. It hides the bar and leaves the
//   badge on Settings, so the update stays one click away. A failure, a download or a result is never hidden by it.
// - The start dialog asks once per run, never over a match, an installation or a running game, nor over the install
//   wizard; it waits until the launcher is idle again.
// - During a match the bar is compact (one line, no state text) so it never takes more room than a line.
import type { UpdateView } from "../api";

export type Page = "play" | "profile" | "community" | "settings" | "setup";

/** What the launcher is busy with. `match`: a match is being found, launched or played (a search, a found match, the
 *  private match connecting, a command-line launch), `game`: a game process runs, `install`: an installation runs. */
export interface Busy { game: boolean; install: boolean; match: boolean }

export const IDLE: Busy = { game: false, install: false, match: false };

/** The installed game against the launcher's code, as far as the prompt needs it. */
export interface GameFacts {
  /** `needed`: the installed game is the previous game version; `available`: other engine code than the installed. */
  kind: "needed" | "available";
  /** "Later" was pressed for this code in this run. */
  putOff: boolean;
}

export type LauncherState =
  | "notice" // the result of the last update: updated, put back, installer unfinished
  | "available" // Update / Later
  | "error" // the download failed: Try again
  | "downloading"
  | "ready" // Restart to finish (or Start the installation)
  | "blocked" // ready, but a game, an installation or a relay runs
  | "elsewhere" // ready, but this copy cannot replace itself: choose a folder
  | "working" // applying, restarting
  | "saved";

export type Bar =
  | { source: "launcher"; state: LauncherState; compact: boolean }
  | { source: "game"; state: "available" | "needed"; compact: boolean };

export interface Plan {
  bar: Bar | null;
  /** The start dialog. */
  dialog: boolean;
  /** Something is put off: a dot on the Settings entry, and the Settings section to open from it. */
  badge: "about" | "installed" | null;
}

export interface Input {
  launcher: UpdateView | null;
  game: GameFacts | null;
  page: Page;
  busy: Busy;
  /** The start dialog was answered in this run. */
  dialogDone: boolean;
}

/** Why "Restart to finish" waits, if it does. The game and the installation are followed live by the page; the relay
 *  only by the backend's last answer (which checks all three again when the button is pressed). */
export function restartBlock(v: Pick<UpdateView, "blocked">, busy: Busy): "game" | "install" | "relay" | null {
  return busy.game ? "game" : busy.install ? "install" : v.blocked === "relay" ? "relay" : null;
}

const anyBusy = (b: Busy) => b.game || b.install || b.match;

/** The launcher's bar state, or `null` when it shows none (nothing on offer, or put off with "Later"). */
export function launcherState(v: UpdateView | null, busy: Busy): LauncherState | null {
  if (!v || !v.enabled) return null;
  if (v.notice) return "notice";
  const a = v.available;
  if (!a) return null;
  switch (v.phase) {
    case "downloading": return "downloading";
    case "applying":
    case "restarting": return "working";
    case "saved": return "saved";
    case "ready":
      if (a.mode === "elsewhere") return "elsewhere";
      return restartBlock(v, busy) ? "blocked" : "ready";
    default:
      if (v.error) return "error";
      return v.later ? null : "available";
  }
}

export function plan(i: Input): Plan {
  const { launcher: v, game, page, busy } = i;
  const state = launcherState(v, busy);
  const compact = busy.match;
  let bar: Bar | null = state ? { source: "launcher", state, compact } : null;
  // The Play page carries the game update in full (its duration, when to run it); the bar is for the other pages.
  if (!bar && game && !game.putOff && page !== "play" && !busy.install) {
    bar = { source: "game", state: game.kind, compact };
  }
  const offered = !!v && v.enabled && !!v.available;
  const dialog = offered && v!.phase === "idle" && !v!.later && !v!.error && !v!.notice
    && !i.dialogDone && !anyBusy(busy) && page !== "setup";
  let badge: Plan["badge"] = null;
  if (page !== "settings") {
    if (offered && v!.phase === "idle" && v!.later && !v!.error) badge = "about";
    else if (!state && game?.putOff) badge = "installed";
  }
  return { bar, dialog, badge };
}

// ---- the start dialog's memory (this run only) --------------------------------------------------------------------
let dialogAnswered = false;
export const dialogDone = () => dialogAnswered;
export function markDialogDone() { dialogAnswered = true; }
