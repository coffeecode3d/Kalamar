// The full-frame screen of a match on its way (community match and private match by code): the steps on the left; on
// the right the step under way in large with its countdown ring and sentence, one card per player, and the match's
// rules. Stacks into one column in a narrow window.
import type { ReactNode } from "react";
import { IconAlert, IconCheck, IconClose, Mark } from "../icons";

export type StepState = "done" | "now" | "next" | "failed";
export interface StageStep { key: string; label: string; state: StepState; detail?: string | null }
export interface StagePlayer { key: string; name: string; me?: boolean; team?: string | null; teamLabel?: string | null; step: string; progress: number; total: number; bad?: boolean; rank?: ReactNode }

/** A ring that empties as a deadline nears (`fraction` 1 → 0), or turns slowly when nothing is known (`spin`). */
export function Ring({ fraction, value, caption, spin, tone = "ok" }: { fraction: number | null; value: string; caption?: string; spin?: boolean; tone?: "ok" | "bad" | "done" }) {
  const r = 52;
  const len = 2 * Math.PI * r;
  const f = tone === "bad" || tone === "done" ? 1 : fraction === null ? 0.28 : Math.max(0, Math.min(1, fraction));
  return (
    <div className={`ring-box ring-${tone} ${spin ? "ring-spin" : ""}`} role="timer" aria-label={`${value}${caption ? ` ${caption}` : ""}`}>
      <svg viewBox="0 0 120 120" aria-hidden="true">
        <circle className="ring-track" cx="60" cy="60" r={r} />
        <circle className="ring-ink" cx="60" cy="60" r={r} strokeDasharray={`${len * f} ${len}`} transform="rotate(-90 60 60)" />
      </svg>
      <div className="ring-text">
        {tone === "done" ? <IconCheck className="ring-icon" /> : tone === "bad" ? <IconAlert className="ring-icon" /> : <span className="ring-value">{value}</span>}
        {caption && <span className="ring-caption">{caption}</span>}
      </div>
    </div>
  );
}

export function StageShell({ bad, eyebrow, title, titleId, steps, stepsLabel, now, art, playersTitle, players, recapTitle, recap, actions, hint }: {
  bad?: boolean; eyebrow: string; title: string; titleId: string; steps: StageStep[]; stepsLabel: string; now: ReactNode;
  /** The match's stage picture (from the player's game files), faded in behind the step under way. */
  art?: string | null;
  playersTitle?: string; players?: ReactNode; recapTitle?: string; recap?: { label: string; value: ReactNode }[]; actions: ReactNode; hint?: string;
}) {
  return (
    <div className={`stage stage2 ${bad ? "stage-bad" : ""}`} role={bad ? "alertdialog" : "dialog"} aria-modal="true" aria-labelledby={titleId}>
      <header className="st-head">
        <div className="st-head-text">
          <div className="eyebrow">{eyebrow}</div>
          <h1 id={titleId} className="display stage-title">{title}</h1>
        </div>
        <div className="st-head-actions">{actions}{hint && <span className="hint">{hint}</span>}</div>
      </header>
      <div className="st-cols">
        <section className="st-steps" aria-label={stepsLabel}>
          <ol className="steps-v">
            {steps.map((s) => (
              <li key={s.key} className={`step-v ${s.state}`} aria-current={s.state === "now" ? "step" : undefined}>
                <span className="step-dot">{s.state === "done" ? <IconCheck /> : s.state === "failed" ? <IconClose /> : <span />}</span>
                <span className="step-text">{s.label}</span>
                {s.detail && <span className="step-detail mono">{s.detail}</span>}
              </li>
            ))}
          </ol>
          <div className="st-mark" aria-hidden="true">
            {!bad && <><span className="ripple r1" /><span className="ripple r2" /></>}
            <Mark className="st-mark-svg" size="100%" />
          </div>
        </section>
        <div className="st-main">
          <section className={`st-now ${art ? "st-now-art" : ""}`}>
            {art && <img className="st-art" src={art} alt="" aria-hidden="true" draggable={false} />}
            {now}
          </section>
          {players && (
            <section className="st-block" aria-label={playersTitle}>
              {playersTitle && <h2 className="st-block-title">{playersTitle}</h2>}
              {players}
            </section>
          )}
          {recap && recap.length > 0 && (
            <section className="st-block" aria-label={recapTitle}>
              {recapTitle && <h2 className="st-block-title">{recapTitle}</h2>}
              <dl className="st-recap">
                {recap.map((x) => <div key={x.label}><dt>{x.label}</dt><dd>{x.value}</dd></div>)}
              </dl>
            </section>
          )}
        </div>
      </div>
    </div>
  );
}

/** The players' cards: initial in the team's color, name, step in words, progress. */
export function PlayerCards({ players, youLabel }: { players: StagePlayer[]; youLabel: string }) {
  return (
    <ul className="st-players">
      {players.map((p) => (
        <li key={p.key} className={`st-player ${p.team ? `t-${p.team}` : ""} ${p.bad ? "bad" : ""} ${p.me ? "me" : ""}`}>
          <span className="st-avatar" aria-hidden="true">{p.name.slice(0, 1).toUpperCase()}</span>
          <div className="st-player-text">
            <span className="st-player-line">
              <span className="st-player-name">{p.name}{p.me && <span className="st-you"> {youLabel}</span>}</span>
              {p.rank}
            </span>
            <span className="st-player-step">{p.teamLabel ? <span className="st-team">{p.teamLabel}</span> : null}{p.step}</span>
            <span className="st-bar" role="img" aria-label={`${p.progress}/${p.total}`}>
              {Array.from({ length: p.total }, (_, i) => <span key={i} className={i < p.progress ? "done" : i === p.progress ? "now" : ""} />)}
            </span>
          </div>
        </li>
      ))}
    </ul>
  );
}

/** m:ss for a ring. */
export function clock(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}
