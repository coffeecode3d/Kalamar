// The state of a private match once it has been started: connecting (driven by the runtime's status file, else its
// log), then the result screen. It lives above the pages so that navigating does not lose it.
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Ring, StageShell, type StageStep } from "./StageLayout";
import { api, toAppError, type MatchPlan, type MatchProgress, type MatchResultView, type MatchRole, type MatchRun } from "../api";
import { BRAND } from "../brand";
import { useApp } from "../ctx";
import { errorText, formatDateTime, useI18n, type Key } from "../i18n";
import { IconBack, IconCrown, IconRefresh, IconStop } from "../icons";
import { diagnostic } from "../diagnostic";
import { CopyButton } from "../ui";
import { useStageInfo } from "../stages";

export type Phase = "idle" | "connecting" | "results" | "failed";

interface Flow {
  phase: Phase;
  role: MatchRole | null;
  plan: MatchPlan | null;
  run: MatchRun | null;
  progress: MatchProgress | null;
  result: MatchResultView | null;
  failure: string | null;
  /** What the player can do about it (for the runtime's error codes). */
  failureFix: string | null;
  /** The pre-match screen asks for the next round's plan to be shown (set by "play again"). */
  rematch: MatchPlan | null;
  /** `confirmRelay`: the player confirmed a relay this launcher did not know (`MatchPlan.relay_known`). */
  start: (plan: MatchPlan, role: MatchRole, confirmRelay?: boolean) => Promise<void>;
  playAgain: () => Promise<void>;
  stop: () => void;
  dismiss: () => void;
  clearRematch: () => void;
}

const FlowCtx = createContext<Flow | null>(null);
export function useMatchFlow(): Flow {
  const f = useContext(FlowCtx);
  if (!f) throw new Error("MatchFlow missing");
  return f;
}

/** The phase of the match flow, "idle" where there is no flow (the first-run window has none). */
export function useMatchPhase(): Phase {
  return useContext(FlowCtx)?.phase ?? "idle";
}

/** Steps shown while connecting; `states` are the runtime states that mean "this step is under way". */
export const STEPS: Record<MatchRole, { key: Key; states: string[] }[]> = {
  host: [
    { key: "mc.step.boot", states: ["booting"] },
    { key: "mc.step.lobby", states: ["lobby"] },
    { key: "mc.step.create", states: ["creating-room"] },
    { key: "mc.step.wait", states: ["waiting-for-players"] },
    { key: "mc.step.room", states: ["in-room"] },
    { key: "mc.step.stage", states: ["loading-stage"] },
    { key: "mc.step.match", states: ["in-match"] },
  ],
  join: [
    { key: "mc.step.boot", states: ["booting"] },
    { key: "mc.step.lobby", states: ["lobby"] },
    { key: "mc.step.join", states: ["joining"] },
    { key: "mc.step.room", states: ["in-room"] },
    { key: "mc.step.stage", states: ["loading-stage"] },
    { key: "mc.step.match", states: ["in-match"] },
  ],
};

export function stepIndex(role: MatchRole, state: string | undefined): number {
  const steps = STEPS[role];
  if (state === "results" || state === "finished") return steps.length;
  const i = steps.findIndex((s) => s.states.includes(state ?? ""));
  return i < 0 ? 0 : i;
}

export function MatchFlowProvider({ children }: { children: ReactNode }) {
  const { t } = useI18n();
  const { game } = useApp();
  const [phase, setPhase] = useState<Phase>("idle");
  const [role, setRole] = useState<MatchRole | null>(null);
  const [plan, setPlan] = useState<MatchPlan | null>(null);
  const [run, setRun] = useState<MatchRun | null>(null);
  const [progress, setProgress] = useState<MatchProgress | null>(null);
  const [result, setResult] = useState<MatchResultView | null>(null);
  const [failure, setFailureText] = useState<string | null>(null);
  const [failureFix, setFix] = useState<string | null>(null);
  const [rematch, setRematch] = useState<MatchPlan | null>(null);
  const busy = useRef(false);
  const waiting = useRef<{ present: number; expected: number } | null>(null);
  const setFailure = useCallback((text: string | null, fix: string | null = null) => { setFailureText(text); setFix(fix); }, []);
  /** The runtime's error code as a message and advice; unknown codes keep the runtime's own sentence. */
  const explain = useCallback((code: string | null | undefined, reason: string | null | undefined, extra?: Record<string, string | number>) => {
    const key = `mc.err.${code}` as Key;
    if (code && t(key) !== key) setFailure(t(key, extra), t(`${key}.fix` as Key));
    else setFailure(reason || code || t("mc.fail.noresult"), null);
  }, [t, setFailure]);

  // A match started before the launcher window was re-opened: pick it up again.
  useEffect(() => {
    if (game.state === "running" && game.mode === "match" && phase === "idle") {
      void api.matchLast().then((r) => {
        if (!r || r.started_at !== game.started_at) return;
        setRun(r);
        setRole(r.role);
        setPhase("connecting");
      });
    }
  }, [game.state, game.mode, game.started_at, phase]);

  const start = useCallback(async (p: MatchPlan, r: MatchRole, confirmRelay = false) => {
    const started = await api.matchStart({ code: p.code, role: r, confirm_relay: confirmRelay });
    setPlan(p);
    setRole(r);
    setRun(started);
    setResult(null);
    setFailure(null);
    waiting.current = null;
    setProgress(null);
    setRematch(null);
    setPhase("connecting");
  }, [setFailure]);

  // Poll while connecting.
  useEffect(() => {
    if (phase !== "connecting") return;
    let alive = true;
    const tick = async () => {
      if (busy.current) return;
      busy.current = true;
      try {
        const p = await api.matchPoll();
        if (!alive) return;
        setProgress(p);
        if (p.status?.state === "waiting-for-players" && p.status.expected != null) waiting.current = { present: p.status.present ?? 0, expected: p.status.expected };
        const mine = p.run && run && p.run.match_id === run.match_id && p.run.started_at === run.started_at;
        if (!mine) return;
        if (p.status?.state === "error") {
          explain(p.status.error, p.status.reason ?? p.status.detail, { present: p.status.present ?? waiting.current?.present ?? 1, expected: p.status.expected ?? waiting.current?.expected ?? run.players });
          setPhase("failed");
          return;
        }
        if (p.result_ready) {
          const res = await api.matchResult();
          if (!alive) return;
          if (res && res.error) {
            explain(res.error, res.reason);
            setPhase("failed");
          } else if (res) {
            setResult(res);
            setPhase("results");
          }
          return;
        }
        if (p.game.state === "exited" && p.game.started_at === run.started_at) {
          setFailure(p.game.abnormal ? t("mc.fail.crashed", { code: p.game.exit_code ?? "?" }) : p.game.stopped_by_user ? t("mc.fail.stopped") : t("mc.fail.noresult"));
          setPhase("failed");
        }
      } catch (e) {
        if (alive) setFailure(errorText(toAppError(e), t));
      } finally {
        busy.current = false;
      }
    };
    void tick();
    const id = window.setInterval(() => void tick(), 1000);
    return () => { alive = false; window.clearInterval(id); };
  }, [phase, run, t, explain, setFailure]);

  const stop = useCallback(() => { void api.gameStop().catch(() => {}); }, []);
  const dismiss = useCallback(() => { setPhase("idle"); setFailure(null); }, [setFailure]);

  const playAgain = useCallback(async () => {
    const code = run?.code ?? plan?.code;
    if (!code) return;
    const next = await api.matchNextRound(code);
    setRematch(next);
    setPlan(next);
    setPhase("idle");
  }, [run, plan]);

  const value: Flow = useMemo(
    () => ({ phase, role, plan, run, progress, result, failure, failureFix, rematch, start, playAgain, stop, dismiss, clearRematch: () => setRematch(null) }),
    [phase, role, plan, run, progress, result, failure, failureFix, rematch, start, playAgain, stop, dismiss],
  );

  return (
    <FlowCtx.Provider value={value}>
      {children}
      {phase === "connecting" && <Connecting />}
      {phase === "failed" && <Failed />}
      {phase === "results" && result && <Results />}
    </FlowCtx.Provider>
  );
}

// ---- connecting -------------------------------------------------------------------------------------------------

function fmtClock(s: number): string {
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** The rules of the match as the stage screens recall them. */
function useRecap(): { label: string; value: ReactNode }[] {
  const { t } = useI18n();
  const { advanced, servers } = useApp();
  const f = useMatchFlow();
  const run = f.run;
  const st = f.progress?.status;
  const relay = servers?.servers.find((s) => s.selected && s.relay_host) ?? servers?.servers.find((s) => s.relay_host);
  if (!run) return [];
  return [
    { label: t("mc.recap.players"), value: `${run.players} ${t("mc.players")}` },
    { label: t("mc.rules.mode"), value: t(`mode.${run.mode}` as Key) },
    { label: t("mc.fact.host"), value: run.host },
    { label: t("mc.recap.server"), value: relay?.name ?? f.plan?.relay ?? "—" },
    ...(advanced ? [
      { label: t("mc.fact.id"), value: <span className="mono">{run.match_id}</span> },
      { label: "status", value: st?.source === "file" ? t("mc.src.file") : st?.source === "log" ? t("mc.src.log") : t("mc.src.none") },
    ] : []),
  ];
}

function Connecting() {
  const { t } = useI18n();
  const { advanced } = useApp();
  const f = useMatchFlow();
  const role = f.role ?? "join";
  const steps = STEPS[role];
  const state = f.progress?.status?.state;
  const idx = stepIndex(role, state);
  const inMatch = state === "in-match";
  const run = f.run;
  const [, setNow] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => setNow((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, []);
  const elapsed = run ? Math.max(0, Math.floor(Date.now() / 1000) - run.started_at) : 0;
  const detail = f.progress?.status?.detail;
  const expected = f.progress?.status?.expected ?? run?.players ?? null;
  // Once the game is in the room, everyone is there (the count is only reported while waiting).
  const everyone = ["in-room", "loading-stage", "in-match", "results", "finished"].includes(state ?? "");
  const present = everyone ? expected : f.progress?.status?.present ?? null;
  const recap = useRecap();
  const stepList: StageStep[] = steps.map((s, i) => ({
    key: s.key, label: t(s.key), state: i < idx ? "done" : i === idx ? "now" : "next",
    detail: i === idx ? (s.states.includes("waiting-for-players") && expected != null ? `${present ?? 0}/${expected}` : advanced ? detail ?? null : null) : null,
  }));
  return (
    <StageShell
      titleId="mc-title"
      eyebrow={`${role === "host" ? t("mc.role.host") : t("mc.role.join")} · ${BRAND.name}`}
      title={inMatch ? t("mc.title.inmatch") : t("mc.title.connecting")}
      steps={stepList}
      stepsLabel={t("mc.steps")}
      now={
        <div className="st-now-row">
          <Ring fraction={null} spin={!inMatch} value={fmtClock(elapsed)} caption={t("mc.ring.elapsed")} />
          <div className="st-now-text" aria-live="polite">
            <p className="st-now-step">{idx < steps.length ? t(steps[idx].key) : ""}</p>
            <p className="st-now-title">{inMatch ? t("mc.sub.inmatch") : role === "host" ? t("mc.sub.host") : t("mc.sub.join")}</p>
            <p className="st-now-sub">{inMatch ? t("mc.hint.inmatch") : t("mc.hint.window")}</p>
          </div>
        </div>
      }
      playersTitle={t("res.players")}
      players={
        <div className="st-seats">
          <p className="st-seats-line">{expected != null ? t("mc.players.here", { present: present ?? (idx > 0 ? 1 : 0), expected }) : t("mc.players.wait")}</p>
          {expected != null && (
            <div className="st-seat-row" aria-hidden="true">
              {Array.from({ length: expected }, (_, i) => <span key={i} className={`st-seat ${i < (present ?? (idx > 0 ? 1 : 0)) ? "on" : ""}`} />)}
            </div>
          )}
        </div>
      }
      recapTitle={t("mc.recap.title")}
      recap={recap}
      actions={<button className="btn btn-ghost" onClick={f.stop}><IconStop /> {t("mc.cancel")}</button>}
    />
  );
}

// ---- failure ----------------------------------------------------------------------------------------------------

function Failed() {
  const { t, lang } = useI18n();
  const { advanced, info, status } = useApp();
  const f = useMatchFlow();
  useEffect(() => { window.getSelection()?.removeAllRanges(); }, []);
  const tail = f.progress?.game.tail;
  const st = f.progress?.status;
  const role = f.role ?? "join";
  const recap = useRecap();
  const diag = () => diagnostic({ info, status, game: f.progress?.game, lang }, f.failure || t("mc.fail.noresult"), [
    `private match: ${f.run?.match_id ?? "-"} (${f.role ?? "-"}), ${f.run?.players ?? "?"} players, ${f.run?.mode ?? "?"}`,
    st ? `status: ${st.state} ${st.error ?? ""} ${st.reason ?? ""} ${st.detail ?? ""} (${st.source})` : null,
  ]);
  const retry = async () => {
    if (f.plan && f.role) {
      try { await f.start(f.plan, f.role); } catch { f.dismiss(); }
    } else f.dismiss();
  };
  // Where it stopped: the last step the game reported before its error, when known.
  const lastState = st?.state && st.state !== "error" ? st.state : undefined;
  const stopIdx = lastState ? stepIndex(role, lastState) : -1;
  const stepList: StageStep[] = STEPS[role].map((s, i) => ({ key: s.key, label: t(s.key), state: stopIdx < 0 ? "next" : i < stopIdx ? "done" : i === stopIdx ? "failed" : "next" }));
  return (
    <StageShell
      bad
      titleId="mc-fail-title"
      eyebrow={BRAND.name}
      title={t("mc.fail.title")}
      steps={stepList}
      stepsLabel={t("mc.steps")}
      now={
        <div className="st-now-row">
          <Ring fraction={0} value="" tone="bad" />
          <div className="st-now-text" role="alert">
            <p className="st-now-title">{f.failure || t("mc.fail.noresult")}</p>
            <p className="st-now-sub">{f.failureFix ?? t("mc.fail.hint")}</p>
            <div className="row wrap">
              <button className="btn btn-small" onClick={() => void retry()} disabled={!f.plan}><IconRefresh /> {t("mc.fail.retry")}</button>
              <CopyButton small label={t("diag.copy")} text={diag} />
              {advanced && <button className="btn btn-ghost btn-small" onClick={() => void api.openLogsFolder().catch(() => {})}>{t("play.logs")}</button>}
            </div>
            {advanced && tail && <pre className="log">{tail}</pre>}
          </div>
        </div>
      }
      recapTitle={t("mc.recap.title")}
      recap={recap}
      actions={<button className="btn btn-ghost" onClick={f.dismiss}><IconBack /> {t("mc.back")}</button>}
    />
  );
}

// ---- results ----------------------------------------------------------------------------------------------------

const TEAM_CLASS: Record<string, string> = { alpha: "t-alpha", bravo: "t-bravo", charlie: "t-charlie", delta: "t-delta" };
const teamKey = (n: string) => (TEAM_CLASS[n.toLowerCase()] ?? "t-alpha");

function Results() {
  const { t, lang } = useI18n();
  const f = useMatchFlow();
  const { go } = useApp();
  const r = f.result!;
  const [again, setAgain] = useState<string | null>(null);
  // Stage and mode names from the player's own game data (the engine's list, cached by the launcher); else the raw id.
  const [names, setNames] = useState<{ stages?: Record<string, string>; modes?: Record<string, string> } | null>(null);
  useEffect(() => {
    void import("../community/api").then(({ proto }) => proto.engine()).then((e) => setNames(e?.names ?? null)).catch(() => setNames(null));
  }, []);
  // The stage as the game shows it: its name in the interface language and its picture behind the banner.
  const stageOf = useStageInfo();
  const stage = r.stage_scene ? stageOf(r.stage_scene, names?.stages?.[r.stage_scene]) : null;
  const stageLabel = stage?.name ?? "—";
  const modeLabel = names?.modes?.[r.mode] ?? t(`mode.${r.mode}` as Key);
  const teams = r.teams.filter((x) => x.paint_points > 0 || x.turf_permille > 0 || r.players.some((p) => p.team === x.team));
  const total = teams.reduce((s, x) => s + x.turf_permille, 0) || 1;
  const outcome = r.outcome;
  const players = [...r.players].sort((a, b) => a.team.localeCompare(b.team) || b.paint - a.paint);
  const best = Math.max(1, ...players.map((p) => p.paint));
  const teamName = (n: string) => t(`team.${n.toLowerCase()}` as Key) === `team.${n.toLowerCase()}` ? n : t(`team.${n.toLowerCase()}` as Key);
  const playAgain = async () => {
    try {
      setAgain(null);
      await f.playAgain();
      go("community");
    } catch (e) {
      setAgain(errorText(toAppError(e), t));
    }
  };
  const when = r.end_utc ? formatDateTime(new Date(r.end_utc * 1000), lang) : "";
  return (
    <div className="stage stage-results" role="dialog" aria-modal="true" aria-labelledby="res-title">
      <div className="results">
        <header className={`res-banner ${outcome} ${stage?.picture ? "res-has-stage" : ""}`}>
          {stage?.picture && <img className="res-banner-stage" src={stage.picture} alt="" aria-hidden="true" draggable={false} />}
          <div className="res-banner-ink" aria-hidden="true" />
          <div>
            <div className="eyebrow">{t("res.eyebrow", { mode: modeLabel, stage: stageLabel })}</div>
            <h1 id="res-title" className="display res-title">
              {outcome === "victory" ? t("res.victory") : outcome === "defeat" ? t("res.defeat") : t("res.over")}
            </h1>
            <p className="res-sub">
              {r.winner ? <>{t("res.winner")} <strong className={teamKey(r.winner)}>{teamName(r.winner)}</strong></> : null}
              {when && <> · <span className="muted">{when}</span></>}
              {r.duration_s > 0 && <> · <span className="muted">{fmtClock(Math.round(r.duration_s))}</span></>}
            </p>
          </div>
          <IconCrown className="res-crown" />
        </header>

        <section className="res-teams" aria-label={t("res.teams")}>
          {teams.map((tm) => {
            const pct = Math.round((tm.turf_permille / 10) * 10) / 10;
            const win = tm.team.toLowerCase() === r.winner.toLowerCase();
            return (
              <div key={tm.team} className={`team-card ${teamKey(tm.team)} ${win ? "win" : ""}`}>
                <div className="team-head">
                  <span className="team-name">{teamName(tm.team)}</span>
                  {win && <span className="badge badge-new">{t("res.wins")}</span>}
                  {r.my_team?.toLowerCase() === tm.team.toLowerCase() && <span className="badge badge-soft">{t("res.you")}</span>}
                </div>
                <div className="team-score"><span className="display">{tm.paint_points}</span><span className="muted"> {t("res.points")}</span></div>
                <div className="bar" role="img" aria-label={`${pct}%`}><span style={{ width: `${Math.max(2, (tm.turf_permille / total) * 100)}%` }} /></div>
                <div className="team-pct"><span className="mono">{pct.toFixed(1)} %</span> <span className="muted">{t("res.ofturf")}</span></div>
              </div>
            );
          })}
        </section>

        <section className="card res-table-card" aria-label={t("res.players")}>
          <table className="res-table">
            <thead>
              <tr>
                <th>{t("res.col.player")}</th><th className="num">{t("res.col.paint")}</th><th className="num">{t("res.col.kills")}</th><th className="num">{t("res.col.assists")}</th>
                <th className="num">{t("res.col.deaths")}</th><th className="num">{t("res.col.specials")}</th>
              </tr>
            </thead>
            <tbody>
              {players.map((p) => (
                <tr key={`${p.team}-${p.name}`} className={`${teamKey(p.team)} ${p.self ? "me" : ""} ${p.disconnected ? "dc" : ""}`}>
                  <td>
                    <div className="p-cell">
                      <span className="p-team" />
                      <span className="p-name">{p.name}</span>
                      {p.self && <span className="badge badge-soft">{t("res.you")}</span>}
                      {p.disconnected && <span className="badge badge-live">{t("res.left")}</span>}
                      <span className="p-bar" aria-hidden="true"><span style={{ width: `${(p.paint / best) * 100}%` }} /></span>
                    </div>
                  </td>
                  <td className="num mono">{p.paint}</td><td className="num mono">{p.kills}</td><td className="num mono">{p.assists}</td>
                  <td className="num mono">{p.deaths}</td><td className="num mono">{p.specials}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>

        {again && <div className="notice notice-error" role="alert">{again}</div>}
        <div className="row res-actions">
          <button className="btn btn-big" onClick={() => void playAgain()}><IconRefresh /> {t("res.again")}</button>
          <button className="btn btn-ghost btn-big" onClick={() => { f.dismiss(); go("play"); }}><IconBack /> {t("mc.back")}</button>
          <span className="hint grow">{f.role === "host" ? t("res.again.host") : t("res.again.join")}</span>
        </div>
      </div>
    </div>
  );
}
