// The full-frame screen of a community match once the game is launched: every step of the game in order (done / under
// way / to come), the sentence, time and give-up deadline of the current step, and the step of each player. It reuses
// the layout of the private-match screen (`match/StageLayout.tsx`) and can be reduced to the page.
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { DICTS, useI18n, type Key } from "../i18n";
import { STEPS } from "../match/MatchFlow";
import { BRAND } from "../brand";
import { IconClose } from "../icons";
import type { MatchRole } from "../api";
import type { J, RunnerStatus } from "./api";
import { myId, useLive } from "./store";
import { RankBadge, stageFallback, useModeName, vs } from "./ui";
import { StageTag, useStageInfo } from "../stages";
import { PlayerCards, Ring, StageShell, clock, type StagePlayer, type StageStep } from "../match/StageLayout";
import { useC, type CKey } from "./texts";
import { SLOW_AFTER_S, missingNames, slowPlayers, stateKey as waitKey, stepTimes, trackLaunched, windowHidden, type SeenLaunched } from "./waiting";
import { useApp } from "../ctx";
import { diagnostic } from "../diagnostic";
import { CopyButton } from "../ui";
import { SpectatorControls, isSpectator } from "./Stands";

/** Runner phases in which the game is up or being brought up (the screen is shown). */
const ACTIVE = ["closing", "launching", "continuing", "running", "uploading"];
/** Runner phases that stopped the match: the screen shows the reason (1.1.7: `late`, a spectator whose match had
 *  started). */
export const RUNNER_STOPPED = ["refused", "failed", "closed", "late"];

// ---- reduced / open state (shared between the screen and the "open" button of the page) --------------------------

let reducedFor: string | null = null;
const listeners = new Set<() => void>();
const subscribe = (f: () => void) => { listeners.add(f); return () => { listeners.delete(f); }; };
const setReduced = (id: string | null) => { reducedFor = id; listeners.forEach((f) => f()); };
/** The match whose screen the player reduced (key: match id + launch moment, so that a new launch opens it again). */
export function useReduced(): [string | null, (id: string | null) => void] {
  return [useSyncExternalStore(subscribe, () => reducedFor), setReduced];
}

/** Is the screen relevant for this runner (a game on its way, or a stop to explain)? */
export function screenWanted(r: RunnerStatus | null | undefined): boolean {
  return !!r && (ACTIVE.includes(r.phase) || RUNNER_STOPPED.includes(r.phase));
}
export const screenKey = (r: RunnerStatus) => `${r.match_id}:${RUNNER_STOPPED.includes(r.phase) ? "stop" : "run"}`;

// ---- the sentence and the times ----------------------------------------------------------------------------------

/** Where the game stands, in plain words: one sentence for the step (who is awaited while the session waits for
 *  players), the time spent in it and before the engine gives up, the hidden window, and help when another player's
 *  game seems not to come. `null`: nothing to say yet. Ticks every second. */
export function useWaitInfo(r: RunnerStatus, match: J) {
  const { c } = useC();
  const me = myId();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  const seen = useRef<SeenLaunched>({});
  const parts: J[] = match?.participants ?? [];
  seen.current = trackLaunched(seen.current, parts, me, now);
  const g = r.game;
  const state = g?.state ?? r.game_state;
  const key = waitKey(state, r.role);
  if (!key) return null;
  let sentence: string;
  if (state === "waiting-for-players") {
    const names = missingNames(parts, me);
    const counts = g && g.present !== null && g.expected !== null ? { present: g.present, expected: g.expected } : null;
    sentence = counts ? (names.length > 0 ? c("m.wait.waiting-for-players", { names: names.join(", "), ...counts }) : c("m.wait.waiting-for-players.anon", counts)) : c("m.wait.waiting-for-players.none");
  } else {
    sentence = c(key as CKey);
  }
  const hidden = windowHidden(g, r.game_state);
  const { elapsed, left } = hidden ? stepTimes(g, now) : { elapsed: null, left: null };
  const slow = slowPlayers(seen.current, parts, state, now);
  return { sentence, elapsed, left, hidden, slow };
}

// ---- steps ---------------------------------------------------------------------------------------------------------

function stepsFor(role: MatchRole): { key: string; states: string[] }[] {
  return [...STEPS[role], { key: "m.screen.step.results", states: ["results", "finished", "waiting-command"] }];
}

/** Index of the step under way for a game state (the last one when the match is over). */
function indexOf(role: MatchRole, state: string | null | undefined): number {
  const steps = stepsFor(role);
  const i = steps.findIndex((s) => s.states.includes(state ?? ""));
  return i < 0 ? 0 : i;
}

const roleOf = (v: string | null | undefined): MatchRole => (v === "host" ? "host" : "join");

// ---- the screen ----------------------------------------------------------------------------------------------------

export function MatchScreen({ r, match, onReduce }: { r: RunnerStatus; match: J; onReduce: () => void }) {
  const { c, L } = useC();
  const { t, lang } = useI18n();
  const { advanced, info: appInfo, status, game } = useApp();
  const live = useLive();
  const modeName = useModeName();
  const stageOf = useStageInfo();
  const stopped = RUNNER_STOPPED.includes(r.phase) || r.game?.state === "error";
  const role = roleOf(r.role);
  const state = r.game?.state ?? r.game_state ?? undefined;
  // The last step seen before a stop: the one the game stopped at.
  const lastGood = useRef<string | undefined>(undefined);
  if (state && state !== "error") lastGood.current = state;
  const steps = stepsFor(role);
  const over = state === "results" || state === "finished" || state === "waiting-command";
  const idx = indexOf(role, stopped ? lastGood.current : state);
  const info = useWaitInfo(r, match);
  const parts: J[] = match?.participants ?? [];
  const me = myId();
  const label = (k: string) => (k.startsWith("m.") ? c(k as CKey) : t(k as Key));
  const gameState = (s: string | null | undefined) => (s ? (c(`m.game.${s}` as CKey) === `m.game.${s}` ? s : c(`m.game.${s}` as CKey)) : "");

  // The engine's error codes in the player's language, with the advice; unknown codes keep the engine's sentence.
  const errKey = r.code && `mc.err.${r.code}` in DICTS.en && !t(`mc.err.${r.code}` as Key).includes("{") ? (`mc.err.${r.code}` as Key) : null;
  const errFix = errKey && `${errKey}.fix` in DICTS.en ? t(`${errKey}.fix` as Key) : null;
  const diag = () => diagnostic({ info: appInfo, status, game, lang }, `${c(`m.runner.${r.phase}` as CKey)} (${r.code ?? "-"})`, [`match: ${match?.id ?? "-"}`, `runner: ${r.phase} ${r.game_state ?? ""}`, `reason: ${r.reason ?? "-"}`, `detail: ${r.game?.detail ?? "-"}`, ...r.env.map(([k, v]) => `${k}=${v}`)]);

  // 1.1.7: a spectator watches: its own words, no team of its own, the spectator view's controls.
  const watching = r.seat === "spectator" || isSpectator(match);
  const title = r.phase === "late" ? c("st.screen.late") : stopped ? c("m.screen.stopped") : state === "in-match" ? (watching ? c("st.screen.title") : c("m.screen.inmatch")) : over ? c("m.screen.over") : watching ? c("st.screen.wait") : c("m.screen.title");
  const stepList: StageStep[] = steps.map((s, i) => ({
    key: s.key,
    label: label(s.key),
    state: stopped ? (lastGood.current && i < idx ? "done" : lastGood.current && i === idx ? "failed" : "next") : i < idx || (i === idx && over) ? "done" : i === idx ? "now" : "next",
    detail: !stopped && i === idx && s.states.includes("waiting-for-players") && r.game?.expected != null ? `${r.game.present ?? 0}/${r.game.expected}` : null,
  }));

  // The step under way, in large: its countdown (time before the engine gives up), else the time spent in it.
  const now = stopped ? (
    <div className="st-now-row">
      <Ring fraction={0} value="" tone="bad" />
      <div className="st-now-text" role="alert">
        <p className="st-now-title">{errKey ? t(errKey) : r.phase === "closed" ? c("m.runner.closed") : c(`m.runner.${r.phase}` as CKey)}</p>
        {errFix && <p className="st-now-sub">{errFix}</p>}
        {advanced && r.reason && <p className="small mono wrapany muted">{r.code} — {r.reason}</p>}
        {advanced && !r.reason && r.game?.detail && <p className="small mono wrapany muted">{r.game.detail}</p>}
        <div className="row wrap"><CopyButton small label={t("diag.copy")} text={diag} /></div>
      </div>
    </div>
  ) : (
    <div className="st-now-row">
      {over ? <Ring fraction={1} value="" tone="done" />
        : info && info.left !== null && info.elapsed !== null ? <Ring fraction={info.left / Math.max(1, info.left + info.elapsed)} value={clock(info.left)} caption={c("m.ring.left")} />
          : <Ring fraction={null} spin value={info?.elapsed != null ? clock(info.elapsed) : "…"} caption={info?.elapsed != null ? c("m.ring.elapsed") : undefined} />}
      <div className="st-now-text" aria-live="polite">
        <p className="st-now-step">{idx < steps.length ? label(steps[Math.min(idx, steps.length - 1)].key) : ""}</p>
        <p className="st-now-title">{info?.sentence ?? c(`m.runner.${r.phase}` as CKey)}</p>
        {info?.hidden && <p className="st-now-sub">{c("m.wait.hidden")}</p>}
        {info && info.slow.length > 0 && <p className="notice notice-info small" role="status">{c("m.wait.slow", { names: info.slow.join(", "), s: SLOW_AFTER_S })}</p>}
        {watching && <SpectatorControls compact />}
      </div>
    </div>
  );

  const watchers: J[] = Array.isArray(match?.spectators) ? match.spectators.filter((s: J) => !s.left).slice(0, 2) : [];
  const players: StagePlayer[] = parts.map((p: J): StagePlayer => {
    const ps: string | undefined = p.status?.state;
    const pi = ps ? indexOf(roleOf(p.role), ps) : -1;
    const team = p.team === "alpha" || p.team === "bravo" ? p.team : null;
    return {
      key: String(p.user?.id ?? p.member_name),
      name: String(p.member_name ?? p.user?.display_name ?? "?").slice(0, 32),
      me: p.user?.id === me,
      team,
      teamLabel: team ? c(team === "alpha" ? "room.alpha" : "room.bravo") : null,
      step: ps ? gameState(ps) : p.launched ? c("m.launched") : "…",
      progress: Math.max(0, pi),
      total: stepsFor(roleOf(p.role)).length,
      bad: ps === "error",
      rank: <RankBadge rating={p.rating} />,
    };
  }).concat(watchers.map((s: J) => {
    // The stands: every spectator, shown to the players (nobody watches unseen).
    const ps: string | undefined = s.status?.state;
    const pi = ps ? indexOf(roleOf(s.role), ps) : -1;
    return {
      key: `s:${String(s.user?.id ?? s.member_name)}`,
      name: String(s.member_name ?? s.user?.display_name ?? "?").slice(0, 32),
      me: s.user?.id === me,
      team: null,
      teamLabel: c("st.title"),
      step: ps ? gameState(ps) : s.launched ? c("m.launched") : "…",
      progress: Math.max(0, pi),
      total: stepsFor(roleOf(s.role)).length,
      bad: ps === "error",
    };
  }));

  const rules = match?.rules ?? {};
  // The stage as the game shows it (its picture and name, from the player's files); null: the game's choice.
  const stageId = typeof rules.stage === "string" ? rules.stage : null;
  const stage = <StageTag id={stageId} fallback={stageId ? stageFallback(L, stageId, live.overview?.meta) : c("field.null")} className="st-stage" />;
  const art = stageId && !stopped ? stageOf(stageId).picture : null;
  const recap = [
    { label: c("m.recap.format"), value: vs(c, rules.teams) || "—" },
    { label: c("m.recap.mode"), value: modeName(rules.mode, live.overview?.meta) || "—" },
    { label: c("m.recap.stage"), value: stage },
    { label: c("m.recap.rated"), value: rules.rated ? c("q.rated") : c("q.unrated") },
    { label: c("m.recap.server"), value: live.overview?.server.name ?? "—" },
    ...(watching ? [{ label: c("st.screen.seat"), value: c("st.title") }] : []),
    ...(advanced ? [{ label: "match", value: <span className="mono">{String(match?.id ?? "-")}</span> }] : []),
  ];

  return (
    <StageShell
      bad={stopped}
      titleId="cm-screen-title"
      eyebrow={`${watching ? c("st.screen.eyebrow") : role === "host" ? t("mc.role.host") : t("mc.role.join")} · ${BRAND.name}`}
      title={title}
      steps={stepList}
      stepsLabel={t("mc.steps")}
      now={now}
      art={art}
      playersTitle={c("m.screen.players")}
      players={parts.length > 0 ? <PlayerCards players={players} youLabel={c("m.screen.you")} /> : undefined}
      recapTitle={c("m.recap.title")}
      recap={recap}
      actions={<button className="btn btn-ghost" data-autofocus onClick={onReduce}>{stopped ? <><IconClose /> {c("m.screen.close")}</> : c("m.screen.reduce")}</button>}
      hint={stopped ? undefined : watching ? c("st.screen.hint") : c("m.screen.hint")}
    />
  );
}
