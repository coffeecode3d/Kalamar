# Kalamar

> **This fork adds a Linux port.** See [LINUX.md](LINUX.md) for the AppImage, the system requirements and how this
> branch stays level with upstream. Everything below is the upstream project's documentation and remains true, except
> that the launcher and the game also build and run on Linux.

**Kalamar** builds a native Windows version of a Nintendo Switch game *from your own copy of it*, on your own PC, and
lets you play it offline with keyboard, mouse or gamepad. Multiplayer exists only if you choose to add a community
server, and anyone can run one or write their own from the open documentation.

It currently supports *Splatoon 3*, version 11.3.0.

> [!IMPORTANT]
> **What this is not.**
> - **No game files are included.** No code, data, images, sounds, fonts or keys of the game are in this repository
>   or in the launcher. You need to own the game and the console, and your own copy of the game and of its 11.3.0
>   update, dumped from your own console, with your console's keys. How to obtain them is not explained here, and
>   no help is given for it.
> - **Not a way to get the game.** The build happens on your machine, from your files; the result contains the game
>   and is for your own use only. Do not share it.
> - **No original online service is contacted.** The game never talks to the publisher's servers. Without a
>   community server added by you, the launcher makes no network access, apart from the build tools its install
>   wizard downloads after listing them and asking you, and, in published releases, the check for its own updates
>   on GitHub (at each start and every 6 hours while it stays open, can be turned off; no personal data sent, GitHub sees the IP address; nothing is
>   installed without your consent).
> - **Discord presence is off by default.** Switched on (Settings › Discord), the launcher tells the Discord app of
>   your PC, through its local channel, what you are doing at the level you chose, and Discord shows it to your
>   contacts: never other players' names, room codes or server addresses.
> - **Not affiliated** with, endorsed by or connected to Nintendo or any game publisher. An independent fan project.
> - **No money.** Nothing is sold, no donations are asked, and the server protocol has no payments.

## What is in this repository

| Folder | What |
|---|---|
| [`launcher/`](launcher/README.md) | the **Kalamar launcher** (Tauri 2 + Rust + React/TypeScript): install wizard that builds the game from your files, offline play, settings, your character, and the client of community servers |
| [`engine/`](engine/README.md) | the **engine**: static recompiler (your game's code to C) and native runtime (Vulkan renderer, input, audio, LAN play); a fork of totk-pc-recomp (MIT) |
| [`docs/`](docs/README.md) | guides for players, operators and contributors |
| [`scripts/`](scripts/) | repository checks run by CI (English-only text, relative links, nothing derived from the game, third-party inventory) |

The protocol specification (version 1.1, currently revision 1.1.13), the reference community server, the UDP relay
and the conformance test kit live in the companion **server repository**,
[kalamar-server](https://github.com/CyriacFR/kalamar-server). How the pieces fit together:
[docs/architecture.md](docs/architecture.md).

## Status

Experimental, and tested on few machines. What that means today:

| Area | State |
|---|---|
| Install wizard (build from your files) | works; measured end to end on one PC (8 cores, 32 GB), little tested elsewhere |
| Offline play (start-up, character creation, the hub, the test range, the full game offline) | works on the few machines it was tried on, mainly with NVIDIA cards; visual bugs and crashes are still expected |
| LAN play (one network, or through a relay) | works with two players |
| Community matches (rooms, queues, series, a match won or lost by forfeit) with the reference server, protocol 1.1.13 | measured with **two players** only |
| Spectators | measured by the engine with copies on one PC; not verified end to end with a server |
| Matches with more than two players | **not verified** |
| AMD graphics cards | **not verified** with this game (a crash at load and graphical defects were reported on one) |
| Intel graphics, Windows on ARM, Linux | not tested (Windows on ARM is refused by the wizard; Linux is not offered) |
| OAuth logins (Discord, Google...) on a community server | tested against local stand-ins only, never against the real providers |

Requirements: Windows 10 or 11 x64, a CPU with AVX2 and FMA, a graphics card with Vulkan 1.3 and
`VK_EXT_external_memory_host`, about 15 GB free during the first install (about 13 GB afterwards). Details:
[docs/getting-started.md](docs/getting-started.md).

## Security: what to know before playing online

Offline play makes no network access. Online play, through a community server, carries risks that Kalamar reduces
but cannot remove:

- **The game's network code is not hardened.** It is the original game's code, recompiled: not memory-safe, not
  sandboxed, and what other players, the relay or a LAN host send reaches it after only size, source and destination
  checks. A bug there could let a malicious player run code on your PC. Play with people and servers you trust.
- **A server's operator is trusted widely.** It chooses your relay, starts the matches you joined, sees your IP
  address, names, room codes and results, and can show any text and links.
- **Results are reported by the players' launchers**, and decided by the server's rules: collusion, second accounts
  and modified games are out of reach of any cross-check.
- **The relay protocol is plaintext.** A room code is the only key of a room: someone who sees your network traffic
  can join it. Share room and match codes privately.

What the launcher does about each, and the other remaining risks:
[launcher/docs/security-model.md](launcher/docs/security-model.md). Reporting a vulnerability: [SECURITY.md](SECURITY.md).

## Quick start

**Players.** Download the launcher from the [releases page](https://github.com/CyriacFR/Kalamar/releases) (the
portable `Kalamar.exe` or the installer), start it, and let its install wizard ask for your three files (the game, its
11.3.0 update, your console's keys file) and build the game. Then press **Play**. Step by step:
[docs/getting-started.md](docs/getting-started.md).

**Building from source.** The launcher builds on Windows only: the prerequisites and the commands are in
[CONTRIBUTING.md](CONTRIBUTING.md#building-and-testing-the-launcher) (`npm ci`, then `npm run tauri build` in
`launcher/`). The engine is not built by hand for playing: the launcher's wizard builds the game from your files;
building the engine yourself is described in [engine/docs/building.md](engine/docs/building.md). Linux: the engine's
runtime builds and runs there ([engine/docs/building.md](engine/docs/building.md#linux-in-progress), in progress), but
the launcher and its install wizard do not yet, so there are no Linux build steps for the whole project here.

**Developers.** On Windows with Node.js 24, Rust (stable, MSVC) and Python 3:

```powershell
cd launcher
npm ci
npm run dev:mock           # the interface alone in a browser, on made-up data: http://localhost:1430
$env:KALAMAR_APPDATA = "$env:TEMP\kalamar-dev"   # a throwaway data folder (see CONTRIBUTING.md)
npm run tauri dev          # the real launcher window, with hot reload
cd src-tauri; cargo test   # the launcher's Rust tests
```

Everything else (prerequisites, tests, style, how to send a change): [CONTRIBUTING.md](CONTRIBUTING.md).

## Documentation

The index is [docs/README.md](docs/README.md). The main guides:

| Guide | For |
|---|---|
| [Getting started](docs/getting-started.md) | the first install, playing offline, settings, what to do when something fails |
| [Adding a community server](docs/servers.md) | what changes when you add a server, what the launcher sends and does not send |
| [Writing your own server](docs/write-a-server.md) | from zero: architecture, the match contract, conformance, the smallest server, testing |
| [Architecture in 10 minutes](docs/architecture.md) | a new contributor's map of the engine, the launcher, the server and the protocol |
| [The engine's interface for a launcher](engine/docs/launcher-interface.md) | writing another launcher or a script that starts the game |
| [Legal notes](docs/LEGAL-NOTES.md) | factual notes on content, licenses, names and data (not legal advice) |

## Contributing

Contributions are welcome: read [CONTRIBUTING.md](CONTRIBUTING.md) first (in short: no file, extract or image of the
game, ever; small focused pull requests with tests). Report security problems privately: [SECURITY.md](SECURITY.md). Bugs and
ideas go to the issue forms; there is no private support: [SUPPORT.md](SUPPORT.md).

## License

Kalamar is under the MIT License: [LICENSE](LICENSE) for this repository's own files and the launcher (the same text
as [launcher/LICENSE](launcher/LICENSE)), and [engine/LICENSE](engine/LICENSE) for the engine, kept unchanged from the
upstream project, whose copyright notice it carries ([engine/NOTICE](engine/NOTICE)). Third-party components keep their own
licenses: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The licenses grant nothing for any game.

## Sources and credits

**The upstream.** The engine is a fork of [totk-pc-recomp](https://github.com/valentderah/totk-pc-recomp) by
Valiantsin Dzerakh (MIT), a static recompilation of *Tears of the Kingdom*, imported at its version 0.3.13 with its
changes up to 0.4.0 taken in since ([engine/NOTICE](engine/NOTICE)). From it come the recompiler, the runtime, the
engine's tools and most of its documentation. Added here: the *Splatoon 3* target, LAN play and its relay client,
automatic matches, the save profile, the launcher, and the protocol and server of the server repository. What exactly,
in the engine's [README](engine/README.md#sources-and-credits).

**Code and tools from other projects**, each with what was taken, its commit and its files in the notices:
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), [engine/THIRD_PARTY_NOTICES.md](engine/THIRD_PARTY_NOTICES.md),
[launcher/THIRD_PARTY_NOTICES.md](launcher/THIRD_PARTY_NOTICES.md). In short:

| Part | Projects (license) | Role |
|---|---|---|
| engine | LAS Recomp, MacNativeGames, d2r-mobile, nxnative, SavageWindRecomp (MIT) | the recompiler, the CPU headers and much of the runtime the upstream started from |
| engine | Ryujinx's shader translator (MIT), deko3d (zlib) | the game's GPU shaders to SPIR-V; the texture layout |
| engine | Dear ImGui, AMD FSR 1, Vulkan Memory Allocator (MIT), SDL 3 (zlib), libopus, libvpx, Zstandard (BSD-3-Clause) | the in-game menu, upscaling, GPU memory, gamepads, the game's audio, movies and compressed data |
| engine, launcher | hactool (ISC) with mbedtls (Apache-2.0) | decrypting the player's own dump with the player's own keys; the launcher ships it prebuilt with the shader translator |
| launcher | Tauri 2 (Apache-2.0 OR MIT), React (MIT), rustls, reqwest, tokio and other crates (permissive) | the application, its interface, HTTPS to community servers |
| launcher | ruzstd, tegra_swizzle (MIT), texture2ddecoder (MIT OR Apache-2.0) | reading names and icons from the player's own game files at run time |
| launcher | minisign-verify (MIT); minisign (MIT) in tests and release tools only | checking the signature of the launcher's own updates |
| launcher | Archivo, Rubik, JetBrains Mono (SIL OFL 1.1) | the interface fonts, bundled |

Every Rust crate and npm package of the launcher, with its license:
[docs/third-party-dependencies.md](docs/third-party-dependencies.md). The install wizard downloads the toolchain that
builds the game on the player's PC, each file pinned by SHA-256 and none of it in this repository: embeddable Python
with five wheels (numpy, lz4, cryptography, cffi, pycparser), llvm-mingw, Ninja, SDL 3, and MSYS2 packages of Opus,
libvpx, the Vulkan headers and loader library, Vulkan Memory Allocator and zstd
([launcher/THIRD_PARTY_NOTICES.md](launcher/THIRD_PARTY_NOTICES.md)).

**Formats and specifications the work relies on:** the cheat files of Atmosphère and the Switch emulators' mod layout
([engine/docs/cheats.md](engine/docs/cheats.md), [engine/docs/mods.md](engine/docs/mods.md)); the GPU notes of
[engine/docs/maxwell.md](engine/docs/maxwell.md), with their sources; Discord's documented local IPC
([launcher/docs/discord-presence.md](launcher/docs/discord-presence.md)); minisign signatures
([launcher/docs/releasing.md](launcher/docs/releasing.md)); OAuth 2.0 with PKCE (RFC 7636) and loopback redirects
for native apps (RFC 8252) in the community client; Semantic Versioning for release versions; the Kalamar protocol,
specified in the server repository; the
Developer Certificate of Origin 1.1 ([CONTRIBUTING.md](CONTRIBUTING.md#sign-off)).

**No game content.** No code, file or data of the game or of its publisher is in this repository or in the launcher:
the player's own copy provides them, on the player's PC ([docs/LEGAL-NOTES.md](docs/LEGAL-NOTES.md)). Kalamar is not
affiliated with, endorsed by or connected to Nintendo or any game publisher. *Splatoon*, *The Legend of Zelda*,
*Tears of the Kingdom* and *Nintendo Switch* are trademarks of Nintendo. They are named here only to say which game the
software works with.
