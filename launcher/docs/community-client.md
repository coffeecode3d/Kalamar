# Community servers and the protocol client

## Community servers by name (server-gated features)

Without a server the launcher is an offline tool: the Play screen shows the full game (offline) and one card "Play
with others" that adds a server (online play is provided by independent community servers). The Community page and
everything online are absent, not locked; manual LAN stays in the advanced mode. Once a server exists
(`servers::features`), the rail shows **Community**: for a relay-only server it is the private match by code, through
that server's relay; a server with a community service shows its own page (next section). Removing the last server
hides it again.

A server (`online\servers.json`, list and selection; `online\<id>\` holds that server's own files) is `name`, `relay`
(host name preferred, or IPv4, plus UDP port, default 47313) and optional `api` (https URL of the community service,
next section). Unknown keys are ignored. Added from one typed line
(`relay.example.org`, `relay.example.org:47313`, `203.0.113.7:47313`) or from a server card. IPv6 literals are refused
with a clear message: the game side is IPv4-only. Server card of the first format (at most 64 KiB, strict: unknown keys
refused; read from a local file, nothing fetched; nothing saved before the player confirms):

```json
{ "name": "Friends server", "relay": "relay.example.org:47313", "api": "https://play.example.org" }
```

`name` (1 to 40 characters) and `relay` are required; older cards name the relay `match_server`, and the `address`,
`port` and `ca_pem` keys of earlier cards are accepted and ignored. The game never resolves names: at each match launch
the launcher resolves the relay's host name with the system resolver (`servers::SystemResolver`), passes the IPv4 to
the game, and keeps the last good address for display only; a failed lookup says which name failed and why.
Match codes made for a named relay are version 2 (`KLM2-…`): the host name travels in the code with the address the
host saw as a fallback; each guest's launcher resolves the name itself. Relays given as IPv4 keep version 1 codes,
unchanged. A `match_server` left in `config.json` by a pre-release launcher becomes a server entry at start-up, once; the
setting itself is left as it was.

## Community protocol client (Kalamar protocol 1.1)

A server that has a community service is driven by the open protocol specified with the reference server
(`kalamar-server/docs/protocol/`: overview, server card, API, events, match contracts 1 and 2, relay, activities,
series, presentation). The launcher is a data-driven client of it: it shows only what the server declares and renders
the server's data as given, so a server can invent its own tournaments, leagues, challenges or themed evenings
(activities) without a launcher change. It sends `Kalamar-Protocol: 1.1`.

- **Adding a server** (`proto::glue::proto_discover`, the "Add a server" dialog): the typed `host[:port]` (a DNS name or
  an IPv4; a pasted `https://` and one trailing `/` are tolerated) gives `https://<host>[:port]/.well-known/kalamar-server.json`
  (at most 64 KiB, 10 s, at most 3 redirects, never to plain http), then `GET <api>/v1/meta`, which is authoritative.
  The https and loopback-http attempts run together; a line already known as a relay (a saved relay, or the relay's
  UDP port 47313) gets 3 s before the relay-only offer.
  The player sees the name, operator, region, languages, declared areas, rules, privacy link and the API host when it
  differs from the typed one, and confirms. A server is refused when it shares no 1.x version, no match contract the
  launcher implements (`lan-auto-match` 1) or no usable login. **When no service answers** (no document, an invalid
  one), the line can still be added as a **relay only**: private matches by code, no accounts.
  Server cards of the new format (`{"kalamar_server": 1, "name", "api", "relay"}`) are checked the same way; older
  cards keep their import. HTTPS only, with the system's certificate store (rustls + platform verifier); plain http
  only to `127.0.0.1`, `localhost` and `[::1]`, for a test server. Certificate checks are never turned off.
- **What appears.** With such a server selected, the rail shows **Community** and Play shows **Play online**, which
  replace the private match by code; relay-only servers keep the private match by code. The
  page has one tab per declared area, in this order: private rooms, queues, events (activities), party, friends (with
  presence), rankings (ladders grouped by the server's `group`, its `primary` one first; ratings, leaderboards),
  statistics (and history), profile (the server-designed page), account (with badges and title). An undeclared area is not shown and its
  endpoints are never called (`proto::meta::area_of` refuses them in the backend too). Queue formats, room options
  (a form built from the server's field declarations: teams, mode, stage, password, the server's own keys), rating
  tiers, leaderboard columns, result policies and penalties are the server's data; mode names come from the
  launcher's own names, else the server's catalog, else the id; unknown members and values are ignored.
- **Accounts.** The server's login providers: `direct` ones as forms of their declared fields (guest, dev on test
  servers), `oauth_loopback` ones (Discord) through the system browser with PKCE and a loopback redirect. Display names
  are at most 10 characters (the game shows no more). The session (access and refresh tokens) is stored in
  `online\<id>\session.dat`, encrypted for the Windows user with DPAPI, never shown to the page or logged; refresh
  rotation is followed, a revoked session is forgotten, logging out (this device or all) deletes it.
- **Your data.** The Account tab offers the two rights every server of the protocol gives (`proto/account.rs`).
  "Save a copy of my data" asks for `GET /v1/me/export` (read back up to 32 MiB) and writes the answer as a JSON file
  where the player says in the system's "save as" dialog, under the proposed name
  `kalamar-export-<server host>-<UTC date>.json`; the page never chooses the path and the content is never logged.
  "Erase my account" opens a confirmation that says what is deleted and what the server keeps, offers to save a copy
  first, and wants the display name typed. The backend checks the name again, stops the event stream, sends
  `DELETE /v1/me?confirm=erase`, then forgets the session as after a logout (no logout request follows: the server
  has ended the sessions); after a refusal the stream starts again. It is not offered during a match or a search.
  Neither request can be made through the page's general request command.
- **Activities (1.1).** The server's list (icon, kind, state, players, a countdown, its join button) and each
  activity's **view**, rendered from the protocol's closed component list (`src/community/view.tsx`): text, notice,
  fields, list, table, countdown, progress, bracket, series, match, user, form, actions, link, group. Actions go to
  `POST /v1/activities/{id}/actions/{action}` with the view's revision (asked first when the server sets `confirm`);
  the returned view replaces the shown one, its notice is shown, a stale view is fetched again; `activity.updated`
  refetches, `activity.notice` shows a message. A view is data, never code: plain text only (no HTML is ever set),
  links open only on a click, in the system browser, with their host shown (`proto_open_link`: https only), no image
  is ever shown or fetched, the protocol's limits are checked (an error card replaces an oversized view), an unknown
  component shows its `fallback` text or nothing. The 24 icon names are drawn by the launcher (`src/community/icons.tsx`,
  original drawings).
- **Series (1.1).** The series card (sides, wins, games), kept fresh by `series.updated`; a match of a series shows
  "game n · best of N", a match of an activity names it, `under_review` reads "waiting for a referee".
- **Presentation (1.1).** Badges (featured badge) and titles (`PATCH /v1/me`), the server's profile view, User badges and
  titles next to names.
- **Events.** One WebSocket while logged in and the page has been opened once (or from the launcher's start when the
  player ticked "stay connected in the background" for that server, off by default): bearer token (a ticket when a proxy
  drops the header), pong to the server's pings, dead after 3 silent heartbeats, reconnection with backoff (1 to 30 s,
  jitter) and resume (`resume` + `last_seq`); when the stream cannot be resumed, the page and the match runner refetch
  `GET /v1/state` and `GET /v1/matches/current`. A found match brings the launcher to the front and asks
  accept/decline with the server's countdown.
- **Matches** (`proto::contract`, `proto::runner`): when the player's match reaches `ready`, the launcher checks it
  (contract, mode, players, stage), resolves `game.relay.host` to an IPv4 itself and starts the game through the same
  launch as a private match, with `KLM_START=lobby-lan`, `KLM_LAN=relay:<ipv4>[:<port>]`, `KLM_LAN_ROOM` and
  `KLM_MATCH_ID` = the room, `KLM_LAN_NAME` and `KLM_NICKNAME` = `you.member_name`, `KLM_MATCH=host|join`,
  `KLM_MATCH_RULES` = `game.rules` **verbatim** (contract 1: host only; contract 2: every participant; its keys are
  the runtime's), `KLM_MATCH_HOST` = `game.host_name` (guests), the status and result files in
  `online\<id>\matches\...` (contract 2: `{id}` in the paths, the engine puts each round's id there),
  `KLM_MATCH_EXIT=1`, `KLM_MATCH_ON_ERROR=exit`, `KLM_MATCH_REVEAL=intro`, `KLM_MATCH_TIMEOUTS` when the
  server gives it. It reports the launch, mirrors every change of the status file (at most 2 per second), uploads the
  result file as the game wrote it (version 2 `aborted` ones too), or an error report (`launcher-failed`, `-crashed`,
  `-cancelled`, `-relay-unresolved`, `-unsupported-rules`, `-incompatible`, `-loadout-unavailable`; contract 2: the
  status file's error code when the engine wrote no result), retrying on network errors; a match canceled before the
  stage closes the game. The next game of a series waits for the previous game to close (it closes by itself after its
  result) rather than failing.
- **A game opened for solo play is closed for the match.** A match needs a fresh game with its own environment, so a
  game the player started from the launcher for any other mode (Full game, Training, Free roam, LAN) cannot play it.
  Just before the launch (after the loadout choice), when such a game of the first player is running (or restarted
  into its second process), the runner goes to phase `closing` ("Closing the game you had open..."), asks the game to
  close like its window's close button (`WM_CLOSE`, `winutil::request_close`), waits 6 seconds for it to end by itself,
  then terminates it (only the process the launcher started and the one it restarted into; `launch::close_open_game`)
  and waits 3 more seconds. The match then starts within its launch deadline. The launcher's own match in progress
  is never closed (it keeps its wait and the refusal of a second game). When the game cannot be closed, the match
  stops in phase `failed` with the code `game-not-closed`, the page shows its own sentence, and the server gets
  `launcher-failed` with the reason, like any failed launch. Until then the page warns the player: a notice on the
  search ticket (searching, match found) and in a room that is open says that the open game will be closed when the
  match starts and that what is not saved in it is lost. The second player's game is not touched, and neither is a
  game this launcher did not start.
- **Engine capabilities and contract 2.** `PUT /v1/me/game` is sent after each login and each new stream. Its `build`
  is `kalamar/<launcher version> runtime/<engine snapshot, 12 digits> game/11.3.0`, the same for every player of the
  same build (`proto::engine::build_label`). Its content comes from the engine itself: `splat3.exe --list-match-options` (the engine's JSON list, no window, no game started)
  runs once per executable (size and date), only when a server needs it, never at start-up, and is cached in
  `<data>\engine\match-options.json`. The list is relayed verbatim (`options`), with `contracts` (1 and 2 when the
  engine writes result files of version 2), `engine` (rule keys, team limits, rounds), the mode keys and the stages
  (those of the player's room list once a status file gave them). Stage identifiers are the game's scene names, the
  strings the result reports in `stage_scene` and the server compares with `rules.stage` (`match-contract.md` 2): the
  engine numbers its stages, so the launcher maps them to their `scene` (the engine's `stage=` takes either). The list
  is read in the launcher's language (`KLM_LIST_LANG`), and its labels name stages and modes on the community page
  before the server's catalog. An engine without the command keeps contract 1 and
  the five mode keys. **Rounds in one game session** (contract 2 `keep_open`, `KLM_MATCH_CMD`) are
  announced when the engine says a round there is faster than a new game (`rounds.faster_than_restart`:
  about 20 s against about 50 s); a game continues the open session only with the same room, host and role (else the
  open game gets `quit` and the match is launched anew), and gets `quit` when the series is over or when no continuing
  match came within `game.continue_window_s` (default 90 s). A match replacing a void one (`rematch_id`, a stage the
  host lacked) or the next game of a series starts once the previous game is closed. A server line's port is the HTTPS
  port of its discovery document; the relay comes from the service; a relay only is added on the player's confirmation.
  `incompatible_game` shows what each engine lacks ("your game engine cannot do: ...; update it").
- **Waiting made readable, and a game too old.** "Your match" gives one sentence per engine state (`booting` to
  `results`; `proto/progress.rs` reduces the status file, `community/waiting.ts` holds the page logic, tested with
  `npm run test:ui`), the players awaited with `present`/`expected` while the host waits, the time spent in the step
  and, when the step is exactly one engine step, the time before the engine gives up (the documented defaults of
  `KLM_MATCH_TIMEOUTS` replaced by the match's `game.timeouts`; nothing is claimed for `booting` and `lobby`),
  that the window stays hidden until the intro, and a help line when another player stays at "launched" for 90 s while
  the host waits. `proto_game_check` (`proto/freshness.rs`) tells a game that does not know `--list-match-options`
  (too old: "ready", "start" and "accept" are refused on the page, the runner refuses the match with
  `launcher-incompatible`, and the message leads to the install wizard) from one compiled from another engine snapshot
  than the embedded one (warning only: the compile step's record in `kalamar-install.json` against
  `runtime-src.json`). A listing that merely timed out proves nothing and blocks nothing.
- **Loadout constraints** (`rules.loadout`, launcher-applied weapon and gear): applied when the engine lists
  `match_loadout` (a match plays on a copy of the save, deleted at the end; the player's save is never
  written), declared in `engine.loadout`, refused with `launcher-loadout-unavailable` otherwise. This player's
  constraints (`players` over `teams` over `all`, per slot): a fixed id as is; a choice from the player's pick (the
  match card lists the allowed items, or the Locker catalog's items minus the denied ones, until shortly before the
  launch deadline), else the Locker's current item when it complies, else the first allowed one. The ids go to the
  engine as a save profile (`KLM_MATCH_LOADOUT`), the launch report says what was applied, the result's
  `loadout.ok` is shown. During a match, the Locker's profile (`KLM_SAVE_PROFILE`) also reaches only the copy. For
  each gear part the file names, it also carries the player's sub-abilities of the Locker (`gear_abilities`, three
  names or `null`s) and main ability (`main_abilities`, a name) when the player chose them; what is reported to the
  server stays ids only.
  An engine that reports `stage_list: "all"` offers every listed stage whatever the save: the whole list is declared.
  The reference server imposes no loadout; `proto_peer --loadout-rules` hands constraints to the runner to try it.
- **Several relays and a changing offer (1.1.9).** `meta.changed` makes the backend read the meta again (after a
  random delay of up to 5 s, once per `revision`; a burst of events gives at most one more read) and declare the
  relays again when their values changed (`proto/relayping.rs`, `RelayJob`), and the page reload everything drawn from
  the meta (`community/store.ts`): an operator's `admin reload` shows without a restart of anything.
  `proto/relayping.rs` measures the relays of `meta.relays` within the limits written in "Network accesses"; a login
  and a stream resync declare the last values at once, and the measurement declares again when it ends and pushes
  `proto-relays` to the page (the Account tab shows each relay address once: measuring, its latency, no answer, not
  measured, or refused address). While the game runs the last values are declared again. The server alone chooses the
  relay of a match, which the launcher starts the game on. Browser preview: `&relays=1` (every state, the
  measurement ending after 3 s).
- **A player who leaves (1.1.10).** Measured with two real games (the server's `docs/protocol/leaving.md`): in a 1v1
  the other game shows its own communication error 2 to 5 s after a player vanishes, no game writes a record, and the
  match is void. A game closed during the match (its window, *Quit to launcher*, the launcher's stop: exit code 0 or
  the stop, no result, no error in the status file) is reported as `left-by-player`, as the game's *Leave match*,
  so a server's penalty for leaving applies to both alike. A
  launcher that starts (again) while its player's game had already reached the stage, or had reported, never starts
  the game again (the engine cannot join a match in progress: measured, it looked for its host for 5 minutes and the
  match waited for it): it sends the earlier game's result file, else its status file's error, else
  `launcher-crashed`, once that game is gone (`runner.rs`, `resume`). A match whose view says `you.free` (protocol 1.1.10: the player's report is in) does not block the queues and the stands, and shows that the player may search
  again.
- **A forfeit (1.1.11).** A server's result policy may make a player who leaves a match in play lose it (`leave`:
  `forfeit`); only the leaver's own report counts, with the engine's `in_play`. When the engine wrote the departure in
  its status file (*Leave match*, or its window closed while the stage loads or the match runs: `left-by-player` with
  `in_play`, or `connection-lost` when the player left a match whose session had already stopped), the launcher
  reports that error and copies `in_play` into the report, whatever the contract version (`runner.rs`, `watch` and
  `resume`; `contract.rs`, `in_play`); without it (an engine that does not write it) it reports without `in_play`,
  which no server counts as a forfeit. A match won or lost that way (`outcome.forfeit`, reason `left`) says so on the
  result and in the history: "Victory by forfeit — your opponent left the match", "Defeat by forfeit — you left the
  match" (team matches: a player of the other team, of your team); the server's summary is not repeated
  (`texts.ts`, `leftForfeit`). The launcher asks for no confirmation: the game's *Leave match* has its own. Browser
  preview: `&match=forfeit&fft=won|lost|team|mate` (with `&section=stats`: the history).
- **Tests.** `cargo test` runs the unit tests. With the reference server running locally
  (`kalamar-relay --bind 127.0.0.1`, `kalamar-service --config deploy/kalamar.local.toml serve`),
  `set KALAMAR_TEST_HOST=127.0.0.1:8080` then `cargo test --lib proto::live_tests -- --ignored --test-threads=1`
  plays rooms, queues, penalties, a dropped stream and a crashed game with two simulated launchers;
  `KALAMAR_TEST_OAUTH_HOST` names a server whose `oauth_loopback` provider is a local stand-in;
  `KALAMAR_TEST_STANDIN=<stand-in splat3.exe>` adds the 1.1 tests with the stand-in as a real game process (its
  capability list read by the launcher's code): the reference server's best-of-3 activity (continued in the open game
  when rounds are announced and the host stays, relaunched otherwise) and its four-player cup (a fresh server: the cup
  reopens only a while after its final). `cargo run --example proto_peer -- --host 127.0.0.1:8080 --name Bravo --room
  <code>` (or `--queue <id>`, or `--activity <id> --action <id>`, with `--standin <exe>` for a game process) is a
  headless second launcher to try the page against a test server; `--game-root <root> --userdata <folder>` makes it
  start the real game with its own user data folder (never the player's own `userdata`), `--play <s>` adds the engine's test
  aid `KLM_MATCH_PLAY`, `--dir <folder>` keeps its data.
- **Verified end to end with the real game** (two copies on one PC, the reference server 1.1 and its relay on
  loopback, the launcher driven through its page and `proto_peer` with the real game as the second player, each game
  with its own user data folder): a private 1 v 1 room on a chosen mode and stage, a ranked duel from a queue (rating
  changed), the best-of-3 event relaunching the game between games (series won 2-0), and a game killed in the match
  (the other side stops with `connection-lost` 45 s later; the server voids the match) all reached the server's
  decision. From the start of the game to the match's intro: about a minute.

## Private match

Create: the launcher makes a code `KLM1-XXXXX-...` (`KLM2-` when the relay is a host name, see above; both are accepted) (version, relay IPv4:port, 24-symbol room, host nickname, players
2-8, mode, round, CRC-16; base32 without 0/1/O/I; `matchcode.rs`), the host shares it and presses Start. Join: paste the
code, check the summary, Join. The game is started with the variables of `automatch.rs` (one table, overridable by
`match-vars.json` in the data directory); the "Connecting" screen follows the status file (`KLM_MATCH_STATUS`) or,
without it, the `automatch:` lines of the run log; at the end the result JSON gives the result screen. "Play again"
raises the round (same room base, id `<room>-<round>`), which every member can compute. The relay is the one of the server in use.
