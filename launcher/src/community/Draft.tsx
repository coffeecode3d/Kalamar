// The stage draft of a match (protocol 1.1.6, `draft.md`): a full-frame screen between "match found" and the launch,
// drawn from the server's draft object only. Both teams with their players (captain, the player's own team's votes),
// the stages as the game shows them (banned ones in the color of the team that banned them, the chosen one in front),
// whose turn it is in large with a countdown ring, an action only for the player who may act, the turn log, then the
// stage retained while the game gets ready. The server decides everything; this screen never blocks the match.
import { useEffect, useRef, useState } from "react";
import { play } from "../sound/engine";
import { BRAND } from "../brand";
import { IconBan, IconCheck, IconClose, IconCrown, IconShuffle } from "../icons";
import { StageCard, StageTag } from "../stages";
import { Ring, clock } from "../match/StageLayout";
import type { J } from "./api";
import { applyMatch, myId, req, serverNow, useLive } from "./store";
import { useC, type CFn, type CKey } from "./texts";
import { ErrorLine, RankBadge, stageFallback, useAction, useModeName, useStageName } from "./ui";
import { acted as actedTeams, actingTeams, currentTurn, headline, idsOf, log, markOf, needed, selectable, toggle, turnClock, usable, type Headline, type Team } from "./draftState";
import { isSpectator } from "./Stands";
import "./draft.css";

const teamKey = (t: Team): CKey => (t === "alpha" ? "d.alpha" : "d.bravo");

/** "a stage", "2 stages", "a mode"... */
function what(c: CFn, kind: string, n: number): string {
  return c(`d.what.${kind === "mode" ? "mode" : "stage"}.${n > 1 ? "many" : "one"}` as CKey, { n });
}

/** The headline as a title and a sentence. */
function words(c: CFn, h: Headline, name: (id: string) => string, d: J): { title: string; line: string } {
  switch (h.kind) {
    case "you": {
      const w = what(c, h.what, h.n);
      const verb = h.action === "pick" ? "pick" : "ban";
      return { title: c("d.title.you"), line: c(h.vote ? (`d.you.vote.${verb}` as CKey) : (`d.you.${verb}` as CKey), { what: w }) };
    }
    case "team": {
      const w = what(c, h.what, h.n);
      return { title: c("d.title.team"), line: h.captain ? c(h.action === "pick" ? "d.captain.pick" : "d.captain.ban", { name: name(h.captain), what: w }) : c("d.team.wait") };
    }
    case "them": {
      const w = what(c, h.what, h.n);
      if (h.team === "both") return { title: c("d.title.both"), line: c(h.action === "pick" ? "d.them.both.pick" : "d.them.both.ban", { what: w }) };
      const team = c(teamKey(h.team));
      return { title: c("d.title.them", { team }), line: c(h.action === "pick" ? "d.them.pick" : "d.them.ban", { team, what: w }) };
    }
    case "hidden":
      return { title: c("d.title.team"), line: h.acted ? c("d.hidden.done") : c("d.hidden.wait", { what: what(c, h.what, h.n) }) };
    case "server":
      return { title: c("d.title.server"), line: h.action === "last" ? c("d.server.last") : c("d.server.random") };
    case "done":
      return { title: c("d.title.done"), line: c("d.launching") };
    default:
      return { title: c("d.title.waiting"), line: d?.state === "waiting" ? c("d.waiting") : c("d.title.waiting") };
  }
}

/** The countdown of the current turn, by the server's clock; re-renders every 250 ms while a turn runs. */
function useTurnClock(d: J): { left: number; fraction: number } | null {
  const [, tick] = useState(0);
  const running = d?.state === "running" && d?.current != null;
  useEffect(() => {
    if (!running) return;
    const id = window.setInterval(() => tick((n) => n + 1), 250);
    return () => window.clearInterval(id);
  }, [running]);
  return running ? turnClock(d, serverNow()) : null;
}

/** One team: its players (the captain crowned, the player marked, their votes for the player's own team), whether it
 *  plays now, whether the server plays for it, and what it banned. */
function TeamPanel({ team, d, names, ratings, me, stageName }: { team: Team; d: J; names: Record<string, string>; ratings: Record<string, J>; me: string | null; stageName: (id: string) => string }) {
  const { c } = useC();
  const t = d.teams?.[team] ?? {};
  const players: string[] = (Array.isArray(t.players) ? t.players : []).filter((x: unknown) => typeof x === "string").slice(0, 8);
  const votes: Record<string, string[]> = {};
  for (const v of Array.isArray(t.votes) ? t.votes : []) if (typeof v?.user_id === "string" && Array.isArray(v.ids)) votes[v.user_id] = v.ids;
  const acting = actingTeams(d).includes(team);
  const chose = actedTeams(d).includes(team);
  const bans = (Array.isArray(d.options) ? d.options : []).filter((o: J) => markOf(o) === "banned" && o.by === team && o.kind === "stage").map((o: J) => o.id as string);
  return (
    <aside className={`dr-team t-${team} ${acting && !chose ? "dr-acting" : ""}`} aria-label={c(teamKey(team))}>
      <header className="dr-team-head">
        <span className="team-name">{c(teamKey(team))}</span>
        {d.first === team && <span className="dr-first">{c("d.first")}</span>}
      </header>
      <ul className="dr-players">
        {players.map((u) => (
          <li key={u} className={`dr-player ${u === me ? "me" : ""}`}>
            <span className="st-avatar" aria-hidden="true">{(names[u] ?? "?").slice(0, 1).toUpperCase()}</span>
            <span className="dr-player-id">
              <span className="dr-player-name">{names[u] ?? "?"}{u === me && <span className="st-you"> {c("m.screen.you")}</span>}</span>
              <RankBadge rating={ratings[u]} />
            </span>
            {t.captain === u && <IconCrown className="dr-crown" aria-label={c("d.captain")} />}
            {d.decider === "vote" && Array.isArray(t.votes) && (
              <span className="dr-vote">{votes[u] ? votes[u].map((id) => stageName(id)).join(", ") : c("d.vote.none")}</span>
            )}
          </li>
        ))}
      </ul>
      <p className="dr-team-state">
        {t.auto ? c("d.team.auto") : acting ? (chose ? <><IconCheck /> {c("d.team.chose")}</> : c("d.team.acting")) : " "}
      </p>
      {bans.length > 0 && (
        <div className="dr-bans">
          <span className="dr-bans-title"><IconBan /> {c("d.bans")}</span>
          {bans.map((id: string) => <StageTag key={id} id={id} fallback={stageName(id)} />)}
        </div>
      )}
    </aside>
  );
}

/** The turns in order: done ones with what was chosen and how, the current one, the ones to come. */
function TurnLog({ d, stageName, modeName }: { d: J; stageName: (id: string) => string; modeName: (id: string) => string }) {
  const { c } = useC();
  const lines = log(d);
  const turns: J[] = Array.isArray(d.turns) ? d.turns.slice(0, 64) : [];
  const nameOf = (kind: string, id: string) => (kind === "mode" ? modeName(id) : stageName(id));
  return (
    <ol className="dr-log" aria-label={c("d.log")}>
      {turns.map((t: J, i: number) => {
        const index = typeof t.index === "number" ? t.index : i;
        const teams: Team[] = (Array.isArray(t.teams) ? t.teams : []).filter((x: string) => x === "alpha" || x === "bravo");
        const mine = lines.filter((l) => l.index === index);
        const label = t.action === "ban" ? c("d.log.ban") : t.action === "pick" ? c("d.log.pick") : t.action === "random" ? c("d.log.random") : t.action === "last" ? c("d.log.last") : String(t.action ?? "?").slice(0, 24);
        return (
          <li key={index} className={`dr-log-turn dr-turn-${t.state === "current" ? "now" : t.state === "done" || t.state === "skipped" ? "done" : "next"}`} aria-current={t.state === "current" ? "step" : undefined}>
            <span className="dr-log-n">{index + 1}</span>
            <span className="dr-log-what">
              {teams.map((x) => <span key={x} className={`dr-log-team t-${x}`}>{c(teamKey(x))}</span>)}
              {teams.length === 0 && <IconShuffle className="dr-log-ico" />}
              <span>{label}{t.kind === "mode" ? ` · ${c("d.modes")}` : ""}</span>
            </span>
            <span className="dr-log-result">
              {t.state === "skipped" && <span className="muted">{c("d.log.skipped")}</span>}
              {mine.filter((l) => !l.skipped).map((l, k) => (
                <span key={k} className={l.team ? `t-${l.team}` : ""}>
                  {l.ids.length ? l.ids.map((id) => nameOf(l.kind, id)).join(", ") : c("d.log.nothing")}
                  {l.by === "timeout" && <em className="dr-log-by"> · {c("d.by.timeout")}</em>}
                  {l.by === "vote" && <em className="dr-log-by"> · {c("d.by.vote")}</em>}
                  {l.by === "server" && l.team && <em className="dr-log-by"> · {c("d.by.server")}</em>}
                </span>
              ))}
              {t.state === "current" && <span className="dr-log-live">{c("d.log.now")}</span>}
            </span>
          </li>
        );
      })}
    </ol>
  );
}

export function DraftScreen({ match, onReduce }: { match: J; onReduce: () => void }) {
  const { c, L } = useC();
  const live = useLive();
  const meta = live.overview?.meta;
  const me = myId();
  const d = match.draft;
  const a = useAction();
  const [picked, setPicked] = useState<string[]>([]);
  const [leaving, setLeaving] = useState(false);
  const stageNameOf = useStageName();
  const modeNameOf = useModeName();
  const stageName = (id: string) => stageNameOf(id, meta) || id;
  const modeName = (id: string) => modeNameOf(id, meta) || id;
  const names: Record<string, string> = {};
  const ratings: Record<string, J> = {};
  for (const p of Array.isArray(match.participants) ? match.participants : []) if (p?.user?.id) { names[p.user.id] = String(p.member_name ?? p.user.display_name ?? "?").slice(0, 32); ratings[p.user.id] = p.rating; }
  const h = headline(d);
  const turn = currentTurn(d);
  const n = needed(d);
  const tclock = useTurnClock(d);
  const canAct = d?.you?.can_act === true && d?.state === "running";
  // 1.1.7: a spectator watches the draft read-only (no action, no sound; leaving frees its place).
  const watching = isSpectator(match) || d?.you?.team === "spectator";

  // A new turn: the selection starts again. One light sound when the player's turn comes.
  const turnIndex = d?.current ?? null;
  useEffect(() => { setPicked([]); }, [turnIndex]);
  const wasActing = useRef(false);
  useEffect(() => {
    if (canAct && !wasActing.current) play("confirm");
    wasActing.current = canAct;
  }, [canAct, turnIndex]);
  useEffect(() => {
    if (!leaving) return;
    const t = window.setTimeout(() => setLeaving(false), 5000);
    return () => window.clearTimeout(t);
  }, [leaving]);

  if (!usable(d)) return null;
  const { title, line } = words(c, h, (u) => names[u] ?? "?", d);
  const stages = idsOf(d, "stage");
  const modes = idsOf(d, "mode");
  const yourTeam: Team | null = d.you?.team === "alpha" || d.you?.team === "bravo" ? d.you.team : null;
  const myChoice: string[] = Array.isArray(d.you?.choice) ? d.you.choice : [];
  // Votes of the player's team on the current turn (shown on the cards).
  const teamVotes: Record<string, number> = {};
  for (const v of yourTeam && Array.isArray(d.teams?.[yourTeam]?.votes) ? d.teams[yourTeam].votes : []) for (const id of Array.isArray(v?.ids) ? v.ids : []) teamVotes[id] = (teamVotes[id] ?? 0) + 1;
  const reason = (o: J) => {
    const k = `d.reason.${o.reason}` as CKey;
    return o.reason && c(k) !== k ? c(k) : c("d.reason.other");
  };
  const send = () => void a.run(async () => {
    try {
      applyMatch(await req("POST", `/v1/matches/${encodeURIComponent(match.id)}/draft`, { turn: d.current, choices: picked }));
      setPicked([]);
    } catch (e) {
      // The turn moved on meanwhile: read the match again (the error stays shown).
      applyMatch(await req("GET", `/v1/matches/${encodeURIComponent(match.id)}`).catch(() => match));
      throw e;
    }
  });
  const leave = () => {
    if (!leaving) return setLeaving(true);
    void a.run(async () => { applyMatch(await req("POST", `/v1/matches/${encodeURIComponent(match.id)}/decline`)); });
  };
  const verb = turn?.action === "pick" ? "pick" : "ban";
  const resultStage: string | null = typeof d.result?.stage === "string" ? d.result.stage : null;
  const resultMode: string | null = typeof d.result?.mode === "string" ? d.result.mode : null;

  const card = (o: J) => {
    const id = o.id as string;
    const m = markOf(o);
    const choosable = selectable(d, o);
    const mineNow = picked.includes(id) || (picked.length === 0 && myChoice.includes(id) && d.state === "running");
    const team: Team | null = o.by === "alpha" || o.by === "bravo" ? o.by : null;
    const votes = teamVotes[id] ?? 0;
    const mark = m === "banned" ? "banned" : m === "unavailable" ? "unavailable" : m === "picked" || mineNow ? "selected" : undefined;
    const sub = m === "picked" ? (team ? c("d.picked.by", { team: c(teamKey(team)) }) : c("d.picked")) : votes > 0 && d.decider === "vote" ? c("d.votes", { n: votes }) : undefined;
    return (
      <StageCard
        key={id} id={id} fallback={stageFallback(L, id, meta)} mark={mark} sub={sub}
        note={m === "banned" ? (team ? c("d.banned.by", { team: c(teamKey(team)) }) : undefined) : m === "unavailable" ? reason(o) : undefined}
        className={`${team ? `dr-by-${team}` : ""} ${m === "picked" ? "dr-picked" : ""} ${choosable ? "dr-choosable" : ""}`}
        role={choosable ? (n > 1 ? "checkbox" : "radio") : undefined} checked={choosable ? picked.includes(id) : undefined}
        onClick={choosable ? () => setPicked((p) => toggle(p, id, n)) : undefined}
      />
    );
  };

  return (
    <div className={`stage stage2 dr ${canAct ? "dr-mine" : ""}`} role="dialog" aria-modal="true" aria-labelledby="dr-title">
      <header className="st-head">
        <div className="st-head-text">
          <div className="eyebrow">{c("d.eyebrow")} · {L(d.title) || BRAND.name}</div>
          <h1 id="dr-title" className="display stage-title">{title}</h1>
          {L(d.summary) && <p className="dr-summary">{L(d.summary).slice(0, 300)}</p>}
          {watching && <p className="dr-summary dr-watching">{c("st.draft")}</p>}
        </div>
        <div className="st-head-actions">
          {match.state === "drafting" && (
            <button className={`btn btn-small ${leaving ? "btn-danger" : "btn-ghost"}`} disabled={a.busy} onClick={leave}>{leaving ? c(watching ? "st.leave.confirm" : "d.leave.confirm") : c(watching ? "st.leave" : "d.leave")}</button>
          )}
          <button className="btn btn-ghost" data-autofocus={canAct ? undefined : true} onClick={onReduce}>{c("d.reduce")}</button>
        </div>
      </header>
      <div className="dr-cols">
        <TeamPanel team="alpha" d={d} names={names} ratings={ratings} me={me} stageName={stageName} />
        <main className="dr-main">
          <section className={`dr-now ${h.kind === "you" ? "dr-now-you" : ""}`} aria-live="polite">
            {h.kind === "done" ? <Ring fraction={1} value="" tone="done" />
              : tclock ? <Ring fraction={tclock.fraction} value={clock(tclock.left)} caption={c("d.ring")} />
                : <Ring fraction={null} spin value="…" />}
            <div className="dr-now-text">
              <p className="st-now-title">{line}</p>
              {h.kind === "you" && (
                <div className="dr-act">
                  <span className="dr-picked-line">
                    {picked.length === 0 ? c("d.select", { what: what(c, turn?.kind, n) }) : picked.map((id) => (turn?.kind === "mode" ? modeName(id) : stageName(id))).join(", ")}
                  </span>
                  <button className="btn btn-big" data-autofocus disabled={a.busy || picked.length !== n} onClick={send}>
                    {verb === "pick" ? <IconCheck /> : <IconBan />} {h.vote ? c("d.btn.vote") : verb === "pick" ? c("d.btn.pick") : c("d.btn.ban")}
                  </button>
                </div>
              )}
              {h.kind === "you" && h.vote && myChoice.length > 0 && <p className="st-now-sub">{c("d.vote.yours", { names: myChoice.map(stageName).join(", ") })}</p>}
              {h.kind === "done" && (resultStage || resultMode) && (
                <p className="st-now-sub">{[resultMode ? c("d.result.mode", { name: modeName(resultMode) }) : null, resultStage ? c("d.result.stage", { name: stageName(resultStage) }) : null].filter(Boolean).join(" · ")}</p>
              )}
              <ErrorLine text={a.error} />
            </div>
            {h.kind === "done" && resultStage && <div className="dr-result"><StageCard id={resultStage} fallback={stageFallback(L, resultStage, meta)} mark="selected" /></div>}
          </section>
          {modes.length > 0 && (
            <section className="dr-modes" aria-label={c("d.modes")}>
              <h2 className="st-block-title">{c("d.modes")}</h2>
              <div className="dr-chips">
                {modes.map((o: J) => {
                  const m = markOf(o);
                  const choosable = selectable(d, o);
                  const team: Team | null = o.by === "alpha" || o.by === "bravo" ? o.by : null;
                  const on = picked.includes(o.id);
                  const cls = `dr-chip dr-chip-${m} ${team ? `dr-by-${team}` : ""} ${on ? "on" : ""}`;
                  const body = <>{m === "banned" ? <IconBan /> : m === "picked" || on ? <IconCheck /> : m === "unavailable" ? <IconClose /> : null}{modeName(o.id)}</>;
                  return choosable
                    ? <button key={o.id} type="button" className={cls} role={n > 1 ? "checkbox" : "radio"} aria-checked={on} onClick={() => setPicked((p) => toggle(p, o.id, n))}>{body}</button>
                    : <span key={o.id} className={cls} title={m === "unavailable" ? reason(o) : undefined}>{body}</span>;
                })}
              </div>
            </section>
          )}
          {stages.length > 0 && (
            <section aria-label={c("d.stages")}>
              <div className={`stp dr-grid ${stages.length > 8 ? "stp-compact" : ""}`} role={canAct && turn?.kind === "stage" ? (n > 1 ? "group" : "radiogroup") : undefined} aria-label={c("d.stages")}>
                {stages.map(card)}
              </div>
            </section>
          )}
        </main>
        <TeamPanel team="bravo" d={d} names={names} ratings={ratings} me={me} stageName={stageName} />
      </div>
      <section className="dr-foot" aria-label={c("d.log")}>
        <h2 className="st-block-title">{c("d.log")}</h2>
        <TurnLog d={d} stageName={stageName} modeName={modeName} />
      </section>
    </div>
  );
}

/** The draft in a match card: where it stands, and the button that opens the screen again. */
export function DraftLine({ match, onOpen }: { match: J; onOpen?: () => void }) {
  const { c, L } = useC();
  const d = match?.draft;
  if (!usable(d) || (match.state !== "drafting" && match.state !== "pending_accept")) return null;
  if (match.state === "pending_accept") {
    return <p className="small dr-then">{c("d.then", { title: L(d.title) || c("d.eyebrow") })}{L(d.summary) ? ` ${L(d.summary).slice(0, 200)}` : ""}</p>;
  }
  const mine = d.you?.can_act === true;
  return (
    <div className={`dr-line ${mine ? "dr-line-mine" : ""}`}>
      <span>{mine ? c("d.line.you") : c("d.line")}</span>
      {onOpen && <button className="btn btn-small" onClick={onOpen}>{c("d.open")}</button>}
    </div>
  );
}
