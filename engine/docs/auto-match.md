# Automatic LAN match (Splatoon 3)

For a launcher that is the game's menu (accounts, parties, queues, pre-match lobby, results) and starts the game only
for one match: the game enters a given LAN private battle by itself, as host or as guest, and writes the match's
result to a file when it ends. Runtime: `runtime/auto_match.c`. It builds on LAN play ([lan-play.md](lan-play.md)) and
the start scene `lobby-lan` ([save-profile.md](save-profile.md)).

With `KLM_MATCH` the player sees nothing of the game until the match's intro: the window opens hidden, the sound is
muted and the player's own input is ignored while the runtime drives the lobby, and the window comes up at the stage's
intro ([Out of sight](#out-of-sight)). The host sets the room's mode, stage and other settings exactly
([Room settings](#room-settings)); a launcher can play further rounds in the same room without restarting the game
([Rounds](#rounds)); a room can have spectators ([Spectators](#spectators)). What was run, and what was not:
[What was verified](#what-was-verified).

## Settings

Environment variables, per launch. `KLM_MATCH` needs LAN play (`KLM_LAN`, lan or relay mode) and
`KLM_START=lobby-lan`; without them, or with any other setting below that cannot work, the flow stops before the
window opens with the error `bad-settings` (status and result files written, exit code 3).

| Variable | Values |
|---|---|
| `KLM_MATCH` | `host`: create the session, wait for the players, set the teams, start; `join`: join the session a host made. Unset or `off`: nothing (the game starts as usual) |
| `KLM_MATCH_ID` | the launcher's match id, 1-64 of `A-Z a-z 0-9 - _`, copied into the status and the result. In relay mode give `KLM_LAN_ROOM` the same value (at most 32 characters; the relay upper-cases it): the relay room is what keeps the players of one match apart, and a room that differs from the id is logged |
| `KLM_MATCH_RULES` | comma-separated keys, [Rule keys](#rule-keys); the room's keys act on the host only. Example: `players=2,teams=Alpha:B,Bravo:A,mode=zones,stage=11,results_hold=15` |
| `KLM_MATCH_LOADOUT` | path of a `profile.json` ([save-profile.md](save-profile.md)), usually only `weapon` and `gear`: applied to the match's copy of the save, never to the player's ([The match's save](#the-matchs-save)) |
| `KLM_MATCH_STAGES` | `all` (default): the room offers every stage the game's stage lists show, whatever the save's progress; `save`: the save's own list ([Every stage](#every-stage)) |
| `KLM_MATCH_CMD` | path of a command file the launcher appends lines to: `next [id=<match id>] [<rule keys>]` starts another round in the same room after this one, `quit` closes the game ([Rounds](#rounds)). Without it the game closes (`KLM_MATCH_EXIT=1`) or stays open after the match |
| `KLM_MATCH_HOST` | guest only: list only the sessions whose host has this nickname; the other hosts' answers to the search are dropped before the game sees them |
| `KLM_MATCH_REVEAL` | when the hidden window is shown: `intro` (default: the stage's intro, below), `loading` (when the stage starts loading: its black loading screen is seen), `never` (stays hidden to the end; tests) |
| `KLM_MATCH_ON_ERROR` | `exit` (default): on an error the window stays hidden (or is hidden again), the error goes to the status and result files and the game closes with exit code 3; `show`: the window is shown on whatever the game shows and the game stays open |
| `KLM_MATCH_STATUS` | path of the status JSON (below), rewritten at every change of state; `{id}` in the path is replaced by the round's match id |
| `KLM_MATCH_RESULT` | path of the result JSON ([The result](#the-result)), written only when the game ended a match: no result file means no match was played to its end; `<path>.bin` holds the raw record; `{id}` as for the status |
| `KLM_MATCH_EXIT` | `1`: after the result screens (below) hide the window, mute, and close the game as the window's close button does (save flushed, exit code 0) |
| `KLM_MATCH_SILENT` | 5-600: in a match, the seconds of session silence after which the flow stops with `connection-lost` ([When a member disappears](#when-a-member-disappears)). Default: 15 in a match of two players (spectators not counted: the rules' `players` less their `S` rows), 45 with more |
| `KLM_MATCH_TIMEOUTS` | each step's time limit in seconds, comma-separated `<step>=<s>`; the steps and their defaults: `boot=300` (process start to the local lobby's network), `list=90` (the session list open and searched; between rounds: the host's room screen back), `create=60` (host: a session made), `players=300` (host: every player in the room), `join=300` (guest: a session found and joined), `start=360` (in the room until the stage loads; a guest's wait includes the other players'), `stage=90` (stage loading to its intro), `match=900` (intro to the match's record), `results=120` (the result screens at most: then the game goes on as if they were over) |
| `KLM_MATCH_WAIT` | older form of `players=` and `join=` together |
| `KLM_MATCH_SHOTS` | a folder: a screenshot at each step, `auto-<host\|join>-<nn>-<step>.png`, and of the first frame shown, `reveal-<host\|join>-<match id>.png` |
| `KLM_MATCH_PLAY` | test aid: seconds the player walks forward with ZR held once in the stage (default 0: stands still) |
| `KLM_MATCH_DIAG` | test aid: `1` logs every RomFS file the game opens in the 30 s after the stage pack |

The players' names are their nicknames (`KLM_NICKNAME`, else the profile's name). In relay mode set
`KLM_LAN_NAME` to the nickname too: it is the name the relay gives the other members (teams by name).

### Rule keys

`KLM_MATCH_RULES` and a `next` command's rules: comma-separated `<key>=<value>`. A key or value that cannot work
stops the flow before the window opens (`bad-settings`); in a `next` command it is refused (logged, the status says
why) and the game waits for another command.

| Key | Values | Who |
|---|---|---|
| `players` | 2-10: the host starts once that many are in the session (default 2), the spectators included: 8 players and 2 spectators at most | host |
| `teams` | `<A\|B\|S\|->` per player in arrival order (`AB`: the host in Alpha, the first guest in Bravo), or by name `teams=<name>:<A\|B\|S\|->,<name>:<A\|B\|S\|->,...` (relay mode); `S` a spectator ([Spectators](#spectators)); `-` leaves the row as the game has it; at most 4 per team and 2 spectators, and the game's own condition for its "Teams OK": two players at least and a player for each team, counting `-` rows (else `bad-settings`). A guest given the same rules (contract 2 gives them to every participant) finds its own seat by its name in them (status `seat`) | host; a guest's own seat |
| `mode` | `turf`, `zones`, `tower`, `rainmaker`, `clams` (the private room's mode selector, in its order: the game's rule ids 0-4), or the game's own rule name (`--list-match-options`); default `turf` | host |
| `stage` | `random` (the game's draw among the room's stages; default), a stage's scene name (`scene` in `--list-match-options`) or its number (the game's `VersusSceneInfo` Id, `stage` there): both name the same row of the game's stage table. The scene is the identifier to exchange with a server: it is also the room's `stage`, the status `stage_scenes` and the result's `stage_scene`, and it does not depend on the table's numbering. It must be in the host's room list ([Room settings](#room-settings)) | host |
| `main_abilities_only`, `figure_gear`, `figure_names`, `reset_settings` | `0` (default) or `1`: the room's four "other settings", in the order of its screen: gear's sub abilities off; the gear the game's NFC figures bring; the names they bring; camera and player settings reset at each match | host |
| `results_hold` | 0-600 s (default 10): how long the result screens wait for the player before the runtime moves them on ([Rounds](#rounds)) | each player |
| `spectators_wait` | 0-600 s (default 60): with teams by name, how long the host waits for missing spectators once every player is in; then the match starts without them ([Spectators](#spectators)) | host |

The service can rely on these keys and on the status and result files below. The room has no time limit, no weapon
restriction and, on LAN, no password ([Room settings](#room-settings)); its spectators are the team screen's `S` rows.
Recon (ZL on the room screen: the stage without a battle, no result) exists and is always off.

`splat3.exe --list-match-options` prints, from the user's game files (no game started), the mode keys with the game's
rule ids, names and labels, and every stage of the game's stage table with its id, scene, label and whether stage
lists show it; `KLM_LIST_LANG=<Mals prefix>` (`USen` by default, `EUfr`, `JPja`, ...) picks the labels' language.
Nothing of that table is in this repository: it is read from the RomFS (`RSDB/VersusSceneInfo`, `Mals/*`) and from the
game's code (its rule enum's text) at run time.

## Status file

`KLM_MATCH_STATUS=<path>` is rewritten whole at every change (a temporary file renamed over it, so a reader never
sees half a file). One JSON object on one line:

```json
{"version": 1, "match_id": "R3", "role": "host", "seat": "player", "state": "waiting-for-players",
 "since_unix": 1794346577.120, "detail": "waiting for the players", "present": 1, "expected": 2, "visible": false,
 "pid": 12345}
```

- `role`: `host` or `join` (`none` for a `bad-settings` error on `KLM_MATCH` itself): the player's part in the
  session, not in the match;
- `seat`: `player` or `spectator` (the room's stands, team letter `S`), or `unknown`: from the rules first (the host's
  own row; a guest whose name the rules' teams by name list), then from the match's record once it is read, which wins
  ([Spectators](#spectators)); `unknown` is a guest the rules do not name (no rules, or teams in arrival order) before
  its record;
- `since_unix`: when this state began (UTC seconds; a rewrite in the same state keeps it, `detail` may change);
- `visible`: the game window is shown; `pid`: the game's process (a status that stops changing while the process is
  gone means the game died: the launcher should check);
- `present`, `expected`: in `waiting-for-players`, the players in the room (the host included) and the number wanted;
- `error`, `reason`: in `error`, a stable code and an English sentence (also in `detail`);
- `save`: `"copy"` once the match's copy of the save is ready, with `loadout` `{"weapon", "gear": [head, clothes, shoes]}`
  when `KLM_MATCH_LOADOUT` was applied (-1: the save's own) ([The match's save](#the-matchs-save));
- `stages`, `stage_scenes` (once a room is up): the stages this player's room offers, by number and by scene, in the
  same order ([Room settings](#room-settings)); `room`
  (host: once applied; guest: when the stage loads): the room's settings as the game's own room state holds them,
  `{"mode", "rule", "rule_name", "stage" (scene or "random"), the four other settings}`.

| `state` | role | meaning |
|---|---|---|
| `booting` | both | the process started, the game loads (hidden) |
| `lobby` | both | the local lobby loads, then the session list is opened |
| `creating-room` | host | creating the session |
| `waiting-for-players` | host | the session exists; `present` of `expected` players are in it |
| `joining` | guest | searching for the host's session and joining it |
| `in-room` | both | host: every player is in, mode and teams set, starting; guest: in the room, waiting for the start |
| `loading-stage` | both | the stage loads (`detail` names its scene) |
| `in-match` | both | the intro has started; the window is shown (`visible`, unless `KLM_MATCH_REVEAL=never`) |
| `results` | both | the match ended; the result file is written; the result screens are on |
| `waiting-command` | both | with `KLM_MATCH_CMD`: the round is over, the window hidden, the room kept; waiting for `next` or `quit` |
| `finished` | both | done: with `KLM_MATCH_EXIT` (or `quit`) the window is hidden and the game closes now (exit code 0) |
| `error` | both | the flow stopped (`error`, `reason`); the game closes (exit code 3) unless `KLM_MATCH_ON_ERROR=show` |

| `error` | step | reason |
|---|---|---|
| `bad-settings` | start | a setting cannot work (no LAN play, no `lobby-lan` start, a bad match id, reveal value or rule key, a stage id the game's stage table does not have, a `KLM_MATCH_LOADOUT` that is not a file, a `KLM_MATCH_STAGES` value) |
| `save-unavailable` | boot | the match's copy of the save cannot be made, or the player has no save yet (a match needs a save that finished the first launch) |
| `loadout-failed` | boot | `KLM_MATCH_LOADOUT` is not a JSON object or is refused (an id the game's tables do not have; `reason` names it) |
| `stage-unavailable` | create | host: the stage asked for is not in this player's room list (`stages`) |
| `rules-not-applied` | start | host: the room's own state did not show the mode, stage and other settings asked for within 10 s |
| `teams-not-set` | start | host: after the team screen a player's team in the room is not the one asked for |
| `room-timeout` | list | host, between rounds: the room screen did not come back |
| `boot-timeout` | boot | the game did not reach its local lobby in time |
| `network-unavailable` | boot | the game's LAN network did not start; relay mode: the relay did not take the player into the room (unreachable, or refused without one of the reasons below) |
| `relay-full` | boot | relay mode: the relay refused the join because it is full (`JOIN_NAK` 5): its sessions or rooms are at their cap. A moment later it may have room |
| `relay-room-full` | boot | relay mode: the relay refused the join because the match's room is full (`JOIN_NAK` 4 without detail: every place, or every virtual address, taken). A relay up to server protocol 1.1.12 also says this when the reason below is the real one |
| `relay-source-limit` | boot | relay mode: too many members of the match's room come from this player's Internet address (`JOIN_NAK` 4 with detail 1, `docs/lan-relay-protocol.md` section 3.5): the relay's operator can raise that limit |
| `relay-source-full` | boot | relay mode: too many players on the relay come from this player's Internet address (`JOIN_NAK` 5 with detail 1): the relay's operator can raise that limit |
| `list-timeout` | list | the session list did not open |
| `create-failed` | create | host: no session could be created |
| `players-timeout` | players | host: not every player came (`present` of `expected` in the last `waiting-for-players` status and in `reason`) |
| `room-not-found` | join | guest: no session (of `KLM_MATCH_HOST`, if set) answered the search |
| `join-failed` | join | guest: a session was listed but joining it did not succeed |
| `host-left` | start | guest: the host's session traffic stopped for 20 s before the match started |
| `player-left` | start | host: a player's session traffic stopped for 20 s before the match started |
| `start-timeout` | start | the match did not start |
| `stage-timeout` | stage | the stage loaded but its intro never came |
| `left-by-player` | any | the player chose *Leave match* in the game's Esc menu, or closed the game's window while the stage loaded or the match ran (below); the status says whether the match was being played (`in_play`) |
| `match-aborted` | match | the game went back to its lobby without a match result (a player left, the connection was lost); the result file says `"outcome": "aborted"` |
| `record-timeout` | match | no match result in time |
| `connection-lost` | match | the session traffic of the host (guest) or of every member (host) stopped for 15 s (two players) or 45 s (more; `KLM_MATCH_SILENT`) in the match and the game did not end it; or the player left (menu or window) after that traffic had already stopped for 2 s |

## Leave match

While `KLM_MATCH` is in effect the Esc menu has a *Leave match* button, apart from *Resume* and *Quit to launcher*. After a confirmation it calls `automatch_leave` (`runtime/auto_match.c`), which stops the flow the way any failure does: the error goes to the status file, the window is hidden and the game closes with exit code 3 (`KLM_MATCH_ON_ERROR=show` does not apply: the player asked to go). *Quit to launcher* is the window's close button: the save is written and the process exits normally (exit code 0); while the stage loads or the match runs, `gfx_window_closed` first calls `automatch_closing`, which writes the same departure to the status file.

What a departure is depends on the session at that moment (`leave_state`; a server may make a departure in play lose
the match, protocol 1.1.11 `leave`, so the game says only what it knows):

| When the player leaves | `error` | `in_play` |
|---|---|---|
| before the match: the stage loading (the window is hidden before, so only a closed window there) | `left-by-player` | `false` |
| in the match, every other player of the match heard within the last 2 s | `left-by-player` | `true` |
| in the match, another player silent for 2 s or more while the session goes on (a team match) | `left-by-player` | `false` |
| in the match, the session already silent for 2 s (a guest: its host; the host: every member): the other game vanished first, the game's own "communication error" is on screen | `connection-lost` | `false` |

The status then also has `session_silent_ms`: the longest silence of another player of the match (or of the session,
for `connection-lost`) at the departure. The players of the match are the members heard at its `in-match` status
(a member who left the room before is not one); with teams by name the members the rules seat as spectators (`S`) are
left out. The game shows its "communication error" 2 to 5 s after the other game vanished, so a player who
leaves on that message is never taken for the one who broke the match. A launcher copies `in_play` into its report.

## The match's save

With `KLM_MATCH` (host or join) the game plays on a copy of the player's save: nothing of the match goes into the
player's own save, and what a launcher imposes for one match, a loadout (`KLM_MATCH_LOADOUT`) and every stage in the
room ([Every stage](#every-stage)), is written into the copy only. `runtime/fs_host.c` ("the match's copy of the save").

- At the game's first use of its save storage, before it reads the save: a copy a run left in `<profile>/match-save` is
  deleted, then the profile's `save/`, the commits `save-tx/` holds (a finished commit not swapped in yet, an interrupted
  one; not the working copy `save-tx/work`) and the cache storage `cache/` are copied there, and the copy becomes the
  game's storage root: its save, transactions, save backups, `temp` and `cache`. The copy is small (one file of about 18 KiB) and takes a few milliseconds.
- On the copy, in this order: `KLM_SAVE_PROFILE` (the launcher's character, as at any start), the stage season
  ([Every stage](#every-stage)), `KLM_MATCH_LOADOUT`. The loadout is a profile.json ([save-profile.md](save-profile.md))
  that names only what it changes, for example `{"weapon": <id>, "gear": {"head": <id>, "clothes": <id>, "shoes":
  <id>}}` (the game's table ids, `--list-match-options` does not list them: [save-profile.md](save-profile.md) says which
  tables); its ids are checked as a start profile's are, and an item the save does not own is added to the copy's
  ownership maps. No backup is made of the copy.
- At the end, on every exit path (the window closed, `KLM_MATCH_EXIT`, `quit`, an error's exit code 3, the game's own
  exit): a save commit still running is waited for, then the copy is deleted. A run that does not end (killed,
  crashed) leaves it; the next start of the same profile, with or without `KLM_MATCH`, deletes it first.
- What a match never writes in the player's profile: `save/`, `save-tx/`, `save-backup/`, `save-profile-backup/`,
  `temp/`, `cache/`. What it still writes there: `playtime.txt` (the engine's play time, not the game's save: a match
  counts), `resolution/` (a RomFS file the runtime generates at every start), and `account.uid` only when missing. The
  shader and pipeline caches are not in the profile: every run that names the same `KLM_SHADER_CACHE` and
  `KLM_PIPELINE_CACHE` shares them.
- Progress: what the game saves during the session is lost with the copy. The game commits its save once at every
  start (its boot and save counts), and a LAN private battle writes nothing else ([The result](#the-result): a save
  trace of a whole match); private battles give no experience, money or weapon progress. So a match loses the boot
  count and whatever the player changes in the game's own settings during it (camera, controls).
- No restart: the copy needs a save; without one the flow stops with `save-unavailable` instead of the first-launch
  path of [save-profile.md](save-profile.md) (a new save, then a restart once the game committed it), so a match never
  restarts the game.
- Errors, both before the lobby, out of sight, exit code 3: `save-unavailable` (no save yet, the copy cannot be made),
  `loadout-failed` (the loadout is not a JSON object or is refused; `reason` says why).

The status says `"save": "copy"` once the copy is ready, and `"loadout": {"weapon", "gear"}` with a loadout. The
result file has `"loadout": {"weapon", "gear", "ok"}`: `ok` is true when this player's own entry of the match record
holds the weapon and the gear asked for (-1 in the request: not checked). Every player's file also lists every player's
`weapon` and `gear` ([The result](#the-result)): what the other players' games received.

### Every stage

The room's stage list is built when the room opens (`main+0x3903e90`, the private settings in x0; the stage table's row
getter is `main+0x16a9bcc`): it takes the rows of `RSDB/VersusSceneInfo` that are shown in stage lists and whose `Season` is at most
`max(1, s)`, `s` the player data's `Season/CurrentSeasonId` (the Season object's `+0x48`, its serializer
`main+0x33cab78`) or, when the game is online, the online season. A save that never went online has -1 there: seasons
-1 to 1, 13 stages. `Plaza/UnlockedSeason` plays no part. With `Season/CurrentSeasonId` 8 the list is the 25 stages
shown in stage lists, in the table's order.

With `KLM_MATCH_STAGES=all` (the default) the runtime raises `1/Season/CurrentSeasonId` of the match's copy to the
newest `Season` of the listed stages (read from the table at run time: 8 in 11.2.0), so a room offers every stage
whatever the save's progress, and the status `stages` lists them; the player's own save keeps its value. `save` leaves
the copy's season as it is (the room offers the save's list, and a stage outside it is `stage-unavailable`).

## Out of sight

While the runtime drives the lobby (`KLM_MATCH` on, the window not yet shown):

- the window is created hidden (`gfx_window_win.c`, `gfx_window_sdl.c`): no taskbar button, no focus taken, nothing on
  screen; the swapchain is made for its client area and the game presents to it as usual;
- the sound is muted at the output (`audio_mute`, `audio_mixer.c`: the mix runs, at volume 0);
- the player's gamepad, keyboard and mouse are not read (`input_human`, `input.c`; the gamepad is still polled and its
  state dropped); only the runtime's own presses reach the game.

At the reveal the window is shown as a normal start shows it (raised, focused, `KLM_FULLSCREEN` applied), the sound
comes back and the player's input reaches the game; from then on the runtime presses nothing until the result screens
have waited `results_hold` seconds for the player (with `KLM_MATCH_REVEAL=never` it keeps pressing A every 4 s
through the match and the results). With `KLM_MATCH_EXIT=1` (or between [Rounds](#rounds)) the window is hidden and
muted again when the result screens end: when the room screen is up again or after `results=` seconds, whichever
comes first; then the game closes. The room is never shown.

The intro (`KLM_MATCH_REVEAL=intro`): after the stage's scene pack opens, the game shows a black loading screen, then
the intro (the rule's title over black, the camera over the stage with the players' names, the countdown). The runtime
shows the window when both of these hold:

1. the intro's music has started: the game opens a streamed track when it plays it, and the first versus music stream
   after the stage pack (`INTRO_STREAM` in `auto_match.c`) is the intro's (the match's own music, another versus
   stream, comes about 15 s later). It can open while the loading screen is still up;
2. the presents run steadily again: the stage's first frames build their pipelines and the loading screen presents in
   fits, so the first two 250 ms windows in a row with at least 13 presents that begin after the intro's music are the
   intro running.

The match's music shows the window at the latest. Limit: a loading screen that kept presenting steadily after the
intro's music would be shown for its remainder.

Rendering does not slow down while hidden: the runtime logs the presents it counts (`automatch: frames hidden|shown`,
10 s windows, 250 ms around the stage's load), and a hidden window presents at the same 60 frames a second as a shown
one. A step screenshot (`KLM_MATCH_SHOTS`) holds one present for 130 to 160 ms, hidden or shown. The mute is at the
output: `KLM_TRACE=audio` logs an output level of 0 in every window until the reveal.

## Relay mode and teams by name

In relay mode each match has its own relay room (`KLM_LAN_ROOM` = the match id), so the only session the guest's
search finds is its match's host. `KLM_MATCH_HOST` is then not needed; on a shared LAN (lan
mode) it is what picks the host.

Teams by name (`teams=<name>:<A|B|S|->,...`) need relay mode: the relay tells every member the others' names (its MEMBER
messages, `lan_relay_member_name`), and the host matches them with the session's members by virtual address. The team
screen's rows are the players in the order they came in, the host first: the host's row takes its own name
(`KLM_NICKNAME`, else `KLM_LAN_NAME`), each guest's row the relay name of the address whose session traffic came
next. A name not in the list, or not known, keeps the game's "any team". The relay does not vouch for names: a name two
members of the room joined with, or this player's own name on another member, names nobody (neither member is seated
by it, and the host waits as for a missing player). In lan mode there is no name on the wire in
clear for a guest (its search is encrypted), so the arrival-order form is the only one there.

## More than two players

What the flow assumes, for up to 8 players and 2 spectators (run with two players and one or two spectators: three and
four members in a session; [What was verified](#what-was-verified)):

- Waiting for `players=n`: the host counts the addresses whose session traffic (UDP 49152) is recent; in a full mesh a
  host hears every member. Holds for n up to 10 (16 addresses tracked).
- Rows of the team screen: the players in the order they came in, the host first; a guest's row is the address whose
  session traffic reached the host next. Teams by name (relay mode) map each row's address to the relay's name of that
  member, for any n; the arrival-order form `teams=AB-B...` is the only one in lan mode. At most 4 per team (checked at
  start and at each `next`: `bad-settings`, a `next` refused); teams of different sizes and `-` rows are allowed.
- Setting the rows: written into the save's 10 places before the team screen opens (no press per row); the team screen
  shows one row per member, spectators included, so `Down` x n reaches "Teams OK"; every asked row's team is then
  checked in the room.
- Start: every guest presses A at the pre-match screen every 3 s until the stage loads; a guest that never confirms
  holds the others until `start=` (`start-timeout`).

`splat3.exe --probe-automatch` runs the rules parser and the team rows without the game: two-player rooms in arrival
order and by name, an 8-player room by name in a shuffled arrival order, 3 against 1, a 5-in-one-team list refused,
and the rule keys.

### Spectators

The game's team screen, for reference (the save's places are one u32 per row at
`*(*(main+0x6097e78)+0xb8)+0x42c`):

- `Right` on a row cycles its place value 4 (any team) -> 0 (Alpha) -> 1 (Bravo) -> 3 (spectator) -> 4; 2 (the
  three-team mode's third team) is skipped.
- "Teams OK" is enabled only with two players at least and a player (or an "any team" row) for each team: a spectator
  needs a room of three at least. `teams_check` refuses other rules at start (`bad-settings`).
- After "Teams OK" a spectator's place holds no team in the room's team array (`+0xdc0`: -1, where the save's place
  said 3). A spectator row is right when the room put it in no team (any value but 0, 1 or 2).
- Every member's game, the spectator's included, stores the same `LastVersusRecord` at the results: the players'
  entries only, and `MyPersonalIndex` -1 on a spectator's game.
- The spectator's game confirms the pre-match screen with A, as a player's; it shows the match with the game's own
  spectator view, then the result screens like a player's, and goes back to the room.

What the runtime does for spectators:

- The room: `players` counts everyone in the session, so two players and one spectator are
  `players=3,teams=Alpha:A,Bravo:B,Charlie:S`. The host may be a spectator itself (`teams=Alpha:S,...`).
- `seat` in the status and the result: `player` or `spectator`. From the rules first: the host knows its own row; a
  guest finds its name in teams by name (contract 2 gives every participant the rules); otherwise `unknown` until the
  record. Then from the record: `MyPersonalIndex` outside its players is a spectator's (`record_seat`), which wins; a
  disagreement with the rules is logged and kept in the result as `seat_asked`.
- A spectator's result says `"outcome": "spectated"` where a player's says `"completed"`, `"seat": "spectator"`, no
  `self: true` entry and no `loadout`; the record's fields are there for information (the same as the players'). A
  report checker that takes `completed` only never counts it as a player's.
- `players` is everyone in the session: 2 to 10, at most 8 players besides the spectators and 2 spectators
  (`bad-settings` otherwise).
- A spectator who does not come: with teams by name, once every player is in, the host waits `spectators_wait` s more
  (default 60) for the spectators, then starts without the missing ones (status `waiting-for-players` says so; their rows
  are not set). In arrival order (no names) the host cannot tell a spectator from a player and waits for `players` as
  before.
- The test aid `KLM_MATCH_PLAY` presses nothing on a spectator's game.
- A spectator's departure in a match is not in the record (it has no entry there, so no `disconnected` flag), and the
  match goes on for the others: a host stops with `connection-lost` only when every member is silent.
- A spectator who comes after the match started joins the session (the host still answers searches) but stays in its
  lobby, hidden, and stops with `host-left` when the players' games close. A launcher should not start a spectator
  once the match has started.

In the spectator view every button the spectator uses has a default key ([controls.md](controls.md)): the camera
buttons act when held, and X opens a map of the stage.

## How the game is driven

The game has no automatic path for its LAN private battle. `LanBackgroundProcessJob::AutoConnectNetworkCreate` and
`...Scan` are states of the session library's own LAN job (the game's network middleware), used only by its `JoinRandomSession` ("join any matching session, else
create one"). The game wraps that call (its network system's virtual method at `main+0x44ea194`, command `0x05bc1741`,
`main+0x44ec410`, the library's `JoinRandomSessionAsync` at `main+0x970e2c`, with a LAN parameter block in mode 3), but its
Private Battle menu never uses it: the list calls the browse
request (`main+0x44ea9f0` -> the library's `BrowseSessionAsync` `main+0x9708a0`), the host the create request (`main+0x44ea67c`
-> `CreateSessionAsync` `main+0x970abc`), the guest the join request (`main+0x44ea834`), and the random join never
runs. The menu posts these requests as delegates to the network system's worker (`main+0x44ff270`), so the menu code
itself is not on their stacks.

So the runtime presses the buttons a player presses, each one gated on an effect the game shows, and repeats a step
whose effect does not follow:

| Step | Presses | Effect waited for |
|---|---|---|
| the local lobby is up | none | the `LobbyLocal` scene pack opens (`romfs.c`), then the nifm local-network request is accepted (the lobby's `InitLan`) |
| session list | `L`, `A` | the session library binds UDP 35000 (`lan_net.c` observer) |
| list searched | none | the library's browse job ends (hooked step `main+0x974f10`, `BrowseSessionJob::CompleteProcess`); a press before that is lost |
| host: create | `Y`, `A` | the library's network restarts: 35000 closed and bound again |
| host: players in | none | session traffic on 49152 from as many other addresses as `players` - 1 |
| host: mode, stage, other settings | none: written at the room screen's commit ([Room settings](#room-settings)) | the room's net states hold them (10 s at most) |
| host: teams | none: each row's team written into the save at the room screen's commit, before the team screen opens on them | read back from the save |
| host: ready | `Down`, `Down`, `A`, again 3 s later while the room screen's state (`+0x14`) does not change (a screen back after a match ignores presses during its entry) | the state changes: the team screen |
| host: teams OK | `Down` once per row, `A`, again while the state does not change | the state changes; every asked row has its team in the room (3 s) |
| guest: a session | `X` (search again) until one answers | a datagram on 35000 from another address starting with byte 1 (an answer; a search starts with 0), then the search's end |
| guest: join | `A`, `A` | the library's network restarts |
| pre-match | `A` every 2.5 s (host) / 3 s (guest) | the stage's scene pack opens (all of them open once at boot; this is the second time) |
| stage intro | none | the intro's music opens and the presents run steadily ([Out of sight](#out-of-sight)): the window is shown |
| match, results | none once shown (`A` every 4 s while hidden) | the match record appears (below) |

The function-entry hook (`FNPROF_AUTO`, `main.c`) is on only from the lobby to the stage's load.

A session on the wire: the guest's search is a 325-byte broadcast to 35000 (16 bytes of header, the rest
encrypted); each host answers with a 620-byte datagram that carries in clear its nickname (UTF-8 at 0x29 with its
length at 0x27, UTF-16 at 0x8d), its address (0x220) and what looks like a session id (bytes 4-7); the list shows the host's nickname. There is
no session name or password. One room per relay room is the simplest isolation; on a shared LAN `KLM_MATCH_HOST`
picks the host by nickname.

When the session library restarts its network for a join it closes its two sockets and binds the same ports at once,
while its receive thread may still be inside a call on the old socket: a bind waits up to 0.5 s for such a socket to
be released (`lan_net.c`), or the join would fall back to the plaza.

What a direct path (calling the game instead of pressing) would still need: the lobby menu's handlers that post the
create and join requests and keep the menu in step with the session (the delegates' sources) and the pre-match
"start". The room's settings and teams are no longer pressed blind: below.

## Room settings

Established on the host's room screen with a function census (`KLM_FN_CENSUS`: the
functions entered in a window around a press, against windows without one), function-entry traces with field dumps
(`KLM_FN_TRACE`) and memory reads and writes from the live input file (`PEEK`/`POKE`, [Tools](#tools)); offsets are
main's (11.2.0).

- The room screen offers: the mode (a selector, Left/Right), the stage (A opens a list: "random" and the stages, Y
  takes a stage in or out of the random draw), Recon (ZL), four "other settings" (X: gear's sub abilities off, the
  gear and the names the game's NFC figures bring, camera and player settings reset at each match), then "ready" and a team screen (8 player rows
  and 2 more, the spectators'). The session's creation dialog offers a single choice on LAN (open access): no password, no name (the
  search answer carries the host's nickname only). There is no time limit and no weapon setting anywhere.
- The settings live in the save data object (`*(main+0x6097e78)`): at `+0xb8` a pointer to the private battle's
  settings {`+0` stage: `VersusSceneInfo` Id, -1 random; `+8` the room's stage list (a count and, at `+0x10`, a pointer
  to entries {u32 stage id, u32 in the random draw}); `+0x42c` the team of each of the 10 places (u32: 4 before any
  choice, `Right` -> 0 Alpha -> 1 Bravo); `+0x454` the four other settings, one byte each in the screen's
  order (`00 00 00 01`: "reset" on); at `+0x219070` a pointer to {`+4` rule}. The rule is the game's
  `VersusRule` (six names in its enum text in main, read at run time): the mode selector cycles 0, 1, 2, 3, 4, 0,
  so rule 5 (the three-team mode) is not a private room's mode.
- The room screen's update (`main+0x38f7ff0`, every frame it is up, host and guests) calls its commit
  (`main+0x38f9930`, x0 the screen) on the host: the screen's rule (`+0xcf0`) goes to the save's, its Recon flag
  (`+0xcef`) to the room as the match type (3 Recon, 1 a battle), the save's stage to the room (the scene name looked
  up by Id, or "random" with the draw's count), and the other settings; the team screen opens on the save's teams and
  its "Teams OK" copies them into the room (`+0xdc0`, -1 before). The
  room's update (`main+0x388fad0`, `main+0x3890300`) then sends what changed to the members as two net states:
  `spl::VersusPrivateSettingNetState` {`+0x20` the random draw's count, `+0x24` match type, `+0x28` rule, `+0x2c` the
  four other settings, `+0x38` the 10 places' teams} and `spl::CommonPrivateSettingNetState` {`+0x28` -> the stage's
  scene name, "" for random; `+0x7c` random}; a member applies them to its own room and save (`main+0x3890d20`).
- Which stages a room offers is the private settings' stage list, filled when the room opens from the game's stage
  table and the save's season ([Every stage](#every-stage)): 13 on a save that never went online, the 25 stages shown in stage lists on the match's copy of the save (the table has 26). A stage outside
  the list is refused (`stage-unavailable`) rather than forced: the game's own screen cannot pick it.

What the runtime does (host): at the entry of the commit, every time it runs, it writes the wanted rule into the
screen's and the save's, the wanted stage id (or -1) and the four other settings into the save's private settings,
and Recon off; the game's own commit then sets the room and sends the settings, as when a player picks them (its own
screen keeps showing its cursor's choices; the window is hidden). The stage is written only once the room's list is
known and has it. Before "ready" the runtime reads back the two net states (their addresses taken at their
serialize and deserialize, `main+0x3d7c6f8`/`0x3d7c844`, `main+0x3d7cda4`/`0x3d7ced4`/`0x3d7cf68`) and goes on only
when they hold the mode, stage and other settings asked for (`rules-not-applied` otherwise; a random stage is the
random flag alone: the stage state's scene is "" until a match drew a stage and keeps the drawn scene after it).
The teams go the same way: each
row's team is written into the save's places at the commit before the team screen opens, read back, and checked in
the room after "Teams OK" ([How the game is driven](#how-the-game-is-driven)); a round after the first starts from
the last round's teams and gets the ones asked for. The result file
reports the record's rule and stage, which both players' records share, and the host's `requested` and
`settings_ok`.

## Rounds

After a match the game goes back to the room by itself, the session kept: the host's game comes back on the
room screen, the guest's in the lobby, both members in the room; no scene pack is opened for it
(`LobbyLocal` stays loaded), so the runtime takes the room screen's update running again (`main+0x38f7ff0`, hooked from
the record on) as the end of the result screens.

The result screens are the player's: the window is shown from the intro to the room. When the player gives no input
(gamepad, keys or mouse; `input_human_last_ms`, `input.c`) for `results_hold` seconds, the runtime presses A every
2 s, which moves the screens on, until the room screen is up (out of sight, `KLM_MATCH_REVEAL=never`: A every 4 s at
once). Then, with `KLM_MATCH_EXIT=1`, the window is hidden and the game closes. With `KLM_MATCH_CMD`
instead, the window is hidden (the room is not shown), the status says `waiting-command`, and the runtime waits for a
line appended to the command file (only lines appended after the game started count):

- `next [id=<match id>] [<rule keys>]`: another round. The match id (if given) becomes the round's, for the status
  and result files (a path with `{id}` gets it); the rule keys change only what they name (a guest needs none:
  `next id=<id>` is enough). Host: the room screen's commit takes the new settings, which are read back as in the first
  round, then ready, teams (a round starts from the last round's teams: they are written again), start. Guest: back
  from a match it stands in the lobby, where A does not confirm the pre-match screen (the game would then start
  the match only at the end of its own countdown, 90 s after the host's "Teams OK"); L and A open the room as at the
  first round, then A at the pre-match screen. The window is shown again at the next intro.
- `quit`: the window stays hidden and the game closes (exit code 0).

Each round's result file has `"round": <n>`; its timeline has the time from the `next` command to the round's intro
(`in the match ... after the next command`).

The room screen's update also runs in a guest's lobby, so it is not the end of a guest's result screens: a guest
presses A through its result screens for 20 s once they advance (`GUEST_RESULTS_MS`), then opens the room with L and
A at the next round. A round in the same room reaches its intro about 20 s after the `next` command, against about
50 s from a game's start.

## The result

A result file is written only when the game ended a match: `"outcome": "completed"` with the match's record, or
`"outcome": "aborted"` (below) when the game went back to its lobby without one; no result file means that no match was
played to its end (every failure of the flow is in the status file only, with its error code). `"version": 2`.
Every file has `seat` (`player`, `spectator`; from the record when there is one) and `seat_asked` (what the rules
said, `unknown` when they did not name this player); a spectator's file with a record says `"outcome": "spectated"`
instead of `completed`, has no entry with `self` true and no `loadout` ([Spectators](#spectators)).

A LAN private battle writes nothing to the save (the only write of a whole match is the profile's at
boot) and has no GameRecord equivalent. The result lives in memory: when the results start, the match stores it in
the local lobby's scene state as `LastVersusRecord` (the state's serializer `main+0x34f894c`, record reader
`main+0x3d80050`), in the state found as `scene_start.c` finds it (`*(*(*(main+0x60f24e0) + 0xd8) + 0x6678) + 0x40`).
The runtime polls it from the stage's load on and takes it when its `DateTimeUtc` changes with sane counts.

Record layout (readers `main+0x3d80050`, player `main+0x3d7e724`, its UserSetting `main+0x3567a00`, its Result
`main+0x3d89df8`):

- record: `Rule` +0x30 (0 = turf), `DeemedResult` +0x90 (-1 here), `StageId` +0x94, `DateTimeUtc` +0x98 (Unix
  seconds), `WinTeam` +0xa0 (0 Alpha, 1 Bravo: Alpha's screen said "defeat" with 1), `BattleFrame` +0xa4 (60 per
  second), team array {buffer +0xa8, capacity +0xb0, start +0xb4, count +0xb8; 0x1b8 bytes each, index = team},
  player array {+0x5e8, +0x5f0, +0x5f4, +0x5f8; 0xe60 bytes each}, `MyPersonalIndex` +0x7900;
- team: paint points +0xa0 (the team's players' points), turf share in tenths of a percent +0xa4 (inferred: 19 and 40
  for 43 and 92 points);
- player: name (string chars at [+0xe0]), team +0x150, kills +0xac8, assists +0xacc, deaths +0xad0, paint points
  +0xad4, specials +0xad8, disconnected +0xadc, player index +0xe50.

A host's file (round 1 of a session), shortened:

```json
{"version": 2, "match_id": "S-1", "role": "host", "round": 1, "outcome": "completed",
 "rule": 3, "rule_name": "<the game's rule name>", "mode": "rainmaker", "stage_id": 12, "stage_scene": "<stage 12's scene>",
 "requested": {"mode": "rainmaker", "stage": 12, "main_abilities_only": 0, "figure_gear": 0, "figure_names": 0,
               "reset_settings": 0},
 "settings_ok": true,
 "room": {"mode": "rainmaker", "rule": 3, "rule_name": "<the game's rule name>", "stage": "<stage 12's scene>", "main_abilities_only": 0, ...},
 "end_utc": 1794368315, "battle_frames": 18000, ...}
```

Each team also has `count` (`+0xa8`: 100 at the start; the modes' count, inferred from two matches), and each player
`weapon` (`+0x240`) and `gear` (`[head, clothes, shoes]`, `+0x2c8`, `+0x4d0`, `+0x6d8`), the game's ids. `abilities` (prototype): for each of the three pieces `[main, sub, sub, sub]` as ability enum values, -1 for an empty slot (the gear record's `+0x14`, `+0xa0`, `+0x118`, `+0x190`, which hold the ability's position in main's enum; measured in few matches). How far each field can be relied
on: `rule`, `stage_id`, `win_team`, the teams' paint and share, every player's team, kills, assists, deaths,
specials, paint, disconnected, weapon: read from the record the game shows on its result screens, equal in every
player's file (the shared outcome); `count`, `gear`: measured on few matches; `deemed_result` (-1 in every match run)
and the meaning of `disconnected` for a player who left are not measured beyond that.

`battle_frames` (and `duration_s`) is each game's own count and is not identical between players: equal in most
turf matches (10800), a guest's a few frames longer than the host's in the other modes (18000 against 18001 to 18007). The raw records (`<result>.bin`) of both players hold no other field equal to the
host's count, so the runtime cannot report one shared value. Inferred, not measured: a turf match ends on each game's own
timer, the other modes end when the host ends them (a knockout, the end of overtime) and a guest counts until that
message arrives. Take the host's value, or compare with a tolerance.

`rule_name` is the game's own name of the rule; `requested` and `settings_ok` (the record's rule and stage are the
ones asked for; with `stage=random` the rule only) are the host's; `room` is what the room's net states held when the
stage loaded. An `aborted` file has `outcome`, `reason`, `stage_scene`, `room` and the timeline, and no record fields.

The record's fields in such a file, shortened:

```json
{"match_id": "M1", "role": "host", "rule": 0, "mode": "turf", "stage_id": 11, "stage_scene": "<stage 11's scene>",
 "end_utc": 1794342745, "battle_frames": 10800, "duration_s": 180.00, "win_team": 0, "winner": "alpha",
 "deemed_result": -1,
 "teams": [{"team": "alpha", "paint_points": 43, "turf_permille": 19},
           {"team": "bravo", "paint_points": 17, "turf_permille": 7}, {"team": "charlie", ...0}],
 "players": [{"name": "Alpha", "self": true, "team": "alpha", "paint": 43, "kills": 0, "assists": 0, "deaths": 1, ...},
             {"name": "Bravo", "self": false, "team": "bravo", "paint": 16, "deaths": 0, ...}],
 "t0_unix": 1794342500.623, "timeline_s": [["lobby loading", 7.9], ...]}
```

The guest's file is the same except `role` (`join`), the `self` flags, `t0_unix` and `timeline_s`. Field by field:

- equal in both reports (the game's shared outcome): `match_id` (the launcher's), `rule`/`mode`, `stage_id`,
  `stage_scene`, `win_team`/`winner`, `deemed_result`, every team's points and share,
  every player's name, team, paint, kills, assists, deaths, specials, disconnected flag, and the players' order;
- equal on one PC but not guaranteed: `end_utc` (two copies on one PC read the same clock; across PCs it is each machine's
  clock unless the game synchronizes it, not checked);
- different by nature: `role`, `self`, `t0_unix`, `timeline_s`, and `requested`/`settings_ok` (host only).

Also seen: a team's paint points can be one more than its players' sum (Bravo 17 against 16 here), so a check should compare the team values between reports, not recompute them from the players.

## When a member disappears

In a match the game itself ends nothing when a member disappears: it shows its "communication error" 2 to 5 s later
and the match stays frozen behind the message, with no record; pressing A takes the player back to the game's lobby,
which the flow does not see (no scene pack loads again). The runtime stops the flow instead. No case below writes a
result file: none ends a match.

| What happens | The other side |
|---|---|
| the host vanishes while the room waits | guest: `host-left` after 20 s of session silence (`PEER_SILENT_MS`), window never shown, exit code 3 |
| a guest vanishes while the room waits | host: `player-left` after 20 s, or `teams-not-set` when the team screen was already being confirmed, exit code 3 |
| a member vanishes while the stage loads or in the match | `connection-lost` once the session has been silent for `KLM_MATCH_SILENT` seconds: 15 in a match of two players (it cannot go on without the other one), 45 with more (the others may play on). A guest stops when its host's traffic is that old, a host when every member's is |
| the relay stops for a few seconds | both rejoin the relay room with their addresses, but the game's session does not survive the outage: `connection-lost` on both sides |
| a guest never comes | host: `players-timeout` (`present` of `expected`), exit code 3 |
| a guest looks for the wrong host (`KLM_MATCH_HOST`) | guest: `room-not-found`; the host: `players-timeout` |

A match the game itself ends early (back on its room or lobby without a record) gives `"outcome": "aborted"`. What a
player's own departure is: [Leave match](#leave-match).

## Capabilities

`splat3.exe --list-match-options` is also the machine-readable list of what this build drives, for a launcher to pass
on to a match server as it is. `"format": "splat3-match-options", "version": 1`; members:

| Member | Meaning |
|---|---|
| `modes` | `{key, rule, name, label}` per mode the room offers: the rule key, the game's rule id and name, its label in `language` |
| `not_in_private_rooms` | the game's other rules (id, name, label): never asked |
| `stages` | `{stage, scene, label, in_stage_lists}` for every row of the game's stage table: `stage=` takes the `scene` (recommended as the identifier a server exchanges) or the `stage` number, two names of the same row |
| `stage_random`, `stage_list` | `stage=random` works; `"all"`: a room offers every stage with `in_stage_lists`, whatever the save ([Every stage](#every-stage); `"host_save"` in builds before it: the stages of its host's save, status `stages`) |
| `teams` | 4 places per team, 2 to 8 players, uneven teams and "any team" rows allowed, teams by name in relay mode, `spectators` true; `verified_players_max`: the most players run in one match (2) |
| `spectators` | `max` 2, `team` `"S"` (the `teams` letter), `in_players` true (`players=` counts them, `session_max` 10), `modes` (the mode keys a room can have spectators in: all of the room's modes), `verified_max` and `verified_modes` (what was run), `host_verified` (a host in the stands was run), `seat` (where the seat is reported), `result_outcome` (`"spectated"`), `wait` (the `spectators_wait` key) ([Spectators](#spectators)) |
| `room_options` | each other setting's key with its values and default |
| `recon`, `time_limit` | false: not drivable / not in the game's room |
| `rounds` | `next` commands and what each round may change; `faster_than_restart`: a round in the same room reaches its intro sooner than a new game (about 20 s against about 50 s) |
| `results_hold` | bounds and default |
| `loadout` | `"profile"`: a player's weapon and gear come from its save (the launcher's `KLM_SAVE_PROFILE` before the start); the result reports each player's |
| `match_loadout` | `KLM_MATCH_LOADOUT` works: `{"setting", "format": "save_profile", "fields": ["weapon", "gear"], "player_save_written": false, "progress_kept": false, "checked_in_result": true}` ([The match's save](#the-matchs-save)) |
| `capabilities` | names of what this engine does beyond the members above; a launcher shows a feature only when its name is listed. `gear-abilities`: a save profile's `gear_abilities` and `chunks` are applied ([save-profile.md](save-profile.md)) |
| `result` | the result file's version, outcomes (`completed`, `aborted`, `spectated`) and `seat` (the file has one) |

A new member may appear in a later version; a member's meaning does not change within a version.

### What else could be driven

Against the game's own room and records; "now" is implemented and was run ([What was verified](#what-was-verified)).

| Setting | Status | Evidence, and how every player gets the same |
|---|---|---|
| mode, stage, other settings | now | the host's room sends them to every member (net states); read back before the start; the record of every player has the same rule and stage |
| teams, uneven teams | now (2 players run) | room places checked after "Teams OK"; the team values per place are the room's, sent by the host |
| series (best of n), rematch, stage rotation | now | `next` rounds in one room, each with its own mode and stage; about 20 s from command to intro ([Rounds](#rounds)); the server counts the rounds from the result files |
| weapon and gear per player | now, for one match: `KLM_MATCH_LOADOUT` on the match's copy of the save | each player plays the loadout of its save's copy ([The match's save](#the-matchs-save)); the room has no loadout rule, so nothing forces it in the game: the server checks each player's `weapon`/`gear` in the result files (the same in every player's file) and refuses the result if they differ from what it assigned |
| match duration | not in this game | no time setting in the room; the record's battle frames: 10800 (turf) and 18000 (the others) in every match run |
| spectators | now (1 and 2 spectators with 2 players run, turf and zones, the host in the stands too) | `teams` letter `S` writes the place value of a spectator (3); the game confirms the teams only with two players besides the spectators; every member's game, the spectator's too, gets the same record, and the spectator's has no entry of its own: its result says `spectated` ([Spectators](#spectators)) |
| recon | not useful | a stage without a battle, no result |
| password, session name | not on LAN | the creation dialog has one choice; a relay room is the isolation |

A setting that changes the simulation must be the same for every player: the ones above are either the host's room
state, which the game itself sends to every member before the start (a member cannot play other rules), or a player's
own loadout, which the game also sends to the others for the match (every record has every player's weapon). A local
change (a modified game, a memory editor) is outside what the files can prove ([Trust](#trust)).

## Tools

For questions about the game's screens, such as the room settings above:

- `KLM_FN_CENSUS=<file>` (`scene_start.c`): while a window is open every function entry of main is counted; the
  live input line `CENSUS <name>` (`KLM_INPUT_LIVE`) closes the current window (one `<window> <offset> <count>` line
  per function entered) and opens the next, `CENSUS off` closes the last. Windows around a press against windows
  without one give the functions the press runs.
- `PEEK <address> <length>` and `POKE <address> <bytes>` in the live input file log or write guest memory (hex).
- `KLM_FN_TRACE`: function-entry traces with arguments, strings, stack and field dumps.

## What was verified

All runs were made on one Windows PC, with copies of the game talking over loopback addresses (lan mode on
127.0.0.2/8 and 127.0.0.3/8, relay mode through a loopback relay):

- two players, from process start to the match's result and the games' exit with no input, in lan mode and in relay
  mode with teams by name; both result files agree on every shared field;
- every mode of the private room at least once, set stages and a random stage, the four other settings read back;
- rounds in one room with `KLM_MATCH_CMD`, with another mode, stage and swapped teams;
- a loadout on the match's copy of the save: both players' profiles, hashed file by file before and after, are
  identical except `playtime.txt`;
- two players with one spectator and with two (three and four copies), the host in the stands, rounds with a
  spectator, a spectator who comes late, a spectator who vanishes in the match;
- the disappearances of [When a member disappears](#when-a-member-disappears), each provoked once, and a player's
  own departure ([Leave match](#leave-match)).

Not verified: more than two players in a match (the row order with more than three guests, the confirm with more than
four rows, the pre-match start with 8), copies on different PCs or across the internet, the SDL window back end
(Linux), `deemed_result` and the `disconnected` flag of a player who left.

## Trust

Everything here is computed by the client: a modified client (or a memory editor) can write any file and any value in
the game's own record. Two reports of the same match agree exactly on every field of the record (they are the game's
shared outcome); a server comparing the host's and a guest's report detects one side lying alone, not both sides
colluding, nor a lie made in the game's memory before the record is built (a modified game sending false paint to
its peers changes both reports). See the result section for which fields legitimately differ.
