// The stage draft of the browser preview (protocol 1.1.6): a made-up match whose draft is in the situation the URL asks
// for (`&match=draft&ds=you|them|vote|timeout|random|done|mode|both`), and the answers to the player's actions (the next
// situation). Stage ids are the made-up ones of catalogue.ts; nothing here comes from a game.
/* eslint-disable @typescript-eslint/no-explicit-any */
import { STAGE_IDS } from "./catalogue";
import { RANK_OF } from "./ranks";

const L = (fr: string, en: string, es: string, ja: string) => ({ fr, en, es, ja });
const iso = (ms: number) => new Date(ms).toISOString();
const POOL = STAGE_IDS.slice(0, 7);
// The last of the made-up stages has no picture: here, a stage a player's game cannot load.
const MISSING = STAGE_IDS[STAGE_IDS.length - 1];

const ME = { id: "u-me", display_name: "Mira" };
const PABLO = { id: "u-2", display_name: "Pablo" };
const SACHA = { id: "u-3", display_name: "Sacha" };
const NOOR = { id: "u-4", display_name: "Noor" };

function people(four: boolean): any[] {
  const p = (u: any, team: string, role: string) => ({ user: u, member_name: u.display_name, team, role, accept: "accepted", launched: false, status: null, reported: false, rating: RANK_OF[u.id] });
  return four ? [p(ME, "alpha", "host"), p(SACHA, "alpha", "join"), p(PABLO, "bravo", "join"), p(NOOR, "bravo", "join")] : [p(ME, "alpha", "host"), p(PABLO, "bravo", "join")];
}

function turn(index: number, action: string, teams: string[], state: string, extra: any = {}): any {
  return { index, action, kind: "stage", teams, count: 1, state, started_at: null, deadline: null, ended_at: null, choices: [], acted: [], ...extra };
}

/** A turn running now, `left` seconds before its deadline (of `total`). */
function live(t: any, left: number, total = 20): any {
  const now = Date.now();
  return { ...t, state: "current", started_at: iso(now - (total - left) * 1000), deadline: iso(now + left * 1000) };
}

function options(banned: Record<string, string>, picked: Record<string, string | null> = {}): any[] {
  return [
    ...POOL.map((id, i) => ({
      id, kind: "stage",
      state: id in picked ? "picked" : id in banned ? "banned" : "available",
      by: id in picked ? picked[id] : id in banned ? banned[id] : null,
      turn: id in banned || id in picked ? i % 3 : null, order: id in picked ? 1 : null, reason: null,
    })),
    { id: MISSING, kind: "stage", state: "unavailable", by: null, turn: null, order: null, reason: "game" },
  ];
}

const VETO = {
  preset: "veto", title: L("Veto des stages", "Stage veto", "Veto de escenarios", "ステージ拒否"),
  summary: L("Chaque équipe bannit un stage, puis un stage est tiré au sort parmi les autres.", "Each team bans one stage, then a stage is drawn among the rest.", "Cada equipo veta un escenario y luego se sortea uno entre los demás.", "各チームが1つずつステージをバンし、残りから抽選します。"),
};

function teams(four: boolean, decider: string, votes: any[] | null = null, autoBravo = false): any {
  return {
    alpha: { players: four ? ["u-me", "u-3"] : ["u-me"], actors: decider === "vote" && four ? ["u-me", "u-3"] : ["u-me"], captain: "u-me", auto: false, ...(votes ? { votes } : {}) },
    bravo: { players: four ? ["u-2", "u-4"] : ["u-2"], actors: autoBravo ? [] : ["u-2"], captain: autoBravo ? null : "u-2", auto: autoBravo },
  };
}

/** The draft in situation `ds`. */
export function mockDraft(ds: string): any {
  const base = { version: 1, decider: "captain", tiebreak: "captain", turn_s: 20, first: "alpha", result: { mode: null, stage: null, stages: [] }, ...VETO };
  const [a, b, c] = [POOL[1], POOL[4], POOL[2]];
  switch (ds) {
    case "them":
      return { ...base, state: "running", current: 1, options: options({ [a]: "alpha" }), teams: teams(false, "captain"),
        turns: [turn(0, "ban", ["alpha"], "done", { choices: [{ team: "alpha", ids: [a], by: "players" }] }), live(turn(1, "ban", ["bravo"], "current"), 13), turn(2, "random", [], "pending")],
        you: { team: "alpha", role: "captain", can_act: false, choice: null } };
    case "timeout":
      return { ...base, state: "running", current: 1, options: options({ [POOL[5]]: "alpha" }), teams: teams(false, "captain"),
        turns: [turn(0, "ban", ["alpha"], "done", { choices: [{ team: "alpha", ids: [POOL[5]], by: "timeout" }] }), live(turn(1, "ban", ["bravo"], "current"), 3), turn(2, "random", [], "pending")],
        you: { team: "alpha", role: "captain", can_act: false, choice: null } };
    case "random":
      return { ...base, state: "running", current: 2, options: options({ [a]: "alpha", [b]: "bravo" }), teams: teams(false, "captain"),
        turns: [turn(0, "ban", ["alpha"], "done", { choices: [{ team: "alpha", ids: [a], by: "players" }] }), turn(1, "ban", ["bravo"], "done", { choices: [{ team: "bravo", ids: [b], by: "players" }] }), { ...turn(2, "random", [], "current"), started_at: iso(Date.now()) }],
        you: { team: "alpha", role: "captain", can_act: false, choice: null } };
    case "done":
      return { ...base, state: "done", current: null, options: options({ [a]: "alpha", [b]: "bravo" }, { [c]: null }), teams: teams(false, "captain"),
        turns: [turn(0, "ban", ["alpha"], "done", { choices: [{ team: "alpha", ids: [a], by: "players" }] }), turn(1, "ban", ["bravo"], "done", { choices: [{ team: "bravo", ids: [b], by: "timeout" }] }), turn(2, "random", [], "done", { choices: [{ team: null, ids: [c], by: "server" }] })],
        you: { team: "alpha", role: "captain", can_act: false, choice: null }, result: { mode: null, stage: c, stages: [c] } };
    case "vote":
      return { ...base, decider: "vote", state: "running", current: 0, title: L("Bans à l'aveugle", "Blind bans", "Vetos a ciegas", "ブラインドバン"),
        summary: L("Les deux équipes votent un ban en même temps, sans se voir ; puis un stage au hasard parmi le reste.", "Both teams vote one ban at once, unseen; then a random stage among the rest.", "Los dos equipos votan un veto a la vez, sin verse; luego un escenario al azar entre el resto.", "両チームが同時に1つバンを投票（互いに非公開）。その後、残りから抽選。"),
        options: options({}), teams: teams(true, "vote", [{ user_id: "u-3", ids: [POOL[3]] }]),
        turns: [live({ ...turn(0, "ban", ["alpha", "bravo"], "current"), acted: ["bravo"] }, 17, 30), turn(1, "random", [], "pending")],
        you: { team: "alpha", role: "voter", can_act: true, choice: null } };
    case "mode":
      return { ...base, state: "running", current: 0, title: L("Le mode, puis le stage", "Mode, then stage", "El modo y luego el escenario", "ルール、次にステージ"), summary: null,
        options: [...["turf", "zones", "tower"].map((id) => ({ id, kind: "mode", state: "available", by: null, turn: null, order: null, reason: null })), ...options({})],
        teams: teams(false, "captain"),
        turns: [live({ ...turn(0, "ban", ["alpha"], "current"), kind: "mode" }, 16), { ...turn(1, "ban", ["bravo"], "pending"), kind: "mode" }, { ...turn(2, "random", [], "pending"), kind: "mode" }, turn(3, "ban", ["bravo"], "pending"), turn(4, "pick", ["alpha"], "pending")],
        you: { team: "alpha", role: "captain", can_act: true, choice: null } };
    case "auto":
      return { ...base, state: "running", current: 0, options: options({}), teams: teams(false, "captain", null, true),
        turns: [live(turn(0, "ban", ["alpha"], "current"), 18), turn(1, "ban", ["bravo"], "pending"), turn(2, "random", [], "pending")],
        you: { team: "alpha", role: "captain", can_act: true, choice: null } };
    case "waiting":
      return { ...base, state: "waiting", current: null, options: options({}), teams: teams(false, "captain"),
        turns: [turn(0, "ban", ["alpha"], "pending"), turn(1, "ban", ["bravo"], "pending"), turn(2, "random", [], "pending")],
        you: { team: "alpha", role: "captain", can_act: false, choice: null } };
    default: // "you"
      return { ...base, state: "running", current: 0, options: options({}), teams: teams(false, "captain"),
        turns: [live(turn(0, "ban", ["alpha"], "current"), 17), turn(1, "ban", ["bravo"], "pending"), turn(2, "random", [], "pending")],
        you: { team: "alpha", role: "captain", can_act: true, choice: null } };
  }
}

/** The made-up match of a situation. */
export function mockDraftMatch(ds: string): any {
  const draft = mockDraft(ds);
  const four = ds === "vote";
  const done = draft.state === "done";
  return {
    id: "m-1", state: done ? "ready" : "drafting", origin: { kind: "queue", queue_id: "q-1v1" },
    rules: { teams: four ? [2, 2] : [1, 1], mode: "turf", rated: true, stage: done ? draft.result.stage : null, options: {} },
    participants: people(four), you: { user_id: "u-me", member_name: "Mira", team: "alpha", role: "host" },
    game: done ? { contract: { id: "lan-auto-match", version: 2 }, match_id: "m-1", rules: `players=2,teams=Mira:A,Pablo:B,mode=turf,stage=${draft.result.stage}` } : null,
    deadlines: {}, draft,
  };
}

/** The answer to the player's action: the next situation (a ban of the player in `you` leads to `them`, a vote to the
 *  vote with the player's choice, and so on). */
export function mockDraftAct(ds: string, body: any): { ds: string; match: any } {
  const choice = Array.isArray(body?.choices) ? body.choices.map(String) : [];
  if (ds === "vote") {
    const m = mockDraftMatch("vote");
    m.draft.you.choice = choice;
    m.draft.teams.alpha.votes = [...m.draft.teams.alpha.votes, { user_id: "u-me", ids: choice }];
    return { ds: "vote", match: m };
  }
  const next = ds === "mode" ? "you" : ds === "auto" ? "done" : "them";
  const m = mockDraftMatch(next);
  if (next === "them" && choice[0]) {
    m.draft.options = m.draft.options.map((o: any) => (o.id === choice[0] ? { ...o, state: "banned", by: "alpha", turn: 0 } : o.state === "banned" ? { ...o, state: "available", by: null, turn: null } : o));
    m.draft.turns[0].choices = [{ team: "alpha", ids: choice, by: "players" }];
  }
  return { ds: next, match: m };
}
