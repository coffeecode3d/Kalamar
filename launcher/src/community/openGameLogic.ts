// A game the player opened for solo play is closed by the launcher when a match starts (`proto/runner.rs`, phase
// `closing`): the match needs a fresh game with its own environment. The launcher's own match in progress is not.
import type { GameStatus } from "../api";

/** The game that is open now would be closed when a match starts: running (or restarted into its second process) and
 *  started for anything but a match. */
export function opensToClose(game: Pick<GameStatus, "state" | "restarted" | "mode">): boolean {
  return (game.state === "running" || game.restarted) && game.mode !== "match";
}
