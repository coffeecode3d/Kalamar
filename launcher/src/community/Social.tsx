// Party (`parties`), friends and presence (`friends`, `presence`). Each part is shown only when the server declares
// its area.
import { useEffect, useState } from "react";
import type { J } from "./api";
import { has, myId, req, setLive, useLive } from "./store";
import { useC, type CKey } from "./texts";
import { ErrorLine, Who, fmtDay, useAction } from "./ui";
import { IconCopy, IconRefresh } from "../icons";

function useFriends(): [J[] | null, () => void] {
  const live = useLive();
  const [list, setList] = useState<J[] | null>(null);
  const [n, setN] = useState(0);
  useEffect(() => {
    if (!has("friends")) return;
    void req("GET", "/v1/friends").then((r: J) => setList(r.items ?? [])).catch(() => setList([]));
  }, [live.rev.friends, n]);
  return [list, () => setN((x) => x + 1)];
}

function Presence({ id, initial }: { id: string; initial: string | undefined }) {
  const { c } = useC();
  const live = useLive();
  if (!has("presence")) return null;
  const p = live.presence[id] ?? initial ?? "offline";
  const key = `f.presence.${p}` as CKey;
  return <span className={`cm-presence cm-presence-${p}`}>{c(key) === key ? c("f.presence.online") : c(key)}</span>;
}

export function Party() {
  const { c } = useC();
  const live = useLive();
  const me = myId();
  const a = useAction();
  const [friends] = useFriends();
  const [pick, setPick] = useState("");
  const party = live.party;
  const apply = (p: J) => setLive({ party: p && p.id ? p : null });
  const leader = party?.leader === me;
  return (
    <div className="stack">
      {live.partyInvites.length > 0 && (
        <article className="card">
          <h2>{c("p.invites")}</h2>
          <ul className="cm-list">
            {live.partyInvites.map((i: J) => (
              <li key={i.party_id} className="cm-row">
                <span className="grow">{c("p.invite.from", { name: i.from?.display_name ?? "?" })}</span>
                <button className="btn btn-small" disabled={a.busy} onClick={() => void a.run(async () => { apply(await req("POST", `/v1/parties/${encodeURIComponent(i.party_id)}/join`)); setLive((l) => ({ partyInvites: l.partyInvites.filter((x: J) => x.party_id !== i.party_id) })); })}>{c("p.join")}</button>
                <button className="btn btn-small btn-ghost" disabled={a.busy} onClick={() => void a.run(async () => { await req("POST", `/v1/parties/${encodeURIComponent(i.party_id)}/decline`); setLive((l) => ({ partyInvites: l.partyInvites.filter((x: J) => x.party_id !== i.party_id) })); })}>{c("p.decline")}</button>
              </li>
            ))}
          </ul>
        </article>
      )}
      {!party ? (
        <article className="card">
          <p className="muted">{c("p.none")}</p>
          <button className="btn" disabled={a.busy} onClick={() => void a.run(async () => apply(await req("POST", "/v1/party")))}>{c("p.create")}</button>
        </article>
      ) : (
        <article className="card">
          <h2>{c("c.tab.party")} <span className="muted small">{c("p.size", { n: party.members?.length ?? 0, max: party.max_size ?? "?" })}</span></h2>
          {party.ticket && <p className="notice notice-info">{c("p.queueing")}</p>}
          <ul className="cm-list">
            {(party.members ?? []).map((m: J) => (
              <li key={m.user?.id} className="cm-row">
                <Who user={m.user} me={m.user?.id === me} crown={m.user?.id === party.leader} />
                {m.user?.id === party.leader && <span className="pill pill-neutral">{c("p.leader")}</span>}
                {leader && m.user?.id !== me && (
                  <span className="cm-actions">
                    <button className="btn btn-small btn-ghost" disabled={a.busy} onClick={() => void a.run(async () => apply(await req("POST", "/v1/party/leader", { user_id: m.user.id })))}>{c("p.promote")}</button>
                    <button className="btn btn-small btn-ghost" disabled={a.busy} onClick={() => void a.run(async () => apply(await req("POST", "/v1/party/kick", { user_id: m.user.id })))}>{c("p.kick")}</button>
                  </span>
                )}
              </li>
            ))}
            {(party.invites ?? []).map((i: J) => (
              <li key={i.user?.id} className="cm-row cm-empty">
                <Who user={i.user} />
                <span className="pill pill-neutral">{c("p.invited")}</span>
              </li>
            ))}
          </ul>
          {leader && (
            <div className="row wrap">
              {friends && friends.length > 0 ? (
                <>
                  <select value={pick} onChange={(e) => setPick(e.target.value)} aria-label={c("p.invite")}>
                    <option value="">—</option>
                    {friends.filter((f: J) => !(party.members ?? []).some((m: J) => m.user?.id === f.user?.id)).map((f: J) => <option key={f.user?.id} value={f.user?.id}>{f.user?.display_name}</option>)}
                  </select>
                  <button className="btn btn-small" disabled={!pick || a.busy} onClick={() => void a.run(async () => apply(await req("POST", "/v1/party/invites", { user_id: pick })))}>{c("p.invite")}</button>
                </>
              ) : (
                has("friends") && <span className="hint">{c("p.friends.none")}</span>
              )}
            </div>
          )}
          <button className="btn btn-ghost" disabled={a.busy} onClick={() => void a.run(async () => { await req("POST", "/v1/party/leave"); apply(null); })}>{c("p.leave")}</button>
        </article>
      )}
      <ErrorLine text={a.error} />
    </div>
  );
}

export function Friends() {
  const { c, lang } = useC();
  const live = useLive();
  const a = useAction();
  const [friends, again] = useFriends();
  const [requests, setRequests] = useState<J | null>(null);
  const [code, setCode] = useState("");
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => {
    void req("GET", "/v1/friends/requests").then(setRequests).catch(() => setRequests({ incoming: [], outgoing: [] }));
  }, [live.rev.friends]);
  const [blocks, setBlocks] = useState<J[]>([]);
  const [nb, setNb] = useState(0);
  useEffect(() => {
    if (!has("moderation")) return;
    void req("GET", "/v1/blocks").then((r: J) => setBlocks(r.items ?? [])).catch(() => setBlocks([]));
  }, [live.rev.friends, nb]);
  const refresh = () => { again(); void req("GET", "/v1/friends/requests").then(setRequests).catch(() => {}); };
  const myCode: string | null = live.me?.friend_code ?? null;
  return (
    <div className="stack">
      <div className="cm-grid">
        <article className="card">
          <h2>{c("f.code")}</h2>
          {myCode && <div className="cm-code mono">{myCode}</div>}
          <div className="row wrap">
            {myCode && <button className="btn btn-small btn-ghost" onClick={() => void navigator.clipboard?.writeText(myCode)}><IconCopy /> {c("room.copy")}</button>}
            <button className="btn btn-small btn-ghost" disabled={a.busy} onClick={() => { if (window.confirm(c("f.code.new.confirm"))) void a.run(async () => { const r = await req("POST", "/v1/me/friend-code"); setLive((l) => ({ me: l.me ? { ...l.me, friend_code: r.friend_code } : l.me })); }); }}><IconRefresh /> {c("f.code.new")}</button>
          </div>
        </article>
        <article className="card">
          <h2>{c("f.add")}</h2>
          <form className="stack" onSubmit={(e) => {
            e.preventDefault();
            setNote(null);
            void a.run(async () => { const r = await req("POST", "/v1/friends/requests", { friend_code: code.trim() }); setNote((r?.kind ? r.kind === "friend" : !!r?.since) ? c("f.add.friends") : c("f.add.sent")); setCode(""); refresh(); });
          }}>
            <label className="field-label" htmlFor="f-code">{c("f.add.code")}</label>
            <input id="f-code" className="mono" value={code} maxLength={32} placeholder="XXXX-XXXX-XXXX" onChange={(e) => setCode(e.target.value.toUpperCase())} />
            <button className="btn" type="submit" disabled={a.busy || !code.trim()}>{c("f.add.go")}</button>
            {note && <p className="ok-text">{note}</p>}
          </form>
        </article>
      </div>
      <ErrorLine text={a.error} />
      {requests && (requests.incoming?.length > 0 || requests.outgoing?.length > 0) && (
        <article className="card">
          {requests.incoming?.length > 0 && <h2>{c("f.incoming")}</h2>}
          <ul className="cm-list">
            {(requests.incoming ?? []).map((r: J) => (
              <li key={r.id} className="cm-row">
                <Who user={r.user} />
                <span className="grow" />
                <button className="btn btn-small" disabled={a.busy} onClick={() => void a.run(async () => { await req("POST", `/v1/friends/requests/${encodeURIComponent(r.id)}/accept`); refresh(); })}>{c("f.accept")}</button>
                <button className="btn btn-small btn-ghost" disabled={a.busy} onClick={() => void a.run(async () => { await req("POST", `/v1/friends/requests/${encodeURIComponent(r.id)}/decline`); refresh(); })}>{c("f.decline")}</button>
              </li>
            ))}
          </ul>
          {requests.outgoing?.length > 0 && <h3>{c("f.outgoing")}</h3>}
          <ul className="cm-list">
            {(requests.outgoing ?? []).map((r: J) => (
              <li key={r.id} className="cm-row">
                <Who user={r.user} />
                <span className="grow" />
                <button className="btn btn-small btn-ghost" disabled={a.busy} onClick={() => void a.run(async () => { await req("DELETE", `/v1/friends/requests/${encodeURIComponent(r.id)}`); refresh(); })}>{c("f.cancel")}</button>
              </li>
            ))}
          </ul>
        </article>
      )}
      <article className="card">
        <h2>{c("f.list")}</h2>
        {friends === null ? <span className="skeleton" style={{ width: "100%", height: "3rem" }} /> : friends.length === 0 ? <p className="muted">{c("f.none")}</p> : (
          <ul className="cm-list">
            {friends.map((f: J) => (
              <li key={f.user?.id} className="cm-row">
                <Who user={f.user}><Presence id={f.user?.id} initial={f.presence} /></Who>
                <span className="small muted grow">{f.since ? c("f.since", { date: fmtDay(f.since, lang) }) : ""}</span>
                <button className="btn btn-small btn-ghost" disabled={a.busy} onClick={() => { if (window.confirm(c("f.remove.confirm", { name: f.user?.display_name ?? "" }))) void a.run(async () => { await req("DELETE", `/v1/friends/${encodeURIComponent(f.user.id)}`); again(); }); }}>{c("f.remove")}</button>
                {has("moderation") && <button className="btn btn-small btn-ghost" disabled={a.busy} onClick={() => { if (window.confirm(c("f.block.confirm", { name: f.user?.display_name ?? "" }))) void a.run(async () => { await req("PUT", `/v1/blocks/${encodeURIComponent(f.user.id)}`); again(); setNb((x) => x + 1); }); }}>{c("f.block")}</button>}
              </li>
            ))}
          </ul>
        )}
      </article>
      {blocks.length > 0 && (
        <article className="card">
          <h2>{c("f.blocked")}</h2>
          <p className="small muted">{c("f.blocked.lead")}</p>
          <ul className="cm-list">
            {blocks.map((b: J) => (
              <li key={b.user?.id} className="cm-row">
                <Who user={b.user} />
                <span className="grow" />
                <button className="btn btn-small btn-ghost" disabled={a.busy} onClick={() => void a.run(async () => { await req("DELETE", `/v1/blocks/${encodeURIComponent(b.user.id)}`); setNb((x) => x + 1); })}>{c("f.unblock")}</button>
              </li>
            ))}
          </ul>
        </article>
      )}
    </div>
  );
}
