# Kalamar launcher

The desktop launcher of Kalamar, an unofficial fan project. It builds a native version of a game on the player's PC
from the player's own copy, starts it, and is the client of independent community servers. It ships no game file and
sends no telemetry. Tauri 2 (Rust) + Vite + React + TypeScript; Windows.

The project as a whole (what it is, what is not included, how players install it) is described in the
[repository's README](../README.md). This page is for people who work on the launcher.

## Develop

Node.js, the stable Rust toolchain and the WebView2 runtime are needed.

```powershell
npm ci
npm run tauri dev          # the window, with hot reload
npm run dev:mock           # the interface alone in a browser, on made-up data
npx tsc --noEmit           # type check
npm run test:ui            # tests of the page's logic and of the dictionaries
cd src-tauri
cargo test                 # unit tests of the back end
cargo clippy --all-targets -- -D warnings
```

`npm run tauri:release` makes the release executable and the NSIS installer (`scripts\release-build.ps1`: the paths of
the build machine are remapped out of the executable, then checked). Releases are described in
[docs/releasing.md](docs/releasing.md).

A development start must not touch a player's real data: set `KALAMAR_APPDATA=<folder>` (it stands for
`%USERPROFILE%\AppData`: its `Roaming` and `Local` hold the data folder and the WebView's profile) or
`KALAMAR_DATA=<folder>` (the data directory, default `%APPDATA%\dev.kalamar.launcher`).

| Variable | When | Effect |
|---|---|---|
| `KALAMAR_GAME_ROOT` | run | default game folder |
| `KALAMAR_DATA`, `KALAMAR_APPDATA` | run | where the launcher keeps its data (above) |
| `KALAMAR_TRACE=<file>` | run | a timeline of the start-up ([docs/launching.md](docs/launching.md)) |
| `KALAMAR_UPDATE_REPO=owner/repo` | build | gives the build its update feature, checked with the public keys of `src-tauri/update-keys.txt`; unset: no update feature |
| `KALAMAR_DISCORD_CLIENT_ID=<id>` | build | gives the build its opt-in Discord presence; unset: no such feature, except in a release build (`npm run tauri:release`), which takes the id of `src-tauri/discord-app-id.txt` unless the value is `none` |

The older names `SPLAT3_GAME_ROOT` and `SPLAT3_LAUNCHER_DATA` are still read.

## Layout

| Path | What |
|---|---|
| `src/` | the React interface: `api.ts` (typed IPC), `i18n.tsx` and `locales/` (English is the reference; French, Spanish, Japanese), `pages/`, `community/` (the community client's page), `install/` (the wizard), `update/`, `discord/`, `dev/mock/` (made-up data for the browser preview, never in a production build) |
| `src-tauri/src/` | the Rust back end: `install` (detection), `ini` + `settings` (the game's settings file, schema-driven), `launch` (starting the game, process tracking), `gamelang`, `lan`, `online` + `servercard` (saved community servers), `profile`, `game/` (reader of the player's own game data), `proto/` (client of the community protocol), `installer/` (the install wizard), `update/` (signed updates), `discord/`, `commands` (IPC) |
| `src-tauri/resources/` | `settings-schema.json` (data-driven settings) and the two bundled archives ([docs/install-wizard.md](docs/install-wizard.md#bundled-source-and-tools)) |
| `src-tauri/examples/` | developer tools, not part of the application: `proto_peer` (a headless second launcher), `wizard_run` and `wizard_probe` (the install pipeline without the window), `bundle_runtime`, `bundle_tools`, `release_sign`, `gamedump` |
| `kit-tools/` | the helper scripts the wizard runs on the player's files (extraction, RomFS merge, save keys) |
| `scripts/` | build and release scripts, the screenshot runner (`npm run shots`) |
| `docs/` | the pages below |

## Documentation

| Page | Content |
|---|---|
| [docs/network-accesses.md](docs/network-accesses.md) | every network access the launcher makes, and when |
| [docs/security-model.md](docs/security-model.md) | what the launcher protects players from, and the risks that remain online |
| [docs/install-wizard.md](docs/install-wizard.md) | building the game from the player's files: steps, resume, updates, the bundled archives |
| [docs/launching.md](docs/launching.md) | the environment given to the game, play modes, the first screen, the command line |
| [docs/game-data.md](docs/game-data.md) | names and icons read from the player's own game data; `profile.json` |
| [docs/community-client.md](docs/community-client.md) | community servers, the protocol client, private matches |
| [docs/interface.md](docs/interface.md) | design system, navigation, the browser preview |
| [docs/discord-presence.md](docs/discord-presence.md) | the opt-in Discord presence |
| [docs/releasing.md](docs/releasing.md) | making and signing a release |

## Rules that matter here

- Nothing derived from the game is committed, bundled or shipped. Names and icons are read from the player's own
  files at run time; the bundled engine sources are scanned before they are written and again by `cargo test`.
- The game never contacts anyone by itself: the launcher resolves names and hands the game an address. Certificate
  checks are never turned off.
- A server's data is shown as data: plain text, no HTML, no image fetched, links only on a click.
- Interface texts live in `src/locales/<code>/`; a new key goes into every language (the type checker and
  `npm run test:ui` refuse a missing one).

The repository's [CONTRIBUTING.md](../CONTRIBUTING.md) and [AGENTS.md](../AGENTS.md) hold the full rules.
