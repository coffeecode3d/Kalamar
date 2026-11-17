// What the community server asks for wherever the player is in the launcher, once the community page was opened:
// answering a found match, invitations, and a small card of the match being played.
import { useEffect, useRef } from "react";
import { useApp } from "../ctx";
import { Modal } from "../ui";
import { activate, getLive, myId, useLive } from "./store";
import { useC } from "./texts";
import { LaunchQuestion, MatchCard, MatchFound } from "./Match";
import { MatchScreen, screenKey, screenWanted, useReduced } from "./MatchScreen";
import { DraftScreen } from "./Draft";
import { draftWanted } from "./draftState";
import { WeaponWindow } from "./WeaponWindow";
import { weaponWanted } from "./weaponState";
import { useCommunitySounds } from "../sound/events";
import { setSpectating } from "../sound/engine";
import { isSpectator } from "./Stands";
import "./community.css";

export default function CommunityOverlay({ onPage, server, autoConnect }: { onPage: boolean; server?: string; autoConnect?: boolean }) {
  const { c, L } = useC();
  const { go, toast } = useApp();
  const live = useLive();
  const me = myId();
  const seen = useRef(new Set<string>());
  const shownNotice = useRef(0);

  // "Stay connected in the background": the stream starts with the launcher, a moment after its first screen.
  useEffect(() => {
    if (!autoConnect || !server || getLive().server === server) return;
    const t = window.setTimeout(() => void activate(server), 1500);
    return () => window.clearTimeout(t);
  }, [autoConnect, server]);

  // The server's messages for this player (`activity.notice`): one toast each.
  useEffect(() => {
    for (const n of live.notices) {
      if (n.n <= shownNotice.current) continue;
      shownNotice.current = n.n;
      const level = n.notice?.level;
      toast({ kind: level === "error" ? "error" : level === "success" ? "ok" : "info", text: L(n.notice?.text).slice(0, 300) });
    }
  }, [live.notices, toast, L]);
  const m = live.match;
  const mine = m?.participants?.find((p: { user?: { id?: string } }) => p.user?.id === me);
  const r = live.runner && m && live.runner.match_id === m.id ? live.runner : null;
  const [reduced, setReduced] = useReduced();
  const screen = !!r && screenWanted(r) && reduced !== screenKey(r);
  // The window to change weapon before the launch (runner phase `weapon`): over the draft's last screen.
  const weapon = weaponWanted(live.runner, m) ? live.runner : null;
  // 1.1.6: the stage draft, from "everyone accepted" until the game is on its way (then the match screen).
  const draftKey = m ? `${m.id}:draft` : "";
  const drafting = !!m && draftWanted(m) && !(r && screenWanted(r)) && reduced !== draftKey && !weapon;
  useCommunitySounds(m, mine?.accept, live.room, me);
  // 1.1.7: a spectator gets no "your game starts" sound.
  const watching = (!!m && isSpectator(m)) || r?.seat === "spectator";
  useEffect(() => { setSpectating(watching); }, [watching]);
  const mustAnswer = m && m.state === "pending_accept" && mine?.accept === "pending";
  // The launcher's own question before a launch it was not asked for (`proto/consent.rs`), whatever the page shows.
  const ask = live.runner?.phase === "confirm" ? live.runner : null;

  // Invitations: one toast each (they are listed on the page).
  useEffect(() => {
    for (const i of live.roomInvites) {
      const k = `r:${i.room_id}`;
      if (!seen.current.has(k)) {
        seen.current.add(k);
        toast({ kind: "info", text: c("room.invite.from", { name: i.from?.display_name ?? "?", code: i.code ?? "" }) });
      }
    }
    for (const i of live.partyInvites) {
      const k = `p:${i.party_id}`;
      if (!seen.current.has(k)) {
        seen.current.add(k);
        toast({ kind: "info", text: c("p.invite.from", { name: i.from?.display_name ?? "?" }) });
      }
    }
  }, [live.roomInvites, live.partyInvites, c, toast]);

  return (
    <>
      {mustAnswer && (
        <Modal title={c("m.found")} onClose={() => {}} labelledBy="cm-found-title">
          <MatchFound match={m} />
        </Modal>
      )}
      {ask && !mustAnswer && (
        <Modal title={c("m.ask.title")} onClose={() => {}} labelledBy="cm-ask-title">
          <LaunchQuestion r={ask} match={m && m.id === ask.match_id ? m : null} />
        </Modal>
      )}
      {screen && r && <MatchScreen r={r} match={m} onReduce={() => setReduced(screenKey(r))} />}
      {drafting && <DraftScreen match={m} onReduce={() => setReduced(draftKey)} />}
      {weapon && <WeaponWindow key={weapon.match_id} r={weapon} match={m && m.id === weapon.match_id ? m : null} />}
      {!onPage && m && !mustAnswer && (
        <div className="cm-float" role="status">
          <MatchCard match={m} compact />
          <button className="btn btn-small" onClick={() => go("community")}>{c("m.open")}</button>
        </div>
      )}
    </>
  );
}
