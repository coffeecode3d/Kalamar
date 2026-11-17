// The stands (protocol 1.1.7, `spectators.md`): places for spectators. In a room, the zone with its places, who sits
// there and the button to sit there or come back to play; in a match, who watches (every player sees it), the
// spectator's own line and the controls of the game's spectator view with the keys of the player's own settings; and
// the list of matches the server lets the player watch. Everything comes from the server's data: the places, who may
// take them, the reasons of a refusal.
import { useEffect, useState } from "react";
import { api, type Schema } from "../api";
import { useI18n } from "../i18n";
import { keyLabel } from "../pages/settings/fields";
import { toFail, type J } from "./api";
import { applyMatch, has, myId, req, setLive, useLive } from "./store";
import { failText, incompatibleText, useC, type CFn, type CKey } from "./texts";
import { ErrorLine, RankBadge, Who, useModeName, vs } from "./ui";
import { IconCheck } from "../icons";

/** This player watches the match from the stands. */
export const isSpectator = (m: J): boolean => m?.you?.team === "spectator" || m?.you?.seat === "spectator";

/** A `who` value in words: the launcher's own sentence for the protocol's values, else the server's label. */
export function whoText(c: CFn, L: (x: J) => string, who: string, label?: J, inRoom = false): string {
  const k = `st.who.${inRoom ? "" : "match."}${who}` as CKey;
  const plain = `st.who.${who}` as CKey;
  return c(k) !== k ? c(k) : c(plain) !== plain ? c(plain) : L(label) || who;
}

/** An action whose refusal names the members by their names (`incompatible_game` lists user ids). */
function useNamedAction(names: Record<string, string>) {
  const { c } = useC();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (f: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await f();
    } catch (e) {
      const fail = toFail(e);
      setError(incompatibleText(fail, c, myId(), names) ?? failText(fail, c, {}, myId()));
    } finally {
      setBusy(false);
    }
  };
  return { busy, error, run };
}

/** The room's stands: the places, who sits there, sit / come back to play, and the rules the server declares. */
export function RoomStands({ room, meta }: { room: J; meta: J }) {
  const { c, L } = useC();
  const me = myId();
  const decl = meta?.rooms?.spectators;
  const members: J[] = Array.isArray(room?.members) ? room.members : [];
  const seated = members.filter((m: J) => m.team === "spectator");
  const seats = Number(room?.spectators?.seats ?? room?.options?.spectators ?? 0);
  const names: Record<string, string> = {};
  for (const m of members) if (m?.user?.id) names[m.user.id] = String(m.user.display_name ?? "?").slice(0, 32);
  const a = useNamedAction(names);
  if (!decl || (seats === 0 && seated.length === 0)) return null;
  const mine = members.find((m: J) => m.user?.id === me);
  const open = room.state === "open";
  const apply = (r: J) => { if (r && r.id) setLive({ room: r }); };
  const cap: number[] = Array.isArray(room.options?.teams) ? room.options.teams : [4, 4];
  const count = (t: string) => members.filter((m: J) => m.team === t).length;
  // Back to play: the team with room for one more (alpha first).
  const back = count("alpha") < (cap[0] ?? 4) ? "alpha" : count("bravo") < (cap[1] ?? 4) ? "bravo" : null;
  const move = (team: string) => void a.run(async () => apply(await req("PUT", `/v1/rooms/${encodeURIComponent(room.id)}/members/${encodeURIComponent(me ?? "")}`, { team })));
  const who = String(room.spectators?.who ?? room.options?.spectators_who ?? "anyone");
  const whoLabel = (Array.isArray(meta?.rooms?.options) ? meta.rooms.options : []).find((f: J) => f?.key === "spectators_who")?.values?.find((v: J) => v?.id === who)?.label;
  return (
    <section className="card st-zone" aria-labelledby="st-zone-title">
      <header className="row">
        <h3 id="st-zone-title" className="grow">{c("st.title")} <span className="muted small">{seated.length}/{seats}</span></h3>
        {mine && open && mine.team !== "spectator" && seated.length < seats && (
          <button className="btn btn-small btn-alt" disabled={a.busy} onClick={() => move("spectator")}>{c("st.sit")}</button>
        )}
        {mine && open && mine.team === "spectator" && (
          <button className="btn btn-small" disabled={a.busy || !back} title={back ? undefined : c("err.team_full")} onClick={() => back && move(back)}>{c("st.back")}</button>
        )}
      </header>
      <ErrorLine text={a.error} />
      <ul className="cm-list st-seats">
        {seated.map((m: J) => (
          <li key={m.user?.id} className="cm-row">
            <Who user={m.user} me={m.user?.id === me} crown={m.user?.id === room.owner}>
              <RankBadge rating={m.rating} />
            </Who>
            <span className={`pill ${m.ready || m.user?.id === room.owner ? "pill-ok" : "pill-neutral"}`}>{m.ready || m.user?.id === room.owner ? c("room.ready") : c("room.notready")}</span>
          </li>
        ))}
        {Array.from({ length: Math.max(0, seats - seated.length) }, (_, i) => <li key={`e${i}`} className="cm-row cm-empty">{c("room.empty")}</li>)}
      </ul>
      {mine?.team === "spectator" && <p className="small ok-text"><IconCheck /> {c("st.you")}</p>}
      <ul className="small muted st-rules">
        <li>{c("st.who", { who: whoText(c, L, who, whoLabel, true) })}</li>
        <li>{decl.host ? c("st.host.yes") : c("st.host.no")}</li>
        <li>{c("st.notplayers")}</li>
        <li>{c("st.late")}</li>
        <li>{c("st.visible")}</li>
        {L(decl.summary) && <li>{L(decl.summary).slice(0, 300)}</li>}
      </ul>
    </section>
  );
}

/** Who watches a match: every spectator, shown to every player and spectator. */
export function MatchStands({ match }: { match: J }) {
  const { c } = useC();
  const me = myId();
  const list: J[] = Array.isArray(match?.spectators) ? match.spectators.slice(0, 4) : [];
  if (list.length === 0 && !match?.rules?.spectators) return null;
  const state = (s: J) => {
    const v = s?.status?.state;
    if (!v) return s?.launched ? c("m.launched") : "";
    const k = `m.game.${v}` as CKey;
    return c(k) === k ? v : c(k);
  };
  return (
    <div className="cm-team st-match">
      <h3>{c("st.list")} <span className="muted small">{list.filter((s) => !s.left).length}/{match?.rules?.spectators?.seats ?? list.length}</span></h3>
      {list.length === 0 ? (
        <p className="small muted">{c("st.none")}</p>
      ) : (
        <ul className="cm-list">
          {list.map((s: J) => (
            <li key={s.user?.id ?? s.member_name} className="cm-row">
              <Who user={{ display_name: s.member_name ?? s.user?.display_name }} me={s.user?.id === me} crown={s.role === "host"}>
                <RankBadge rating={s.rating} />
              </Who>
              <span className="small muted grow">{s.left ? c("st.left") : state(s)}</span>
              {s.referee && <span className="pill pill-neutral">{c("st.referee")}</span>}
              {s.reported && <span className="pill pill-ok">{c("m.reported")}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** The spectator's own line in a match: "you watch", the way out, nothing to accept nor to wear. */
export function SpectatorLine({ match }: { match: J }) {
  const { c } = useC();
  const [sure, setSure] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!isSpectator(match)) return null;
  const going = ["pending_accept", "drafting", "ready", "running"].includes(match.state);
  const leave = async () => {
    if (!sure) return setSure(true);
    setBusy(true);
    try {
      await req("DELETE", `/v1/matches/${encodeURIComponent(match.id)}/spectate`);
      setLive({ match: null });
    } catch (e) {
      setError(failText(toFail(e), c, {}, myId()));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="st-line" role="status">
      <p><strong>{c("st.watching")}</strong> {match.state === "pending_accept" ? c("st.watching.found") : ""}</p>
      <p className="small muted">{c("st.noloadout")}</p>
      {going && <button className={`btn btn-small ${sure ? "btn-danger" : "btn-ghost"}`} disabled={busy} onClick={() => void leave()}>{sure ? c("st.leave.confirm") : c("st.leave")}</button>}
      <ErrorLine text={error} />
    </div>
  );
}

/** The spectator's verdict, once the match is decided (its report is a witness's, never a player's). */
export function WitnessVerdict({ match }: { match: J }) {
  const { c, L } = useC();
  const me = myId();
  const o = match?.outcome;
  if (!o) return null;
  const mine = (Array.isArray(o.spectators) ? o.spectators : []).find((s: J) => s?.user_id === me);
  return (
    <>
      {o.witnessed === true && o.decision === "witnessed" && !L(o.summary) && <p className="small">{c("st.witnessed")}</p>}
      {mine?.verdict && <p className="small">{c("st.verdict", { v: c(`st.verdict.${mine.verdict}` as CKey) === `st.verdict.${mine.verdict}` ? String(mine.verdict) : c(`st.verdict.${mine.verdict}` as CKey) })}</p>}
    </>
  );
}

/** The bound keys of a button: the player's own binding (`bind_<id>`), else the default, in words. */
function keysOf(schema: Schema | null, values: Record<string, string>, id: string, t: ReturnType<typeof useI18n>["t"]): string[] {
  const b = schema?.bindings.buttons.find((x) => x.id === id);
  const v = values[`bind_${id.toLowerCase()}`] ?? b?.default ?? "";
  return v.split(",").map((s) => s.trim()).filter((s) => s && s.toLowerCase() !== "none" && !s.toLowerCase().startsWith("pad:")).map((s) => keyLabel(s, t)).slice(0, 3);
}

/** The controls of the game's spectator view (the engine's documentation), with the keys the player's own settings
 *  bind: hold ZL to show each player's button, hold a player's button to follow them, hold L for the top view, hold R
 *  for a player's view again, X for the map. */
export function SpectatorControls({ compact }: { compact?: boolean }) {
  const { c } = useC();
  const { t } = useI18n();
  const [schema, setSchema] = useState<Schema | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  useEffect(() => {
    void Promise.all([api.settingsSchema(), api.settingsLoad()]).then(([s, v]) => { setSchema(s); setValues(v.values ?? {}); }).catch(() => {});
  }, []);
  const k = (id: string) => {
    const keys = keysOf(schema, values, id, t);
    return (
      <span className="st-key"><kbd className="kbd-pad">{id === "UP" ? "✛" : id}</kbd>{keys.length > 0 && <> {keys.map((x) => <kbd key={x}>{x}</kbd>)}</>}</span>
    );
  };
  const pad = keysOf(schema, values, "UP", t).concat(keysOf(schema, values, "LEFT", t), keysOf(schema, values, "RIGHT", t), keysOf(schema, values, "DOWN", t));
  return (
    <div className={`st-ctl ${compact ? "st-ctl-compact" : ""}`}>
      <p className="eyebrow">{c("st.ctl.title")}</p>
      <ul>
        <li>{k("ZL")} {c("st.ctl.hold")}</li>
        <li><span className="st-key"><kbd className="kbd-pad">✛</kbd>{pad.slice(0, 4).map((x) => <kbd key={x}>{x}</kbd>)}</span> {k("A")} {c("st.ctl.follow")}</li>
        <li>{k("L")} {c("st.ctl.top")}</li>
        <li>{k("R")} {c("st.ctl.back")}</li>
        <li>{k("X")} {c("st.ctl.map")}</li>
      </ul>
      <p className="small muted">{c("st.ctl.auto")}</p>
    </div>
  );
}

/** The matches and series the server lets the player watch (`GET /v1/spectate`), before their launch. */
export function Watch() {
  const { c, L } = useC();
  const live = useLive();
  const modeName = useModeName();
  const [items, setItems] = useState<J[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  // The queues' names (the list names a queue by its id).
  const [queues, setQueues] = useState<Record<string, J>>({});
  const [titles, setTitles] = useState<Record<string, J>>({});
  useEffect(() => {
    if (has("activities")) void req("GET", "/v1/activities").then((v: J) => setTitles(Object.fromEntries((Array.isArray(v?.items) ? v.items : []).map((a: J) => [String(a.id), a.title])))).catch(() => {});
    if (has("queues")) void req("GET", "/v1/queues").then((v: J) => setQueues(Object.fromEntries((Array.isArray(v?.items) ? v.items : []).map((q: J) => [String(q.id), q.name])))).catch(() => {});
  }, [live.server]); // eslint-disable-line react-hooks/exhaustive-deps
  const load = () => void req("GET", "/v1/spectate").then((v: J) => { setItems(Array.isArray(v?.items) ? v.items.slice(0, 100) : []); setError(null); }).catch((e) => setError(failText(toFail(e), c)));
  useEffect(() => {
    load();
    const id = window.setInterval(load, 10_000);
    return () => window.clearInterval(id);
  }, [live.server]); // eslint-disable-line react-hooks/exhaustive-deps
  const take = (it: J) => {
    const key = String(it.series_id ?? it.match_id);
    setBusy(key);
    const path = it.kind === "series" ? `/v1/series/${encodeURIComponent(it.series_id)}/spectate` : `/v1/matches/${encodeURIComponent(it.match_id)}/spectate`;
    void req("POST", path, {})
      .then(async (v: J) => {
        // A match: its view (the spectator's); a series: its current game, when the place holds there already.
        if (it.kind === "series") {
          const cur: J = await req("GET", "/v1/matches/current").catch(() => null);
          if (cur?.match) applyMatch(cur.match);
        } else applyMatch(v);
        load();
      })
      .catch((e) => setError(failText(toFail(e), c, {}, myId())))
      .finally(() => setBusy(null));
  };
  const names = (list: J) => (Array.isArray(list) ? list.map((u: J) => String(u?.display_name ?? "?").slice(0, 24)).join(", ") : "");
  const origin = (it: J) => {
    if (it.origin?.kind === "queue") return L(queues[it.origin.queue_id]) || String(it.origin.queue_id ?? "");
    if (it.origin?.kind === "activity") return L(titles[it.origin.activity_id]) || L(live.activityTitles[it.origin.activity_id]) || String(it.origin.activity_id ?? "");
    return "";
  };
  return (
    <div className="stack">
      <p className="hint">{c("w.hint")}</p>
      {L(live.overview?.meta?.spectators?.summary) && <p className="hint">{L(live.overview?.meta?.spectators?.summary)}</p>}
      <ErrorLine text={error} />
      {items === null ? (
        <div className="state" aria-busy="true"><span className="skeleton" style={{ width: "12rem", height: "1.2rem" }} /></div>
      ) : items.length === 0 ? (
        <div className="state"><p>{c("w.empty")}</p></div>
      ) : (
        <div className="cm-grid">
          {items.map((it: J) => {
            const key = String(it.series_id ?? it.match_id);
            const st = `w.state.${it.state}` as CKey;
            const why = it.why === "spectators_full" ? c("w.full") : it.why === "incompatible_game" ? c("w.incompatible") : null;
            return (
              <article key={key} className="card w-item">
                <header className="row">
                  <h3 className="grow">{c("w.vs", { a: names(it.teams?.alpha), b: names(it.teams?.bravo) })}</h3>
                  <span className="pill pill-neutral">{c(st) === st ? String(it.state ?? "") : c(st)}</span>
                </header>
                <p className="small muted">{[origin(it), vs(c, it.rules?.teams), modeName(it.rules?.mode, live.overview?.meta)].filter(Boolean).join(" · ")}</p>
                {it.series && <p className="small">{c("w.series", { n: it.series.index ?? "?", a: it.series.wins?.[0] ?? 0, b: it.series.wins?.[1] ?? 0 })}</p>}
                <p className="small">{c("w.places", { taken: it.places?.taken ?? 0, seats: it.places?.seats ?? 0, who: whoText(c, L, String(it.places?.who ?? "anyone"), it.places?.label) })}</p>
                {it.draft && <p className="small muted">{c("w.draft")}</p>}
                {why && <p className="small muted">{why}</p>}
                <button className="btn btn-small" disabled={!it.can_take || busy === key || (!!live.match && live.match.you?.free !== true)} onClick={() => take(it)}>{it.kind === "series" ? c("w.take.series") : c("w.take")}</button>
              </article>
            );
          })}
        </div>
      )}
      {has("spectators") && live.match && live.match.you?.free !== true && <p className="hint">{c("err.in_match")}</p>}
    </div>
  );
}
