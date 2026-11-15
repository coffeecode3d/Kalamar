# Launching the game

The environment the launcher gives the game, the play modes, the first screen and the command line.

## The environment of every launch

The launcher starts the game with environment variables the engine documents
([launcher-interface.md](../../engine/docs/launcher-interface.md)):

- always: `KLM_SHADER_CACHE`, `KLM_PIPELINE_CACHE`, `KLM_ROMFS_PROOF_DIR`, `KLM_XLATE`,
  `KLM_PROFILE_DIR=<root>\userdata\profile` and `KLM_DISCORD_PRESENCE=launcher` (the launcher shows the Discord
  presence: the game it starts never opens Discord's pipe);
- for network play (`lan.rs`): `KLM_LAN` (`lan`, `lan:<ip>/<prefix>` or `relay:<ipv4>[:<port>]`), `KLM_LAN_ROOM` and
  `KLM_LAN_NAME` (relay), `KLM_LAN_BIND=any`, `KLM_NET_LOG=1`. The game takes IPv4 addresses only and never resolves
  a name: the launcher resolves a relay's host name and passes the address.

Every launch first removes the variables the launcher owns from the inherited environment (`launch::OWNED_ENV`), so a
stray `KLM_LAN` in the shell cannot turn an offline launch into a network one. All launches pass
`--profile <root>\userdata\profile`; the user data folder is `<root>\userdata`, so a custom root never touches another
folder's save.

A game folder named for another game version than the one the launcher builds (`work\splatoon3-11.2.0` while it builds
11.3.0: `installer::gamever::is_outdated_root`) still starts, for solo play: nothing in a launch depends on the version.
It is not offered for online play (the community pages ask for the game update, the match runner refuses a match, the
server is not told of the game: [install-wizard.md](install-wizard.md), "One game version, and the update to the next").

## Start modes

A start mode opens the game directly in a scene through `KLM_START`, which wins over the `start_scene=` setting
([save-profile.md](../../engine/docs/save-profile.md), "Start scene"). There is no separate "skip title" variable:
`plaza` skips the title screen and the news by itself.

| Mode (`id`) | Button in the launcher | Environment added | Result |
|---|---|---|---|
| `original` | Play, Full game | `KLM_START=title` | the game as it starts normally, from its title screen; `title` also overrides a `start_scene=` of the settings file |
| `range` | Training | `KLM_START=range` | the shooting range |
| `hub` | Free roam | `KLM_START=plaza` | the hub in free play |
| `lan` | Network play (LAN), in the advanced mode | `KLM_START=lobby-lan` + the network variables above | the local-network lobby; disabled until the LAN feature is enabled and its settings are valid |

`startmode.rs` holds the table; a `start-modes.json` in the data directory overrides an entry of the same `id`
(`{"modes":[{"id":"range","var":"KLM_START","value":"range","enabled":true}]}`) without a rebuild; an unknown `id`
is ignored. Matches of a community server and private matches by code are started by the launcher with the match
variables of [auto-match.md](../../engine/docs/auto-match.md), not through this table
([community-client.md](community-client.md)).

After a LAN launch the launcher shows a short help: wait for the lobby, press the console's L button (key Z in the
default layout, or whatever `bind_l` says in Settings), open the room menu, create or join a room.

### Second local player (advanced, tests on one PC)

Play, "Advanced": a second instance with its own user data folder (`<root>\userdata-p2` by default, never `userdata`),
nickname and the first player's profile under that name. It selects the folder with the same `--profile` /
`KLM_PROFILE_DIR`, gets a copy of the first player's shader and pipeline caches once (never its save) and its own window
title. In LAN mode it takes `127.0.0.3/8` (set the first player to `127.0.0.2/8` on the LAN page). **Two instances used
about 30 GB of RAM and 10 GB of VRAM on one test machine.**

## The first screen

The first screen is meant to feel like a game launcher: open it, press **Play** (or Enter), the game starts offline.
What the Play screen shows is described in [interface.md](interface.md); this section is about what it costs.

- **One call, no big files.** The page asks the backend for everything the first screen needs in a single command
  (`boot`: configuration, install status, game processes, profile.json, servers file, migration notice, command line).
  Install detection is a handful of `stat` calls (`install::detect_light`: exe, the RomFS image's *size*, the index, the
  translator, the save folder, the settings file); the RomFS image (about 10 GB) is never opened or read at start-up,
  and the logs folder is only listed by the full `install_status`.
- **Game data waits for the Locker.** The catalog (RomFS index + cached `catalogue.json`) and the icons are loaded
  when the Locker is opened, when a launch has to check the profile, or when a community screen shows a stage; the
  rail's player card shows the name until then. The Play screen's preflight is the light one (`play_preflight(light)`): it does not load the catalog, so its
  "profile ok" pill is provisional (`deep: false`) until the Locker or a launch has loaded it; a launch always checks
  the profile completely.
- **The window.** Created by the backend (`shell.rs`, `"create": false` in `tauri.conf.json`) at the saved place and
  size (`window.json`, written when the window closes; ignored when no screen shows the title bar any more), with the
  theme color as window and WebView background and a dark title bar: no white frame while the WebView starts.
- **Pages by need.** Only React, the shell and the Play screen are in the first script; the other pages, the wizard and
  its stylesheet load when first opened.
- **Timeline.** `KALAMAR_TRACE=<file>` (or `1`) makes the backend append `epoch ms<TAB>label` lines (setup phases, every
  command's arrival, page load events, install and catalog timings); the page records `performance` marks
  (`js:eval-start`, `app:mounted`, `play:ready`, ...) and a `cmd:<name>` measure for every IPC call. Read them through the
  WebView2 DevTools port (`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`).

**The main button.** Without a community server, the big button of the Play screen starts the full game, offline,
and has the keyboard focus, so Enter is enough. With a server the big button is that server's **Play online** (it
opens the Community page) and takes the focus instead; the offline modes (Full game, Training, Free roam) are the
buttons of the solo block under it. While the game runs the button becomes **Back to the game** (brings the game
window to the front) next to **Stop**.
A game started here for solo play does not block a community match: when a match must start, the launcher closes that
game first (a close request to its window, then, after 6 seconds, termination of that process only) and starts the
match with its own environment, within the server's launch deadline
([community-client.md](community-client.md)). Searching, a found match and an open room say so beforehand. A match
in progress is never closed this way. A private match started from a code still refuses to start while a game runs.
Settings > Launcher > "Minimize the launcher while the game runs" (off by default) minimizes the launcher when the game starts
and brings it back when the game ends (the taskbar, not the notification area: the launcher's icon there only stands for
a game update that runs when the launcher closes, and carries the end-of-install notification).

**Command line**, for a desktop shortcut (`Kalamar.exe --play training --minimized`):

| Argument | Effect |
|---|---|
| `--play full` / `training` / `roam` | starts that offline mode as soon as the install status is known, exactly as its button does (profile, options and the "leave my save alone" choice of the launcher; an incomplete profile or a missing install is reported on screen). LAN and private matches cannot be started this way. Ignored while a game is running or when the install wizard has to open. |
| `--minimized` | opens the launcher in the taskbar without taking the focus |

Anything else on the command line is ignored; a bad `--play` value shows a notice.

## Files the launcher reads from its data directory

All are optional.

- `settings-schema.json` replaces the built-in settings schema.
- `start-modes.json` overrides the play-mode table (see above).
- `config.json`: game root, translator, language, launch options, relay program path, interface choices (minimize while playing, advanced mode, sounds), the update check's choices (`update`: automatic check, channel), the Discord presence's (`discord`: on, level, mode and stage, server name; all off by default).
- `window.json`: size and position of the window (written when it closes).
- `update\` (a build with an update repository only): `state.json` (last check, ETags, "Later"), `available\` (the
  last verified manifest and signature, checked again at each start), `download\<tag>\` (the download), `journal.json`
  and `witness.json` (a replacement under way), `lock`.
