// The notice shown while the player waits for a match (searching, match found, in a room) with a game open for solo
// play: that game is closed when the match starts.
import { useApp } from "../ctx";
import { opensToClose } from "./openGameLogic";
import { useC } from "./texts";

export function OpenGameNotice() {
  const { c } = useC();
  const { game } = useApp();
  return opensToClose(game) ? <p className="notice notice-info small" role="status">{c("m.open_game")}</p> : null;
}
