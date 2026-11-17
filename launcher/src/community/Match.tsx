// A match of the community server: the accept phase (match found), then the game on this machine (the runner's phases
// and the mirrored statuses of every player), then the server's decision.
import { play } from "../sound/engine";
import { proto, type J, type RunnerStatus } from "./api";
import { api, type GameCatalogue, type Labels } from "../api";
import { useEffect, useState } from "react";
import { DICTS, pick, type Key } from "../i18n";
import { applyMatch, myId, req, useLive } from "./store";
import { duration, leftForfeit, useC, type CKey } from "./texts";
import { GameCheckNotice, useGameBlocked } from "./GameCheck";
import { SLOW_AFTER_S, stateKey as waitKey } from "./waiting";
import { screenKey, screenWanted, useReduced, useWaitInfo } from "./MatchScreen";
import { ErrorLine, Left, RankBadge, Who, signed, stageFallback, useAction, useLeft, useModeName, vs } from "./ui";
import { DraftLine } from "./Draft";
import { StageCard, StageTag } from "../stages";
import { IconCheck, IconClose, IconCrown } from "../icons";
import { useApp } from "../ctx";
import { diagnostic } from "../diagnostic";
import { CopyButton } from "../ui";
import { MatchStands, SpectatorLine, WitnessVerdict, isSpectator } from "./Stands";

const RUNNER_BAD = ["refused", "failed", "late"];

function gameState(c: (k: CKey) => string, s: string | null | undefined): string {
  if (!s) return "";
  const k = `m.game.${s}` as CKey;
  return c(k) === k ? s : c(k);
}

/** Where the game stands, in plain words: one sentence for the step (who is awaited while the session waits for
 *  players), the time spent in it and before the engine gives up, the hidden window, and help when another player's game
 *  seems not to come. */
function WaitProgress({ r, match }: { r: RunnerStatus; match: J }) {
  const { c } = useC();
  const info = useWaitInfo(r, match);
  if (!info) return null;
  const { sentence, elapsed, left, hidden, slow } = info;
  return (
    <div className="stack cm-wait" aria-live="polite">
      <p>{sentence}</p>
      {elapsed !== null && (
        <p className="small muted cm-wait-time">
          {c("m.wait.elapsed", { t: duration(c, elapsed) })}{left !== null ? ` ${c("m.wait.left", { t: duration(c, left) })}` : ""}
        </p>
      )}
      {hidden && <p className="small muted">{c("m.wait.hidden")}</p>}
      {slow.length > 0 && <p className="notice notice-info small" role="status">{c("m.wait.slow", { names: slow.join(", "), s: SLOW_AFTER_S })}</p>}
    </div>
  );
}

/** The players of a found match, by team, each with their rank; nothing when the server sent no names. */
function FoundPlayers({ parts, me }: { parts: J[]; me: string | null }) {
  const { c } = useC();
  const nameOf = (p: J) => String(p?.member_name ?? p?.user?.display_name ?? "").slice(0, 32);
  const players = parts.slice(0, 8).filter((p: J) => nameOf(p));
  if (players.length === 0) return null;
  return (
    <div className="cm-found-teams">
      {(["alpha", "bravo"] as const).map((t) => {
        const list = players.filter((p: J) => p.team === t);
        return list.length === 0 ? null : (
          <div key={t} className={`cm-found-team cm-found-team-${t}`}>
            <h4>{t === "alpha" ? c("room.alpha") : c("room.bravo")}</h4>
            <ul className="cm-list">
              {list.map((p: J) => (
                <li key={p.user?.id ?? nameOf(p)} className="cm-row">
                  <Who user={{ display_name: nameOf(p) }} me={p.user?.id === me}>
                    <RankBadge rating={p.rating} />
                  </Who>
                </li>
              ))}
            </ul>
          </div>
        );
      })}
    </div>
  );
}

/** Accept or decline a found match before its deadline (`queues`). */
export function MatchFound({ match, onClose, inCard }: { match: J; onClose?: () => void; inCard?: boolean }) {
  const { c, L } = useC();
  const blocked = useGameBlocked();
  const modeName = useModeName();
  const live = useLive();
  const me = myId();
  const a = useAction();
  const left = useLeft(match?.deadlines?.accept);
  const parts: J[] = match.participants ?? [];
  const mine = parts.find((p: J) => p.user?.id === me);
  const accepted = parts.filter((p: J) => p.accept === "accepted").length;
  const answer = (verb: "accept" | "decline") => void a.run(async () => { applyMatch(await req("POST", `/v1/matches/${encodeURIComponent(match.id)}/${encodeURIComponent(verb)}`)); if (verb === "decline") onClose?.(); });
  const stage = typeof match.rules?.stage === "string" ? match.rules.stage : null;
  return (
    <div className="stack cm-found">
      {!inCard && <p className="cm-found-sub">{c("m.found.sub", { format: vs(c, match.rules?.teams), mode: modeName(match.rules?.mode, live.overview?.meta) })}{match.rules?.rated ? ` · ${c("q.rated")}` : ""}</p>}
      {!inCard && stage && <StageCard id={stage} fallback={stageFallback(L, stage, live.overview?.meta)} className="cm-found-stage" />}
      <DraftLine match={match} />
      <FoundPlayers parts={parts} me={me} />
      {left !== null && <div className="cm-countdown" role="timer" aria-live="polite">{c("m.accept.left", { s: left })}</div>}
      {isSpectator(match) ? (
        <p className="small">{c("st.watching.found")} {c("m.accepted", { n: accepted, total: parts.length })}</p>
      ) : mine?.accept === "accepted" ? (
        <p className="ok-text"><IconCheck /> {c("m.accepted", { n: accepted, total: parts.length })}</p>
      ) : (
        <>
          <GameCheckNotice />
          <div className="row">
            <button className="btn btn-big" data-autofocus disabled={a.busy || blocked} onClick={() => { play("confirm"); answer("accept"); }}>{c("m.accept")}</button>
            <button className="btn btn-danger" disabled={a.busy} onClick={() => answer("decline")}>{c("m.decline")}</button>
          </div>
          <p className="hint">{c("m.decline.warn")}</p>
        </>
      )}
      <ErrorLine text={a.error} />
    </div>
  );
}

/** The launcher's own question before it starts the game for a match this launcher did not join in this session (or
 *  after too many launches from one server): runner phase `confirm` (`proto/consent.rs`). Nothing starts without a yes;
 *  no answer before the countdown ends means no. */
export function LaunchQuestion({ r, match }: { r: RunnerStatus; match: J | null }) {
  const { c } = useC();
  const live = useLive();
  const a = useAction();
  const [left, setLeft] = useState<number>(r.ask_s ?? 0);
  useEffect(() => {
    const end = Date.now() + (r.ask_s ?? 0) * 1000;
    setLeft(r.ask_s ?? 0);
    const t = window.setInterval(() => setLeft(Math.max(0, Math.ceil((end - Date.now()) / 1000))), 500);
    return () => window.clearInterval(t);
  }, [r.match_id, r.ask_s]);
  const server = live.server;
  const answer = (start: boolean) => void a.run(async () => { if (server) await proto.confirmLaunch(server, r.match_id, start); });
  const names: string[] = (match?.participants ?? []).map((p: J) => String(p?.member_name ?? p?.user?.display_name ?? "")).filter((n: string) => n).slice(0, 8);
  return (
    <div className="stack cm-found cm-ask">
      <p>{r.ask === "too-many" ? c("m.ask.too_many") : c("m.ask.not_joined")}</p>
      <ul className="small cm-ask-facts">
        <li>{c("m.ask.server", { name: live.overview?.server?.name ?? "?" })}</li>
        {r.relay && <li>{c("m.ask.relay", { relay: r.relay })}</li>}
        {names.length > 0 && <li>{c("m.ask.players", { names: names.join(", ") })}</li>}
      </ul>
      <p className="hint">{c("m.ask.hint")}</p>
      <div className="cm-countdown" role="timer" aria-live="polite">{c("m.ask.left", { s: left })}</div>
      <div className="row">
        <button className="btn btn-big" disabled={a.busy} onClick={() => answer(true)}>{c("m.ask.start")}</button>
        <button className="btn btn-ghost" data-autofocus disabled={a.busy} onClick={() => answer(false)}>{c("m.ask.refuse")}</button>
      </div>
      <ErrorLine text={a.error} />
    </div>
  );
}

function Outcome({ match }: { match: J }) {
  const { c, L, lang } = useC();
  const { advanced } = useApp();
  const me = myId();
  const o = match.outcome;
  if (!o) return null;
  const mine = (match.participants ?? []).find((p: J) => p.user?.id === me);
  const verdict = (o.players ?? []).find((p: J) => p.user_id === me)?.verdict;
  // 1.1.2: why each refused report was refused (`invalid_reasons`: the failed check and the server's explanation).
  const refused: J[] = Array.isArray(o.invalid_reasons) ? o.invalid_reasons.slice(0, 10) : [];
  const nameOf = (uid: unknown) => (match.participants ?? []).find((p: J) => p.user?.id === uid)?.user?.display_name ?? String(uid ?? "?");
  const winner = o.winner === "alpha" || o.winner === "bravo" ? o.winner : null;
  const changes: J[] = (o.rating_changes ?? []).filter((r: J) => r.user_id === me);
  const teamName = (t: string) => (t === "alpha" ? "Alpha" : t === "bravo" ? "Bravo" : t);
  // The game's record as the server counted it (api.md, "Outcome object"): the teams' scores and each player's line.
  const rec = o.record && typeof o.record === "object" ? o.record : null;
  const teams: J[] = Array.isArray(rec?.teams) ? rec.teams.filter((t: J) => t?.team === "alpha" || t?.team === "bravo").slice(0, 2) : [];
  const players: J[] = Array.isArray(rec?.players) ? rec.players.slice(0, 10) : [];
  const n = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : 0);
  // 1.1.11: won or lost because a player left the match in play (`outcome.forfeit`, reason `left`): said in full; the
  // server's summary says the same in fewer languages.
  const duel = Array.isArray(match.rules?.teams) && match.rules.teams[0] === 1 && match.rules.teams[1] === 1;
  const left = o.decision === "forfeit" ? leftForfeit(c, o.forfeit, mine?.team, me, duel) : null;
  return (
    <div className={`cm-outcome ${winner && mine?.team === winner ? "cm-win" : winner ? "cm-loss" : ""}`}>
      <h3>
        {winner && mine ? (mine.team === winner ? <><IconCrown /> {c("m.you.win")}</> : c("m.you.lose")) : winner ? c("m.winner", { team: teamName(winner) }) : c("m.nowinner")}
        <span className="cm-counted">{o.counted ? c("m.counted") : c("m.notcounted")}</span>
      </h3>
      {left && <p className="cm-forfeit"><strong>{left.won === true ? c("m.forfeit.win") : left.won === false ? c("m.forfeit.loss") : c("m.forfeit")}</strong> — {left.why}</p>}
      {winner && mine && <p className="small">{c("m.winner", { team: teamName(winner) })}</p>}
      {teams.map((t: J) => (
        <p key={t.team} className="small cm-score">{c("m.score.team", { team: teamName(t.team), points: n(t.paint_points), share: (n(t.turf_permille) / 10).toLocaleString(lang, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })}</p>
      ))}
      {players.length > 0 && (
        <ul className="small muted cm-score-players">
          {players.map((p: J, i: number) => (
            <li key={i}>{c("m.score.player", { name: String(p.name ?? "?").slice(0, 32), paint: n(p.paint), k: n(p.kills), a: n(p.assists), d: n(p.deaths), disc: p.disconnected ? c("m.score.disc") : "" })}</li>
          ))}
        </ul>
      )}
      {L(o.summary) && !left && <p>{L(o.summary)}</p>}
      <WitnessVerdict match={match} />
      {o.rematch_id && <p className="cm-rematch"><strong>{match.rules?.stage ? c("m.rematch.stage") : c("m.rematch")}</strong></p>}
      {advanced && verdict && <p className="small muted">{c("m.verdict", { v: c(`m.verdict.${verdict}` as CKey) === `m.verdict.${verdict}` ? verdict : c(`m.verdict.${verdict}` as CKey) })}</p>}
      {advanced && refused.length > 0 && (
        <details className="advanced">
          <summary>{c("m.refused", { n: refused.length })}</summary>
          <ul className="small cm-score-players">
            {refused.map((r: J, i: number) => (
              <li key={i}>{String(nameOf(r.user_id)).slice(0, 32)} · <span className="mono">{String(r.check ?? "?").slice(0, 16)}</span> — {(typeof r.text === "string" ? r.text : L(r.text)).slice(0, 300)}</li>
            ))}
          </ul>
        </details>
      )}
      {changes.map((r: J, i: number) => (
        <p key={i} className="cm-delta">
          {c("m.rating", { ladder: r.ladder, delta: typeof r.delta === "number" ? signed(r.delta) : "" })}
          {r.after?.display ? ` · ${r.after.display}` : ""} {L(r.reason) && <span className="muted">({L(r.reason)})</span>}
        </p>
      ))}
    </div>
  );
}

/** The weapon or gear a match leaves to the player's choice (`rules.loadout` with `allowed` / `denied`): one list per
 *  slot, named from the locker's catalogue (the player's game data), sent before the launch deadline. */
function LoadoutPicker({ r }: { r: RunnerStatus }) {
  const { c, lang } = useC();
  const live = useLive();
  const [cat, setCat] = useState<GameCatalogue | null>(null);
  const [picks, setPicks] = useState<Record<string, string>>({});
  const [sent, setSent] = useState(false);
  useEffect(() => {
    void api.profileCatalogue().then((v) => setCat(v.catalogue)).catch(() => setCat(null));
  }, []);
  const label = (n: Labels | undefined) => (n ? pick(n, lang) : undefined);
  const category = (slot: string) => (slot === "weapon" ? "weapon" : slot.split(".")[1]) as keyof GameCatalogue["categories"];
  const nameOf = (slot: string, id: string) => label(cat?.categories?.[category(slot)]?.find((e) => String(e.id) === id)?.name) ?? id;
  const slots = Object.entries(r.choices ?? {});
  const options = (slot: string, k: { allowed?: string[]; denied?: string[] }): string[] =>
    k.allowed ? k.allowed.slice(0, 300) : (cat?.categories?.[category(slot)] ?? []).filter((e) => e.selectable && !(k.denied ?? []).includes(String(e.id))).slice(0, 300).map((e) => String(e.id));
  const send = () => { if (live.server) void proto.pickLoadout(live.server, r.match_id, picks).then(() => setSent(true)); };
  return (
    <div className="stack cm-loadout-pick">
      <p className="small">{c("m.loadout.pick")}</p>
      {slots.map(([slot, k]) => (
        <label key={slot} className="cm-field">
          <span className="field-label">{c(`m.slot.${slot}` as CKey)}</span>
          <select value={picks[slot] ?? ""} disabled={sent} onChange={(e) => setPicks({ ...picks, [slot]: e.target.value })}>
            <option value="">{c("m.loadout.choose")}</option>
            {options(slot, k).map((id) => <option key={id} value={id}>{nameOf(slot, id)}</option>)}
          </select>
        </label>
      ))}
      <button className="btn btn-small" disabled={sent || slots.some(([s]) => !picks[s])} onClick={send}>{sent ? c("m.loadout.sent") : c("m.loadout.send")}</button>
    </div>
  );
}

/** The current (or last) match: state, players with their mirrored game state, this machine's runner, the outcome.
 *  The settings passed to the game and the engine's codes are for the advanced mode; the simple mode copies them in the
 *  diagnostic. */
export function MatchCard({ match, compact }: { match: J; compact?: boolean }) {
  const { c, L, t, lang } = useC();
  const { advanced, info, status, game } = useApp();
  // The engine's error codes (status file `error`) in the player's language, with the advice; codes whose sentence
  // needs numbers, and unknown codes, keep the engine's own sentence.
  const engineText = (code: string | null | undefined): string | null => {
    const key = `mc.err.${code}` as Key;
    if (!code || !(key in DICTS.en) || t(key).includes("{")) return null;
    const fix = `${key}.fix` as Key;
    return fix in DICTS.en ? `${t(key)} ${t(fix)}` : t(key);
  };
  const modeName = useModeName();
  const live = useLive();
  const me = myId();
  const r = live.runner && live.runner.match_id === match.id ? live.runner : null;
  const stateKey = `m.state.${match.state}` as CKey;
  const [reduced, setReduced] = useReduced();
  const parts: J[] = match.participants ?? [];
  const cancel = match.cancel_reason ? (`m.cancel.${match.cancel_reason}` as CKey) : null;
  // The stage played (the record's), else the one asked for.
  const stage = typeof match.outcome?.record?.stage_scene === "string" ? match.outcome.record.stage_scene : typeof match.rules?.stage === "string" ? match.rules.stage : null;
  return (
    <article className={`card cm-match ${compact ? "cm-compact" : ""}`} aria-live="polite">
      <header className="row">
        <h2 className="grow">{isSpectator(match) ? c("st.card.title") : c("m.title")} <span className="muted small">{vs(c, match.rules?.teams)} · {modeName(match.rules?.mode, live.overview?.meta)}{match.rules?.rated ? ` · ${c("q.rated")}` : ""}</span></h2>
        <span className="pill pill-neutral">{c(stateKey) === stateKey ? c("m.state.other") : c(stateKey)}</span>
      </header>
      {stage && <StageTag id={stage} fallback={stageFallback(L, stage, live.overview?.meta)} className="cm-match-stage" />}
      <SpectatorLine match={match} />
      {match.series && typeof match.series === "object" && (
        <p className="small cm-match-series">
          {c("sr.in_match", { n: match.series.index ?? "?", fmt: match.series.format?.kind === "best_of" ? c("sr.best_of", { n: match.series.format.games ?? "?" }) : match.series.format?.kind === "first_to" ? c("sr.first_to", { n: match.series.format.wins ?? "?" }) : match.series.format?.games ? c("sr.fixed", { n: match.series.format.games }) : "" })}
        </p>
      )}
      {live.ended?.outcome?.rematch_id === match.id && match.id && (
        <p className="notice notice-info">{live.ended.rules?.stage ? c("m.rematch.stage") : c("m.rematch")}</p>
      )}
      {match.origin?.kind === "activity" && <p className="small muted">{c("m.origin.activity", { name: L(live.activityTitles[match.origin.activity_id]) || String(match.origin.activity_id ?? "") })}</p>}
      {L(match.rules?.loadout?.summary) && <p className="small">{c("m.loadout", { summary: L(match.rules.loadout.summary).slice(0, 300) })}</p>}
      {match.state === "pending_accept" && <MatchFound match={match} inCard />}
      {match.state === "drafting" && <DraftLine match={match} onOpen={reduced === `${match.id}:draft` ? () => setReduced(null) : undefined} />}
      {match.state === "ready" && match.deadlines?.launch && !r && <p className="hint"><Left deadline={match.deadlines.launch} /></p>}
      {/* 1.1.10: this player's report is in and the server no longer holds them in this match. */}
      {match.you?.free === true && (match.state === "running" || match.state === "ready") && <p className="hint">{c("m.free")}</p>}
      {r && (
        <div className={`cm-runner ${RUNNER_BAD.includes(r.phase) ? "cm-runner-bad" : ""}`}>
          <strong>{c(`m.runner.${r.phase}` as CKey)}</strong>
          {r.game_state && !(r.phase === "running" && waitKey(r.game?.state ?? r.game_state, r.role)) && <span className="muted"> · {gameState(c, r.game_state)}</span>}
          {r.phase === "running" && <WaitProgress r={r} match={match} />}
          {screenWanted(r) && reduced === screenKey(r) && <button className="btn btn-small" onClick={() => setReduced(null)}>{c("m.screen.open")}</button>}
          {r.phase === "choosing" && r.choices && <LoadoutPicker r={r} />}
          {r.loadout && Object.keys(r.loadout).length > 0 && <p className="small">{c("m.loadout.applied", { n: Object.keys(r.loadout).length })}{r.loadout_ok === true ? ` · ${c("m.loadout.ok")}` : r.loadout_ok === false ? ` · ${c("m.loadout.notok")}` : ""}</p>}
          {r.reason && engineText(r.code) && <p className="small">{engineText(r.code)}</p>}
          {advanced && r.reason && <p className={`small mono wrapany ${engineText(r.code) ? "muted" : ""}`}>{r.code} — {r.reason}</p>}
          {!advanced && !compact && RUNNER_BAD.includes(r.phase) && (
            <div className="row"><CopyButton small label={t("diag.copy")} text={() => diagnostic({ info, status, game, lang }, `${c(`m.runner.${r.phase}` as CKey)} (${r.code ?? "-"})`, [`match: ${match.id}`, `runner: ${r.phase} ${r.game_state ?? ""}`, `reason: ${r.reason ?? "-"}`, ...r.env.map(([k, v]) => `${k}=${v}`)])} /></div>
          )}
          {advanced && !compact && r.env.length > 0 && (
            <details className="advanced">
              <summary>{c("m.env")}</summary>
              <table className="envtable mono tiny"><tbody>{r.env.map(([k, v]) => <tr key={k}><td>{k}</td><td className="wrapany">{v}</td></tr>)}</tbody></table>
            </details>
          )}
        </div>
      )}
      {r?.phase === "refused" && <GameCheckNotice />}
      {cancel && <p className="notice notice-info">{c(cancel) === cancel ? match.cancel_reason : c(cancel)}</p>}
      {!compact && parts.length > 0 && (
        <div className="cm-teams">
          {(["alpha", "bravo"] as const).map((t) => (
            <div key={t} className={`cm-team cm-team-${t}`}>
              <h3>{t === "alpha" ? c("room.alpha") : c("room.bravo")}</h3>
              <ul className="cm-list">
                {parts.filter((p: J) => p.team === t).map((p: J) => (
                  <li key={p.user?.id ?? p.member_name} className="cm-row">
                    <Who user={{ display_name: p.member_name ?? p.user?.display_name }} me={p.user?.id === me} crown={p.role === "host"}>
                      <RankBadge rating={p.rating} />
                    </Who>
                    <span className="small muted grow">
                      {p.status?.state ? gameState(c, p.status.state) : p.launched ? c("m.launched") : ""}
                    </span>
                    {p.accept === "accepted" && match.state === "pending_accept" && <IconCheck aria-label={c("m.accept")} />}
                    {p.accept === "declined" && <IconClose aria-label={c("m.decline")} />}
                    {p.reported && <span className="pill pill-ok">{c("m.reported")}</span>}
                  </li>
                ))}
              </ul>
            </div>
          ))}
          <MatchStands match={match} />
        </div>
      )}
      <Outcome match={match} />
    </article>
  );
}
