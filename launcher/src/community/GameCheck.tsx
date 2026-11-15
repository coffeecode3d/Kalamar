// Is the installed game recent enough for the automatic match? Asked of the backend (`proto_game_check`: the engine's
// capability list, read once per executable, and the commit the game was compiled from against the embedded runtime's).
// A game proven too old blocks "ready", "start" and "accept" and points to the recompilation; a game older than the
// embedded runtime only gets a warning.
import { useEffect, useSyncExternalStore } from "react";
import { useApp } from "../ctx";
import { proto } from "./api";
import { useC } from "./texts";
import { gameBlocks, gameWarns, type GameCheck } from "./waiting";
import { startUpdate, useUpdateCheck } from "../install/UpdateNotice";
import { approx } from "../install/texts";

let check: GameCheck | null = null;
let asking = false;
const subs = new Set<() => void>();

function refresh() {
  if (asking) return;
  asking = true;
  void proto.gameCheck()
    .then((v) => { check = v; subs.forEach((f) => f()); })
    .catch(() => {})
    .finally(() => { asking = false; });
}

/** The game's check, read when a component needing it appears and when the launcher window gets the focus back (after a
 *  recompilation, for example). `null` until the first answer. */
export function useGameCheck(): GameCheck | null {
  useEffect(() => {
    refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, []);
  return useSyncExternalStore((f) => { subs.add(f); return () => subs.delete(f); }, () => check);
}

/** True when the installed game cannot take part in the automatic match. */
export function useGameBlocked(): boolean {
  return gameBlocks(useGameCheck());
}

/** The "recompile your game" message: blocking (error) for a game too old, a plain notice when a recompilation is only
 *  available (with how long it takes; its button opens that update's plan). While an update of the game runs, the
 *  player plays the installed version: a match that needs the new one will not start before it ends, said plainly. */
export function GameCheckNotice() {
  const { c } = useC();
  const { go, install } = useApp();
  const g = useGameCheck();
  const upd = useUpdateCheck();
  const open = () => (upd?.available ? startUpdate(upd.install_dir, go) : go("setup"));
  const dur = upd?.available ? ` (${approx(upd.eta)})` : "";
  // The installed game is the previous game version: the same "game update needed" state, with its own words.
  const other = g?.old_version ? { from: g.old_version, to: g.needed_version } : null;
  if (install?.active && install.update) {
    return (
      <div className="notice notice-info cm-old" role="status">
        <p className="small">{c("m.update.running", { pct: Math.floor(install.overall * 100) })}</p>
        <button className="btn btn-small btn-ghost" onClick={() => go("setup")}>{c("m.update.see")}</button>
      </div>
    );
  }
  if (gameBlocks(g)) {
    return (
      <div className="notice notice-error cm-old" role="alert">
        <strong>{other ? c("m.ver.title", other) : c("m.old.title")}</strong>
        <p className="small">{other ? c("m.ver.body", other) : c("m.old.body")}</p>
        <button className="btn btn-small" onClick={open}>{other ? c("m.ver.btn", other) : c("m.old.btn")}{dur}</button>
      </div>
    );
  }
  if (gameWarns(g)) {
    return (
      <div className="notice notice-info cm-old">
        <p className="small">{c("m.old.warn2")}</p>
        <button className="btn btn-small btn-ghost" onClick={open}>{c("m.old.btn")}{dur}</button>
      </div>
    );
  }
  return null;
}
