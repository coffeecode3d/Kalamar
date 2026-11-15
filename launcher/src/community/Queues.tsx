// Queues (`queues` area): listed exactly as the server gives them, in its order, with what it declares (format,
// rating, party sizes, how mode and stage are chosen, opening hours, requirements, the result policy) and its
// penalties. A queue only 1v1, only 4v4 or anything else is drawn the same way.
import { useEffect, useState } from "react";
import type { J } from "./api";
import { has, noteQueues, req, setLive, useLive } from "./store";
import { duration, useC, type CKey } from "./texts";
import { queueHint } from "./queueHint";
import { ErrorLine, fmtDay, stageFallback, useAction, useLeft, useModeName, useSince, vs } from "./ui";
import { StageStrip } from "../stages";
import { localeOf } from "../locales";
import { IconCheck, IconClose } from "../icons";
import { useApp } from "../ctx";
import { OpenGameNotice } from "./OpenGame";

const SELECTIONS = ["random", "rotation", "fixed", "vote", "game"];

function Ticket({ ticket, queues }: { ticket: J; queues: J[] }) {
  const { c, L } = useC();
  const a = useAction();
  const since = useSince(ticket.created_at);
  const q = queues.find((x: J) => x.id === ticket.queue_id);
  const hint = queueHint(ticket.queue_id, queues, ticket.reason);
  const list = hint.others.map((o) => `${L(queues.find((x: J) => x.id === o.id)?.name) || o.id} (${o.n})`).join(", ");
  return (
    <article className="card cm-ticket" aria-live="polite">
      <div className="cm-pulse" aria-hidden="true" />
      <div className="grow">
        <h2>{ticket.state === "match_found" ? c("q.ticket.found") : c("q.ticket", { name: L(q?.name) || ticket.queue_id })}</h2>
        <p className="muted small">
          {since !== null && c("q.ticket.since", { t: duration(c, since) })}
          {typeof ticket.estimated_wait_s === "number" && ` · ${c("q.wait", { t: duration(c, ticket.estimated_wait_s) })}`}
          {ticket.state === "searching" && hint.here !== null && ` · ${c("q.ticket.here", { n: hint.here })}`}
        </p>
        {ticket.state === "searching" && <p className="small" role="status">{c(`q.hint.${hint.reason}` as CKey, { list })}</p>}
        <OpenGameNotice />
        <ErrorLine text={a.error} />
      </div>
      {ticket.state === "searching" && <button className="btn btn-ghost" disabled={a.busy} onClick={() => void a.run(async () => { await req("DELETE", "/v1/tickets/current"); setLive({ ticket: null }); })}>{c("q.cancel")}</button>}
    </article>
  );
}

function Cooldown({ until }: { until: string }) {
  const { c } = useC();
  const s = useLeft(until);
  return <p className="notice notice-error" role="timer">{c("q.cooldown", { left: duration(c, s ?? 0) })}</p>;
}

/** A time later today as hours and minutes, another day with its date. */
function fmtSoon(at: string | null | undefined, lang: string): string {
  const d = at ? new Date(at) : null;
  if (!d || Number.isNaN(d.getTime())) return fmtDay(at, lang);
  return d.toDateString() === new Date().toDateString() ? d.toLocaleTimeString(localeOf(lang).tag, { hour: "2-digit", minute: "2-digit" }) : fmtDay(at, lang);
}

/** The stages of a pool (`{mode, stage}` pairs), once each; the game's choice only beside named stages. */
function poolStages(pool: J): (string | null)[] {
  const all: (string | null)[] = (Array.isArray(pool) ? pool : []).map((p: J) => (typeof p?.stage === "string" ? p.stage : null));
  const named = [...new Set(all.filter((s): s is string => !!s))];
  return named.length && all.includes(null) ? [...named, null] : named;
}

/** The stages a queue plays on, as the game shows them: its pool, or the current and next entries of its rotation. */
function QueueStages({ q, meta }: { q: J; meta: J }) {
  const { L, t, lang } = useC();
  const rot: J[] = Array.isArray(q.rotation) ? q.rotation.filter((r: J) => r && Array.isArray(r.pool)).slice(0, 2) : [];
  const fallback = (id: string) => stageFallback(L, id, meta);
  if (rot.some((r: J) => poolStages(r.pool).length)) {
    return (
      <div className="cm-q-stages">
        {rot.map((r: J, i: number) => {
          const ids = poolStages(r.pool);
          if (!ids.length) return null;
          const title = i === 0 ? t("stage.rotation.now") : t("stage.rotation.next", { time: fmtSoon(r.starts_at, lang) });
          return (
            <div key={i}>
              <h4>{title}</h4>
              <StageStrip ids={ids} fallback={fallback} label={title} />
            </div>
          );
        })}
      </div>
    );
  }
  const ids = poolStages(q.pool);
  return ids.length ? <div className="cm-q-stages"><StageStrip ids={ids} fallback={fallback} label={t("stage.label")} /></div> : null;
}

function QueueCard({ q, meta, busy, onJoin }: { q: J; meta: J; busy: boolean; onJoin: () => void }) {
  const { c, L, lang } = useC();
  const { advanced } = useApp();
  const modeName = useModeName();
  const policy = (meta?.result_policies ?? []).find((p: J) => p?.id === q.result_policy);
  const selection = SELECTIONS.includes(q.selection) ? c(`q.sel.${q.selection}` as CKey) : c("q.sel.other");
  const modes = [...new Set((q.pool ?? []).map((p: J) => p?.mode).filter(Boolean))] as string[];
  const open = q.availability?.open !== false;
  const next = (q.availability?.windows ?? []).find((w: J) => w?.starts_at);
  const shrink = Array.isArray(q.min_teams) && Array.isArray(q.teams) && (q.min_teams[0] !== q.teams[0] || q.min_teams[1] !== q.teams[1]);
  return (
    <article className={`card cm-queue ${q.eligible === false ? "cm-off" : ""}`}>
      <header className="row">
        <div className="grow">
          <h3>{L(q.name) || q.id}</h3>
          <p className="cm-q-meta">
            {Array.isArray(q.teams) && <strong>{vs(c, q.teams)}</strong>}
            <span>{q.rated ? c("q.rated") : c("q.unrated")}</span>
            <span className={open ? "ok-dot" : "bad-dot"}>{open ? c("q.open") : c("q.closed")}</span>
          </p>
        </div>
        <button className="btn" disabled={busy || q.eligible === false || !open} onClick={onJoin}>{c("q.join")}</button>
      </header>
      {L(q.description) && <p>{L(q.description)}</p>}
      {q.draft && typeof q.draft === "object" && (L(q.draft.label) || L(q.draft.summary)) && (
        <p className="small cm-q-draft"><strong>{c("q.draft", { name: L(q.draft.label) || String(q.draft.id ?? "") })}</strong>{L(q.draft.summary) ? ` — ${L(q.draft.summary).slice(0, 240)}` : ""}</p>
      )}
      <QueueStages q={q} meta={meta} />
      <p className="small muted">
        {modes.map((m) => modeName(m, meta)).join(", ")}{modes.length ? " · " : ""}{selection}
        {shrink && ` · ${c("q.shrink", { a: q.min_teams[0], b: q.min_teams[1] })}`}
        {has("parties") && q.party && ` · ${c("q.party", { min: q.party.min ?? 1, max: q.party.max ?? 1 })}`}
      </p>
      <p className="small muted">
        {[
          typeof q.estimated_wait_s === "number" ? c("q.wait", { t: duration(c, q.estimated_wait_s) }) : null,
          typeof q.searching === "number" ? c("q.searching", { n: q.searching }) : null,
          !open && next ? c("q.next", { date: fmtDay(next.starts_at, lang) }) : null,
        ].filter(Boolean).join(" · ")}
      </p>
      {Array.isArray(q.requirements) && q.requirements.length > 0 && (
        <ul className="cm-reqs" aria-label={c("q.requirements")}>
          {q.requirements.map((r: J) => <li key={r.id} className={r.met ? "ok-text" : "bad-text"}>{r.met ? <IconCheck /> : <IconClose />} {L(r.label) || r.id}</li>)}
        </ul>
      )}
      {advanced && policy && <p className="hint" title={L(policy.summary)}>{c("q.policy", { name: L(policy.name) || policy.id })} — {L(policy.summary)}</p>}
    </article>
  );
}

export function Queues() {
  const { c, L } = useC();
  const live = useLive();
  const meta = live.overview?.meta;
  const [queues, setQueues] = useState<J[] | null>(null);
  const a = useAction();
  const load = useAction();
  useEffect(() => {
    void load.run(async () => {
      const r = await req("GET", "/v1/queues");
      setQueues([...(r.items ?? [])].sort((x: J, y: J) => (x.order ?? 0) - (y.order ?? 0)));
    });
  }, [live.rev.queues, live.ticket?.id, live.ticket === null]); // eslint-disable-line react-hooks/exhaustive-deps
  // Whether each queue is ranked and its team sizes, for the Discord presence's "looking for a ranked match".
  useEffect(() => { if (queues) noteQueues(queues); }, [queues]);
  // While searching, the counts of the queues (`searching`) are read again every few seconds.
  const waiting = live.ticket?.state === "searching";
  useEffect(() => {
    if (!waiting) return;
    const id = window.setInterval(() => {
      void req("GET", "/v1/queues").then((r) => setQueues([...(r.items ?? [])].sort((x: J, y: J) => (x.order ?? 0) - (y.order ?? 0)))).catch(() => {});
    }, 5000);
    return () => window.clearInterval(id);
  }, [waiting]);
  const cooldown = (live.me?.restrictions ?? []).find((r: J) => r?.kind === "queue_cooldown" && r.until && Date.parse(r.until) > Date.now());
  const pen = meta?.queues?.penalties;
  const leaderOnly = has("parties") && live.party && live.party.leader !== live.me?.id;
  const join = (q: J) => void a.run(async () => setLive({ ticket: await req("POST", `/v1/queues/${encodeURIComponent(q.id)}/tickets`, {}) }), (f) => ({ left: f.retry_after_s != null ? duration(c, f.retry_after_s) : "" }));
  // 1.1.10: a match whose report this player sent no longer holds them (`you.free`): they may search again.
  const held = !!live.match && live.match.you?.free !== true;
  return (
    <div className="stack">
      {live.ticket && <Ticket ticket={live.ticket} queues={queues ?? []} />}
      {held && <p className="hint">{c("err.in_match")}</p>}
      {cooldown && <Cooldown until={cooldown.until} />}
      {leaderOnly && <p className="hint">{c("q.party.leader")}</p>}
      <ErrorLine text={a.error || load.error} />
      <h2 className="display">{c("q.list")}</h2>
      {queues === null ? <span className="skeleton" style={{ width: "100%", height: "6rem" }} /> : queues.length === 0 ? <p className="muted">{c("q.none")}</p> : (
        <div className="cm-grid">
          {queues.map((q) => <QueueCard key={q.id} q={q} meta={meta} busy={a.busy || !!live.ticket || held || !!leaderOnly} onJoin={() => join(q)} />)}
        </div>
      )}
      {pen && L(pen.summary) && (
        <details className="advanced cm-pen">
          <summary>{c("q.penalties")}</summary>
          <p>{L(pen.summary)}</p>
          {Array.isArray(pen.cooldowns_s) && <p className="small muted">{pen.cooldowns_s.map((s: number) => duration(c, s)).join(" → ")}</p>}
        </details>
      )}
    </div>
  );
}
