// Private rooms (`rooms` area): the creation form the server declares, joining by code, teams, ready, start.
import { play } from "../sound/engine";
import { useState } from "react";
import type { J } from "./api";
import { applyMatch, has, myId, req, setLive, useLive } from "./store";
import { useC, type CFn } from "./texts";
import { ErrorLine, FieldForm, KNOWN_TYPES, Who, fieldBody, fieldDefaults, fmtDay, stageFallback, unsupportedField, useAction, useModeName, useStageName, vs } from "./ui";
import { StageCard } from "../stages";
import { MatchCard } from "./Match";
import { GameCheckNotice, useGameBlocked } from "./GameCheck";
import { IconCopy } from "../icons";
import { useApp } from "../ctx";
import { RoomStands } from "./Stands";
import { OpenGameNotice } from "./OpenGame";

/** Rated or not, and (advanced mode) how the server decides the result. */
function Policy({ meta, id, ladder }: { meta: J; id: string | null | undefined; ladder: J }) {
  const { c, L } = useC();
  const { advanced } = useApp();
  const p = (meta?.result_policies ?? []).find((x: J) => x?.id === id);
  if (ladder && !(advanced && p)) return null;
  return (
    <p className="hint">
      {ladder ? null : c("room.unrated")}
      {advanced && p && <>{ladder ? "" : " · "}<span title={L(p.summary)}>{c("room.policy", { name: L(p.name) || p.id })}</span></>}
    </p>
  );
}

/** What a room's options are, as the declaration names them. */
function optionText(f: J, room: J, meta: J, c: CFn, L: (x: J) => string, modeName: ReturnType<typeof useModeName>, stageName: ReturnType<typeof useStageName>): string | null {
  const v = room?.options?.[f.key];
  switch (f.type) {
    case "teams":
      return vs(c, v);
    case "mode":
      return v ? modeName(v, meta, (f.values ?? []).find((x: J) => x.id === v)?.label) : null;
    case "stage":
      return v === null || v === undefined ? (f.nullable ? L(f.null_label) || c("field.null") : null) : stageName(v, meta, (f.values ?? []).find((x: J) => x.id === v)?.label);
    case "string":
      return f.secret ? (room?.has_password ? c("room.password") : null) : v ? String(v) : null;
    case "boolean":
      return v === undefined ? null : v ? c("field.yes") : c("field.no");
    case "enum":
      return v === undefined || v === null ? null : L((f.values ?? []).find((x: J) => x.id === v)?.label) || String(v);
    case "integer":
      return v === undefined || v === null ? null : String(v);
  }
  return null;
}

function NoRoom({ meta }: { meta: J }) {
  const { c, L } = useC();
  const live = useLive();
  const decl = meta?.rooms ?? {};
  const fields: J[] = (decl.options ?? []).filter((f: J) => f && f.key);
  const [values, setValues] = useState<Record<string, unknown>>(() => fieldDefaults(fields));
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");
  // 1.1.7: join the room's stands (only when the server has them).
  const [watch, setWatch] = useState(false);
  const stands = !!decl.spectators;
  const create = useAction();
  const join = useAction();
  const unknown = unsupportedField(fields);
  const doJoin = (cd: string, pw?: string) => void join.run(async () => setLive({ room: await req("POST", "/v1/rooms/join", { code: cd, ...(pw ? { password: pw } : {}), ...(stands && watch ? { seat: "spectator" } : {}) }) }));
  return (
    <div className="cm-grid">
      <article className="card">
        <h2>{c("room.create")}</h2>
        {unknown ? (
          <p className="bad-text">{c("field.too_old", { key: unknown })}</p>
        ) : (
          <form onSubmit={(e) => { e.preventDefault(); void create.run(async () => setLive({ room: await req("POST", "/v1/rooms", { options: fieldBody(fields, values) }) })); }}>
            <FieldForm fields={fields} values={values} set={(k, v) => setValues((o) => ({ ...o, [k]: v }))} meta={meta} idPrefix="room" />
            <Policy meta={meta} id={decl.result_policy} ladder={decl.ladder} />
            <ErrorLine text={create.error} />
            <button className="btn" type="submit" disabled={create.busy}>{c("room.create.go")}</button>
          </form>
        )}
      </article>
      <article className="card">
        <h2>{c("room.join")}</h2>
        <form className="stack" onSubmit={(e) => { e.preventDefault(); if (code.trim()) doJoin(code.trim(), password); }}>
          <label className="field-label" htmlFor="room-code">{c("room.join.code")}</label>
          <input id="room-code" className="mono cm-code-input" value={code} autoComplete="off" maxLength={32} onChange={(e) => setCode(e.target.value.toUpperCase())} />
          <label className="field-label" htmlFor="room-pw">{c("room.join.password")}</label>
          <input id="room-pw" type="password" value={password} autoComplete="off" maxLength={64} onChange={(e) => setPassword(e.target.value)} />
          {stands && (
            <label className="check">
              <input type="checkbox" checked={watch} onChange={(e) => setWatch(e.target.checked)} /> {c("st.join")}
              <span className="hint"> · {c("st.join.hint")}</span>
            </label>
          )}
          <ErrorLine text={join.error} />
          <button className="btn btn-alt" type="submit" disabled={join.busy || !code.trim()}>{c("room.join.go")}</button>
        </form>
      </article>
      {live.roomInvites.length > 0 && (
        <article className="card cm-wide">
          <h2>{c("room.invites")}</h2>
          <ul className="cm-list">
            {live.roomInvites.map((i: J) => (
              <li key={i.room_id} className="cm-row">
                <span className="grow">{c("room.invite.from", { name: i.from?.display_name ?? "?", code: i.code ?? "" })}</span>
                <button className="btn btn-small" onClick={() => doJoin(i.code)}>{c("room.invite.join")}</button>
                <button className="btn btn-small btn-ghost" onClick={() => setLive((l) => ({ roomInvites: l.roomInvites.filter((x: J) => x.room_id !== i.room_id) }))}>×</button>
              </li>
            ))}
          </ul>
        </article>
      )}
      {L(meta?.rules?.summary) && <p className="hint cm-wide">{L(meta.rules.summary)}</p>}
    </div>
  );
}

function InviteFriend({ room }: { room: J }) {
  const { c } = useC();
  const [friends, setFriends] = useState<J[] | null>(null);
  const [pick, setPick] = useState("");
  const a = useAction();
  const [sent, setSent] = useState(false);
  if (friends === null) {
    return <button className="btn btn-ghost btn-small" onClick={() => void a.run(async () => setFriends((await req("GET", "/v1/friends")).items ?? []))}>{c("room.invite.friend")}</button>;
  }
  return (
    <div className="row wrap">
      <select value={pick} onChange={(e) => setPick(e.target.value)} aria-label={c("room.invite.friend")}>
        <option value="">—</option>
        {friends.map((f: J) => <option key={f.user?.id} value={f.user?.id}>{f.user?.display_name}</option>)}
      </select>
      <button className="btn btn-small" disabled={!pick || a.busy} onClick={() => void a.run(async () => { await req("POST", `/v1/rooms/${encodeURIComponent(room.id)}/invites`, { user_id: pick }); setSent(true); })}>{c("p.invite")}</button>
      {sent && <span className="ok-text small">{c("room.invite.sent")}</span>}
      <ErrorLine text={a.error} />
    </div>
  );
}

function RoomView({ room, meta }: { room: J; meta: J }) {
  const { c, L, lang, t } = useC();
  const { advanced } = useApp();
  const modeName = useModeName();
  const stageName = useStageName();
  const live = useLive();
  const blocked = useGameBlocked();
  const me = myId();
  const owner = room.owner === me;
  const a = useAction();
  const decl = meta?.rooms ?? {};
  const fields: J[] = (decl.options ?? []).filter((f: J) => f && KNOWN_TYPES.includes(f.type));
  // The room's stage as a card (the game's picture and name), the other options in words.
  const stageField = fields.find((f: J) => f.type === "stage");
  const stage: string | null = typeof room.options?.[stageField?.key] === "string" ? room.options[stageField.key] : null;
  const cap: number[] = Array.isArray(room.options?.teams) ? room.options.teams : [4, 4];
  const members: J[] = room.members ?? [];
  const mine = members.find((m: J) => m.user?.id === me);
  const open = room.state === "open";
  const id = room.id;
  const apply = (r: J) => { if (r && r.id) setLive({ room: r }); };
  const others = members.filter((m: J) => m.user?.id !== room.owner);
  // 1.1.7: spectators are not players: the minimum and both teams count players only.
  const players = members.filter((m: J) => m.team === "alpha" || m.team === "bravo");
  const canStart = open && owner && others.every((m: J) => m.ready) && players.length >= (decl.min_players ?? 2) && ["alpha", "bravo"].every((t) => members.some((m: J) => m.team === t));
  // Why the start is not possible yet, in words: who is not ready, else the room's rule.
  const notReady = others.filter((m: J) => !m.ready).map((m: J) => String(m.user?.display_name ?? "?"));
  const startWhy = players.length >= (decl.min_players ?? 2) && ["alpha", "bravo"].every((t) => members.some((m: J) => m.team === t)) && notReady.length
    ? c("room.wait.ready", { names: notReady.join(", ") })
    : c("room.start.hint", { n: decl.min_players ?? 2 });
  const team = (t: "alpha" | "bravo", size: number) => {
    const list = members.filter((m: J) => m.team === t);
    return (
      <div className={`cm-team cm-team-${t}`}>
        <h3>{t === "alpha" ? c("room.alpha") : c("room.bravo")} <span className="muted small">{list.length}/{size}</span></h3>
        <ul className="cm-list">
          {list.map((m: J) => (
            <li key={m.user?.id} className="cm-row">
              <Who user={m.user} me={m.user?.id === me} crown={m.user?.id === room.owner} />
              <span className={`pill ${m.ready || m.user?.id === room.owner ? "pill-ok" : "pill-neutral"}`}>{m.ready || m.user?.id === room.owner ? c("room.ready") : c("room.notready")}</span>
              {owner && open && m.user?.id !== me && (
                <span className="cm-actions">
                  <button className="link small" disabled={a.busy} onClick={() => void a.run(async () => apply(await req("PUT", `/v1/rooms/${encodeURIComponent(id)}/members/${encodeURIComponent(m.user.id)}`, { team: t === "alpha" ? "bravo" : "alpha" })))}>{c("room.move")}</button>
                  <button className="link small" disabled={a.busy} onClick={() => void a.run(async () => apply(await req("POST", `/v1/rooms/${encodeURIComponent(id)}/owner`, { user_id: m.user.id })))}>{c("room.promote")}</button>
                  <button className="link small cm-danger" disabled={a.busy} onClick={() => void a.run(async () => apply(await req("POST", `/v1/rooms/${encodeURIComponent(id)}/kick`, { user_id: m.user.id })))}>{c("room.kick")}</button>
                </span>
              )}
            </li>
          ))}
          {Array.from({ length: Math.max(0, size - list.length) }, (_, i) => <li key={`e${i}`} className="cm-row cm-empty">{c("room.empty")}</li>)}
        </ul>
      </div>
    );
  };
  return (
    <div className="stack">
      <article className="card cm-room-head">
        <div className="grow">
          <div className="eyebrow">{c("room.code")}</div>
          <div className="cm-code mono">{room.code}</div>
          <p className="cm-q-meta">
            {fields.filter((f: J) => f !== stageField).map((f: J) => {
              const txt = optionText(f, room, meta, c, L, modeName, stageName);
              return txt ? <span key={f.key}><span className="muted">{L(f.label) || f.key}</span> <strong>{txt}</strong></span> : null;
            })}
          </p>
          <Policy meta={meta} id={decl.result_policy} ladder={decl.ladder} />
          {advanced && room.closes_at && <p className="hint">{c("room.closes", { date: fmtDay(room.closes_at, lang) })}</p>}
          <button className="btn btn-small btn-ghost cm-room-copy" onClick={() => void navigator.clipboard?.writeText(room.code)}><IconCopy /> {c("room.copy")}</button>
        </div>
        {stageField && (
          <figure className="cm-room-stage">
            <figcaption className="eyebrow">{L(stageField.label) || c("m.recap.stage")}</figcaption>
            <StageCard id={stage} size="md"
              fallback={stage ? stageFallback(L, stage, meta, (stageField.values ?? []).find((x: J) => x.id === stage)?.label) : stageField.nullable ? L(stageField.null_label) || c("field.null") : undefined}
              sub={stage ? undefined : t("stage.random.sub")} />
          </figure>
        )}
      </article>
      {room.state !== "open" && <p className="notice notice-info">{c("room.state.in_match")}</p>}
      {open && <OpenGameNotice />}
      <div className="cm-teams">
        {team("alpha", cap[0] ?? 4)}
        {team("bravo", cap[1] ?? 4)}
      </div>
      <RoomStands room={room} meta={meta} />
      <ErrorLine text={a.error} />
      <GameCheckNotice />
      <div className="cm-start">
        {owner ? (
          <>
            <button className="btn btn-big" disabled={!canStart || blocked || a.busy} onClick={() => { play("confirm"); void a.run(async () => { applyMatch(await req("POST", `/v1/rooms/${encodeURIComponent(id)}/start`)); }); }}>{c("room.start")}</button>
            {open && !canStart && <p className="cm-why">{startWhy}</p>}
          </>
        ) : (
          mine && (
            <>
              <button className={`btn btn-big ${mine.ready ? "btn-ghost" : ""}`} disabled={!open || (blocked && !mine.ready) || a.busy} onClick={() => { if (!mine.ready) play("confirm"); void a.run(async () => apply(await req("POST", `/v1/rooms/${encodeURIComponent(id)}/ready`, { ready: !mine.ready }))); }}>{mine.ready ? c("room.ready.undo") : c("room.ready.go")}</button>
              {open && mine.ready && <p className="cm-why">{c("room.wait.host")}</p>}
            </>
          )
        )}
      </div>
      <div className="row wrap">
        {mine && open && mine.team !== "spectator" && <button className="btn btn-ghost btn-small" disabled={a.busy} onClick={() => void a.run(async () => apply(await req("PUT", `/v1/rooms/${encodeURIComponent(id)}/members/${encodeURIComponent(me ?? "")}`, { team: mine.team === "alpha" ? "bravo" : "alpha" })))}>{c("room.switch")}</button>}
        {decl.invites && has("friends") && open && <InviteFriend room={room} />}
        <button className="btn btn-ghost btn-small" disabled={a.busy} onClick={() => void a.run(async () => { await req("POST", `/v1/rooms/${encodeURIComponent(id)}/leave`); setLive({ room: null }); })}>{c("room.leave")}</button>
      </div>
      {(live.match?.origin?.room_id === id || room.match_id) && live.match && <MatchCard match={live.match} />}
    </div>
  );
}

export function Rooms() {
  const live = useLive();
  const meta = live.overview?.meta;
  if (live.room && live.room.state !== "closed") return <RoomView room={live.room} meta={meta} />;
  return <NoRoom meta={meta} />;
}
