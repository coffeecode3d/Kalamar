// The stands of the browser preview (protocol 1.1.7, `spectators.md`): `&spect=` chooses the situation.
//   &room=host&spect=room     a room with two places in its stands, Noor seated there
//   &room=guest&spect=sit     the player sits in the stands of Sacha's room
//   &room=host&spect=old      an older launcher (Pablo's) plays: sitting in the stands is refused with what it lacks
//   &match=found&spect=watch  the player took a place during the accept phase of a queue match
//   &match=run&spect=watch    the player watches a running match (the runner's seat `spectator`; &gs=<game state>)
//   &match=draft&ds=them&spect=watch  the stage draft seen from the stands, read-only
//   &match=done&spect=watch   the decided match as the spectator sees it (its witness report broke a disagreement)
//   &match=run&spect=players  a player's view of a watched match (Noor in the stands)
//   &match=stop&spect=late    the spectator's game was not started: the match had started
//   &section=watch            the list of watchable matches (with &spect=none: empty)
// Names are made up; stage ids are the made-up ones of catalogue.ts.
/* eslint-disable @typescript-eslint/no-explicit-any */

import { RANKS } from "./ranks";

const L = (fr: string, en: string, es: string, ja: string) => ({ fr, en, es, ja });
const user = (id: string, name: string) => ({ id, display_name: name });
export const ME = user("u-me", "Mira");
const PABLO = user("u-2", "Pablo");
const SACHA = user("u-3", "Sacha");
const NOOR = user("u-4", "Noor");

/** The room form's fields and the declaration's stands (always declared by the made-up server). */
export const ROOM_FIELDS = [
  { key: "spectators", type: "integer", label: L("Places pour spectateurs", "Places for spectators", "Plazas para espectadores", "観戦席の数"), help: L("Des membres regardent le match depuis la tribune au lieu de jouer (0 pour fermer la tribune).", "Members who watch the match from the stands instead of playing (0 to close them).", "Miembros que miran la partida desde la tribuna en lugar de jugar (0 para cerrarla).", "プレイせずに観覧席から観戦するメンバー（0で観覧席なし）。"), required: false, min: 0, max: 2, default: 0 },
  { key: "spectators_who", type: "enum", label: L("Qui peut regarder", "Who may watch", "Quién puede mirar", "観戦できる人"), required: false, default: "anyone", values: [
    { id: "anyone", label: L("Tout le monde", "Anyone", "Cualquiera", "誰でも") },
    { id: "friends", label: L("Les amis", "Friends", "Los amigos", "フレンド") },
    { id: "invited", label: L("Les invités", "Invited players", "Los invitados", "招待された人") },
  ] },
];
export const ROOM_STANDS = { max: 2, who: ["anyone", "friends", "invited"], host: true, wait_s: 60, late: false, opt_out: true, summary: {} };
export const META_STANDS = { version: 1, summary: L("Les spectateurs regardent depuis la tribune : les joueurs voient qui regarde.", "Spectators watch from the stands: the players see who watches.", "Los espectadores miran desde la tribuna: los jugadores ven quién mira.", "観戦者は観覧席から観戦し、プレイヤーには誰が観戦しているか見えます。"), opt_out: true, referees: true, late: false, wait_s: 60 };

/** The room of `&room=` with its stands, per `&spect=`. */
export function standsRoom(base: any, spect: string | null, moved: boolean): any {
  if (!base || !spect || !["room", "sit", "old"].includes(spect)) return base;
  const members: any[] = base.members.map((m: any) => ({ ...m }));
  if (spect === "room") members.push({ user: NOOR, team: "spectator", ready: true });
  if (spect === "sit" || moved) {
    const me = members.find((m) => m.user.id === "u-me");
    if (me) { me.team = "spectator"; me.ready = true; }
  }
  const taken = members.filter((m) => m.team === "spectator").length;
  return { ...base, options: { ...base.options, spectators: 2, spectators_who: spect === "sit" ? "friends" : "anyone" }, members, spectators: { seats: 2, taken, who: spect === "sit" ? "friends" : "anyone", host: true } };
}

/** The refusal a room gives when an older launcher plays (`&spect=old`). */
export const OLD_REFUSAL = {
  kind: "api", code: "incompatible_game", status: 409, message: "a game in this room cannot play with spectators", details: [], fields: [], retry_after_s: null,
  data: { players: [{ user_id: "u-2", missing: ["launcher:spectate", "spectators"] }] },
};

function watchers(state: string | null, mine: boolean): any[] {
  const s = (u: any) => ({ user: u, member_name: u.display_name, role: "join", referee: false, launched: !!state, left: false, status: state ? { state } : null, reported: state === "results" || state === "finished" });
  return [mine ? s(ME) : s(NOOR)];
}
const RULES = (stage: string | null) => ({ teams: [1, 1], mode: "turf", rated: false, stage, options: {}, spectators: { seats: 2, who: "friends", wait_s: 60, open: false }, result_policy: "agree" });
// In the stands' matches Pablo is #1 and Sacha is placed but not listed; the player is #12.
const STANDS_RANK: Record<string, any> = { "u-me": RANKS.twelve, "u-2": RANKS.first, "u-3": RANKS.unlisted };
const player = (u: any, team: string, role: string, state: string | null, accept = "accepted") => ({ user: u, member_name: u.display_name, team, role, accept, launched: !!state, status: state ? { state } : null, reported: state === "results" || state === "finished", rating: STANDS_RANK[u.id] });
const YOU_WATCH = { user_id: "u-me", member_name: "Mira", team: "spectator", seat: "spectator", role: "join", referee: false, left: false };

/** The match of `&match=` when the player watches it (or, `players`, plays a watched one). */
export function standsMatch(match: string | null, spect: string | null, gs: string): any {
  if (!match || !spect) return null;
  if (spect === "players" && match === "run") {
    return { id: "m-1", state: "running", origin: { kind: "room", room_id: "r-1" }, rules: RULES("Vss_LanternHarbor"), participants: [player(ME, "alpha", "host", gs), player(PABLO, "bravo", "join", gs === "creating-room" ? "joining" : gs)], spectators: watchers(gs === "waiting-for-players" ? "joining" : gs, false), you: { user_id: "u-me", member_name: "Mira", team: "alpha", seat: "player", role: "host" }, deadlines: {} };
  }
  if (spect !== "watch" && spect !== "late") return null;
  const parts = (st: string | null, acc = "accepted") => [player(PABLO, "alpha", "host", st, acc), player(SACHA, "bravo", "join", st, acc)];
  if (match === "found") return { id: "m-1", state: "pending_accept", origin: { kind: "queue", queue_id: "q-1v1" }, rules: { ...RULES(null), spectators: { seats: 2, who: "friends", wait_s: 60, open: true } }, participants: [player(PABLO, "alpha", "host", null, "accepted"), player(SACHA, "bravo", "join", null, "pending")], spectators: watchers(null, true), you: YOU_WATCH, deadlines: { accept: new Date(Date.now() + 14000).toISOString() } };
  if (match === "run" || match === "stop") return { id: "m-1", state: "running", origin: { kind: "queue", queue_id: "q-1v1" }, rules: RULES("Vss_LanternHarbor"), participants: parts(match === "stop" ? "in-match" : gs), spectators: watchers(match === "stop" ? null : gs, true), you: YOU_WATCH, deadlines: {} };
  if (match === "done") {
    return {
      id: "m-1", state: "completed", origin: { kind: "queue", queue_id: "q-1v1" }, rules: RULES("Vss_LanternHarbor"), participants: parts("finished"), spectators: watchers("finished", true), you: YOU_WATCH, deadlines: {},
      outcome: {
        decision: "witnessed", counted: true, winner: "bravo", rated: false, witnessed: true,
        summary: L("Les rapports des joueurs divergeaient ; celui du spectateur a tranché.", "The players' reports disagreed; the spectator's report decided between them.", "Los informes de los jugadores no coincidían; el del espectador decidió.", "プレイヤーの報告が食い違ったため、観戦者の報告で決まりました。"),
        record: { stage_scene: "Vss_LanternHarbor", teams: [{ team: "alpha", paint_points: 812, turf_permille: 401 }, { team: "bravo", paint_points: 1031, turf_permille: 538 }], players: [{ name: "Pablo", paint: 812, kills: 4, assists: 1, deaths: 5 }, { name: "Sacha", paint: 1031, kills: 5, assists: 2, deaths: 4 }] },
        players: [{ user_id: "u-2", verdict: "dissent" }, { user_id: "u-3", verdict: "agreed" }],
        spectators: [{ user_id: "u-me", referee: false, counted: true, verdict: "agreed" }], reports: { expected: 2, results: 2, agreeing: 1, errors: 0, invalid: 0, witnesses: 1 },
      },
    };
  }
  return null;
}

/** The draft of `&match=draft` seen from the stands: the player is not in a team; read-only. */
export function standsDraft(m: any): any {
  if (!m) return m;
  const swap = (id: any) => (id === "u-me" ? "u-3" : id);
  const d = m.draft ?? {};
  const teams: any = {};
  for (const [k, t] of Object.entries<any>(d.teams ?? {})) teams[k] = { ...t, players: (t.players ?? []).map(swap), actors: (t.actors ?? []).map(swap), captain: swap(t.captain), votes: undefined };
  return {
    ...m,
    participants: (m.participants ?? []).map((p: any) => (p.user?.id === "u-me" ? { ...p, user: SACHA, member_name: "Sacha" } : p)),
    spectators: watchers(null, true), you: YOU_WATCH,
    draft: { ...d, teams, you: { team: "spectator", role: "none", can_act: false, choice: null } },
  };
}

/** The runner of a spectator's game (`&match=run|stop&spect=watch|late`). */
export function standsRunner(base: any, match: string | null, spect: string | null): any {
  if (!base || (spect !== "watch" && spect !== "late")) return base;
  if (spect === "late" || match === "stop") {
    return { ...base, phase: "late", game_state: null, code: "launcher-spectator-late", reason: "the match had started: the game cannot let a spectator in any more", role: "join", seat: "spectator", game: null, env: [] };
  }
  return { ...base, role: "join", seat: "spectator", member_name: "Mira", game: { ...base.game, present: null, expected: null }, env: [["SPLAT3_MATCH", "join"], ["SPLAT3_MATCH_ID", "m-1"], ["SPLAT3_MATCH_RULES", "players=3,teams=Pablo:A,Sacha:B,Mira:S,mode=turf,stage=Vss_LanternHarbor,spectators_wait=60"]] };
}

/** `GET /v1/spectate`: two watchable matches and a full series (none with `&spect=none`). */
export function watchable(spect: string | null): any {
  if (spect === "none") return { items: [] };
  const later = (s: number) => new Date(Date.now() + s * 1000).toISOString();
  void later;
  return {
    items: [
      { kind: "match", match_id: "m-7", series_id: null, origin: { kind: "queue", queue_id: "q-1v1" }, state: "drafting", rules: { mode: "turf", stage: null, teams: [1, 1] }, teams: { alpha: [PABLO], bravo: [SACHA] }, places: { seats: 2, taken: 1, who: "friends", label: L("Amis des joueurs", "Players' friends", "Amigos de los jugadores", "プレイヤーのフレンド") }, series: null, draft: true, can_take: true, why: null, missing: [] },
      { kind: "series", match_id: "m-8", series_id: "sr-2", origin: { kind: "activity", activity_id: "cup" }, state: "running", rules: { mode: "zones", stage: null, teams: [2, 2] }, teams: { alpha: [NOOR, user("u-5", "Ilan")], bravo: [user("u-6", "Yuki"), user("u-7", "Romy")] }, places: { seats: 2, taken: 0, who: "anyone" }, series: { index: 2, format: { kind: "best_of", games: 3 }, wins: [1, 0], labels: ["Noor", "Yuki"] }, draft: false, can_take: true, why: null, missing: [] },
      { kind: "match", match_id: "m-9", series_id: null, origin: { kind: "queue", queue_id: "q-4v4" }, state: "pending_accept", rules: { mode: "turf", stage: null, teams: [4, 4] }, teams: { alpha: [user("u-8", "Lou"), user("u-9", "Ana"), user("u-10", "Kim"), user("u-11", "Max")], bravo: [user("u-12", "Eli"), user("u-13", "Zoé"), user("u-14", "Ugo"), user("u-15", "Ines")] }, places: { seats: 1, taken: 1, who: "anyone" }, series: null, draft: false, can_take: false, why: "spectators_full", missing: [] },
    ],
  };
}
