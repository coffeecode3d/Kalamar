# Discord presence

What the player's Discord contacts see while Kalamar is open ("Rich Presence"). **Off by default**: the player switches
it on in Settings › Discord, where a preview shows exactly what goes out. A build without a Discord application id has
no such feature at all (no section, nothing opened).

## What goes where

- **Only the Discord application installed on the player's PC.** Kalamar opens the local named pipe
  `\\.\pipe\discord-ipc-<n>` of the running Discord (n = 0 to 9: stable, PTB, Canary...) and speaks Discord's documented
  IPC protocol (`src-tauri/src/discord/`): no HTTP request to Discord, no Discord library, no new dependency. The pipe is
  opened with an identification-only security level (the program behind the pipe cannot act as the player). The
  Discord application then shares the status with the player's contacts (friends, members of their Discord servers), as
  for any game.
- **Nothing before the player switches it on.** Before that, the launcher only lists the pipe names on Settings ›
  Discord, to say whether Discord is open (a directory listing: nothing is opened or sent).
- **Never sent**: another player's name, a room code or password, a user, room or match identifier, the address of a
  relay or a service, a path or file name, the game's name or pictures. The page reduces what it knows to a situation
  that holds none of them (`src/discord/presence.ts`, tested by `src/discord/presence.test.mjs` on states full of such
  values); the back end keeps only the members it uses, only the launcher's own picture keys, and drops a text that looks
  like an address, a match code or an e-mail (`activity.rs`, and an end-to-end test against a stand-in Discord).
- **Mode and stage names** (an option, off by default) come from the player's own game data on this PC: the stage names
  of the Locker's catalog or the engine's list, the mode names of the engine's list or the launcher's own words. Never
  the server's labels, never an identifier.
- **The server's name** (an option, off by default) is the name the player saved for it, never its address.

## What a contact sees

Discord shows the application's name ("Playing Kalamar", set by the maintainers below), then two lines, a group size, a time,
the large picture (the Kalamar logo, hover text "Kalamar") and, at the activity level, a small picture. The texts are in
the launcher's interface language (English below; French, Spanish and Japanese in `src/locales/<lang>/app.ts`, keys
`dc.*`).

**(a) Minimal**: `In the launcher` (time since the launcher started) or, while the game runs, `In game` (time since the
game started). Nothing else.

**(b) Activity** (the default once switched on):

| Situation | First line | Second line | Group | Time | Small picture |
|---|---|---|---|---|---|
| Play page, Settings... | In the launcher | | | launcher start | |
| The Locker | In the launcher | In the Locker | | launcher start | |
| Full game or free roam | In game | Offline | | game start | `act_solo` |
| Training | In game | Training | | game start | `act_solo` |
| Local network (LAN) | In game | Local network | | game start | `act_match` |
| Private match by code | In game | Private match | | game start | `act_match` |
| Logged in on a server, in its menus | Online | (In a party) | (2 of 4: the party) | since logged in | `act_online` |
| In a private room | Private room | 2 v 2 | 3 of 4 (players, spectators not counted) | since joining | `act_room` |
| Searching a queue | Looking for a match | Ranked · 4 v 4 (or Unranked) | the party, when 2 or more | since the ticket | `act_search` |
| Match found (accept) | Match found | Ranked · 4 v 4 | | since the ticket | `act_search` |
| Stage draft | Picking the stage | Ranked · 4 v 4 | | since the match | `act_match` |
| In a match | In a match | Ranked / Unranked / Private room · 4 v 4 | | since the game started | `act_match` |
| In the stands | Spectating | Private room · 1 v 1 | | since the game started | `act_watch` |
| Two minutes after a match | Match over | Victory / Defeat / No result (a spectator: nothing) | | | `act_done` |

"Ranked"/"Unranked" of a search is known once the Queues tab listed the queues; otherwise the line is left out.

**(c) + mode and stage**: the second line of a room, a found match, a draft, a match, the stands and a result gets the
names, e.g. `Ranked · 4 v 4 · Turf · <the stage's name in the player's game>`. A name the player's game does
not give is left out.

**(d) + server name**: the first line of every online situation gets it, e.g. `In a match · Free Ink`.

The game started on its own (a shortcut to the executable, not through Kalamar) follows the same switch and only says
`In game` (level a) with the same application and logo: it cannot know the rest. Started by Kalamar, it says nothing
(next section).

## One source: the launcher

- The launcher passes `KLM_DISCORD_PRESENCE=launcher` to every game it starts (`launch::build_launch_spec`): such a
  game never opens Discord's pipe, whatever its settings (`runtime/discord_rpc.c` in the engine; its log says
  `discord: started by the launcher, which shows the Discord presence: this game stays quiet`).
- The launcher copies the switch and the application id into the game's settings file (`kalamar.ini`, `[discord]`:
  `discord_rpc`, `discord_app_id`) when the player changes the switch and at each launch, for the game started on its
  own. The game's Esc menu (Game tab) offers the same switch then.
- A machine-wide lock (the named mutex `Local\Kalamar.DiscordPresence`) lets one Kalamar show the presence at a time: a
  second launcher, or the game started on its own while a launcher shows it, stays quiet and says so; it takes over at
  its next attempt once the other is closed.

## Behavior

- One thread owns the connection (`worker.rs`); the window and the game never wait for it. It is started the first
  time the player switches the presence on.
- It lists the pipe names first; with no Discord, it tries again every 30 s, with nothing on screen but the line of
  Settings › Discord. It tries `discord-ipc-0` to `-9` in order: a pipe that closes, refuses or stays silent for 3 s is
  skipped. Discord closed: noticed at the next look at the pipe (twice a second, without writing), looked for again
  30 s later. Discord refusing the application id (Close code 4000) is said on the Settings page; tried again 10 minutes
  later.
- An activity is written only when it changed, after 1.5 s without another change (a room filling up sends one
  update), and never more than 5 per 20 s (Discord's documented limit for activity updates); the latest one wins.
  Elapsed times are timestamps Discord counts itself: nothing is sent to make a clock tick.
- Switched off: the presence is cleared, the pipe closed, the lock given back. The window closed (an update running in
  the notification area afterwards): cleared. The launcher quitting: cleared and closed, waiting at most 0.8 s.

## For maintainers: the Discord application

The application id is public (it names the application and its pictures); it is not a secret and gives no access to
anything. Nothing else of the application (client secret, bot, OAuth2, redirects) is used.

1. Open the Discord Developer Portal (`https://discord.com/developers/applications`), sign in with the account that
   should own the application, **New Application**, name it **Kalamar** (Discord shows "Playing Kalamar"), accept the
   developer terms.
2. **General Information**: an icon is optional (`app-icon.svg` rendered, i.e. `docs/discord-assets/kalamar_logo.png`,
   does). Copy the **Application ID** (17 to 20 digits).
3. **Rich Presence › Art Assets**: upload each file of `docs/discord-assets/` (1024 x 1024 PNG) under **exactly** this
   name (the name is the key; Discord lowercases names, these already are):

   | Key | File | Shown |
   |---|---|---|
   | `kalamar_logo` | `kalamar_logo.png` | the large picture, always |
   | `act_solo` | `act_solo.png` | offline game, training |
   | `act_online` | `act_online.png` | in a server's menus |
   | `act_room` | `act_room.png` | in a private room |
   | `act_search` | `act_search.png` | searching, match found |
   | `act_match` | `act_match.png` | stage draft, match, LAN, private match by code |
   | `act_watch` | `act_watch.png` | in the stands |
   | `act_done` | `act_done.png` | a match's result |

   They are the launcher's own drawings (`app-icon.svg`, `src/discord/art.ts`); `node scripts/discord-assets.mjs`
   draws them again (with the installed Edge, offline). Upload no picture of the game or its publisher. Uploaded
   pictures can take a few minutes to show in Discord. Leave the "Rich Presence Invite Image" empty (joining from
   Discord is not offered, below).
4. Write the id in `src-tauri/discord-app-id.txt`: every release build (`npm run tauri:release`,
   `scripts\make-release.ps1`) then carries it. `KALAMAR_DISCORD_CLIENT_ID=<id>` in the environment of a build (`build.rs`;
   a value that is not 17 to 20 digits fails the build) or `scripts\make-release.ps1 ... -DiscordClientId <id>` names
   another one, and `none` in either place makes a release without the feature. A plain development build reads neither
   the file nor anything else: it has no Discord feature.
5. Try it: start that build with Discord open, Settings › Discord, switch it on, look at your own profile in Discord
   (or ask a friend). `splat3.exe --probe-discord` with `KLM_DISCORD_APP_ID=<id>` checks the engine's side alone (it
   shows "In game", in the menu language, for ten seconds).

## Joining from Discord: not offered

Discord's local channel can carry a "Join" button, but its join secret is stored by Discord and given to whoever
joins. The community protocol joins a room by its code or by an invitation addressed to a server user id, and has no
single-use token to put in that secret instead: a room code there is exactly what must never leave the launcher. A
joiner whose launcher is not running would also need a command registered in the user's registry. The presence
therefore never offers to join.

## Tests

- `cargo test --lib discord`: the frame codec, the activity checks, the payload's members, and a stand-in Discord
  (`discord/fake.rs`, a named pipe of the test's own name, never `discord-ipc-*`): handshake, activity received, clear at
  the end, nothing while off, the rate limit and the latest-wins rule, the settle delay, no Discord then Discord started,
  Discord closed and opened again, a silent pipe skipped for the next, a refused application id, pings, an error answer,
  two launchers and one presence, nothing private on the wire.
- `npm run test:ui` (`src/discord/presence.test.mjs`): every situation at every level in English (the strings the tests check), and with a translator
  that writes every value it is given, against a list of private values (names, codes, ids, hosts, addresses).
- Engine: `python tests/discord_ipc_check.py <splat3.exe>` (a stand-in Discord in Python; the probe with an application,
  started by the launcher, without an application, with a bad pipe name).
- Not tested here: a real Discord client (the tests never talk to one).
