# Security model

What Kalamar protects you from, what it cannot, and what you should know before you play with people and servers you
do not know. Written for players; the technical references are in brackets for contributors. To report a
vulnerability (privately): [SECURITY.md](../../SECURITY.md). The reference server and its relay have their own
security model, in the server repository (`docs/security-model.md`).

## What the launcher does for you

**A server cannot start your game on its own.** The launcher starts a community match by itself only when you joined
it from this launcher since it started: you searched that queue, accepted that match, created, joined or got ready in
that room, acted in that activity, took a place in its stands, or played the previous game of its series. Anything
else asks you first (see "Starting a match" below). Even for matches you joined, a server that starts your game more
than 6 times in 10 minutes or 20 times in an hour gets the same question. (`src-tauri/src/proto/consent.rs`,
`runner.rs`)

**Your game only talks to relays the server announced.** A match must name a relay the server lists in its public
description (or the relay saved when you added the server); any other relay is refused and the game is not started.
A match code from another player cannot point your game at your own network (no broadcast, link-local, multicast or
system-port address), and a relay this launcher does not know is shown to you and used only once you tick that you
trust it. (`runner.rs`, `commands.rs` `match_start`)

**The launcher does not jump in front of you.** It comes to the front for a found match or its own question at most
once per match and once per 20 seconds, and never while a game is running. (`proto/glue.rs` `FrontOnce`)

**Servers learn your game build, not your computer.** The launcher tells a server which launcher, runtime and game
versions you have (`kalamar/<version> runtime/<engine snapshot> game/11.3.0`), the same text for everyone with the same build; nothing
in it tells your installation apart. (`proto/engine.rs` `build_label`)

**Names cannot be disguised.** Server names, player names and the other text a server sends lose the invisible
characters that reorder or hide text (right-to-left overrides and the like); names are displayed isolated from the
launcher's own text. (`util::is_hidden_format`, `src/community/cleanText.ts`)

**Game files cannot crash the launcher.** Everything read from the game's files (textures, tables, archives,
compressed files, the update package) has size and work limits; a malformed file is an error, not a crash, and a
damaged cache is rebuilt. (`src-tauri/src/game/`, `installer/nsp.rs`)

**Nothing planted next to the launcher runs.** Windows programs (the browser opener, PowerShell, Explorer) are started
by their full path, the launcher's DLLs come from Windows' own folder, and only the tools built into the launcher are
installed, never a copy found next to it. (`winutil.rs`, `build.rs`, `installer/prebuilt.rs`)

**Updates run exactly what was checked.** A launcher update is verified (signature, size, SHA-256) and the file that
starts is the one verified, held so that nothing can change it in between. The release signing key is only ever read
by the official minisign tool, never by a program built from this repository. (`update/apply.rs`,
[releasing.md](releasing.md))

## Starting a match

| You did, in this launcher since it started | The game starts |
|---|---|
| searched a queue, then accepted the match found | by itself |
| created or joined a room (the host starts it) | by itself |
| took a place in the stands of a match or series | by itself |
| registered in or acted on an activity (a cup, a best of 3) | by itself, and the next games of its series too |
| nothing, or the launcher restarted since (a match "in progress" found at start-up) | after your yes |
| any of the above, but the server already started 6 games in 10 minutes or 20 in an hour | after your yes |
| any of the above, but the match names a relay the server never announced | never |

The question shows the server, the relay and the players, and waits until the match's launch deadline (2 minutes at
most). No answer counts as no: the game is not started and the server is told the launch was canceled.

## Risks that remain

These are known and not fixed by the launcher. Most come from the game itself or from how community play works.

1. **The game's own network code is the original game's, recompiled.** It is not memory-safe and not sandboxed. What
   other players, the relay or a LAN host send reaches it after only size, source and destination checks, so a bug
   there could let a malicious player run code on your PC as your Windows user. Play with people and servers you
   trust, and prefer relay mode to LAN mode on networks you do not control.
2. **A server's operator is trusted.** It chooses your relay (among those it announces), can start matches for the
   queues, rooms and activities you joined, sees your IP address, names, room codes and results, and its relay sees
   your IP address. A malicious or hacked server can show any text and links: check the address bar of your browser
   before you type a password on a page it opened.
3. **Results are reported by the players' launchers** and decided by the server's rules. Collusion and second
   accounts are possible, especially on servers with guest login and open stands.
4. **The relay protocol is not encrypted or authenticated.** The room code is its only key. Someone who can see your
   network traffic (the same Wi-Fi, a hacked router) can read it, join the room and pretend to be a member. Share room
   and match codes privately, like passwords.
5. **A match code from another player chooses your relay** (and, for a named relay, a host name your launcher looks
   up). The launcher refuses impossible addresses and asks you about relays it does not know, but the relay you
   accept sees your IP address and your nickname.
6. **Game files and mods are untrusted input.** The launcher's own parsers refuse a crafted "ready-made dump" or mod without
   crashing, but the original game code still reads it. Use your own dump.
7. **Logs and diagnostics** hold relay addresses, names and technical details. The launcher's "copy the diagnostic"
   hides your Windows account name and room codes; the raw log files do not. Review before you post them.
8. **Shared PCs.** Install inside your Windows profile: another account can change an install folder elsewhere (the
   install wizard warns you). The game's own DLLs (SDL3, Opus, the Vulkan loader) are found with Windows' default
   search order, so keep the game folder writable only by you.
9. **Opt-in third-party features.** The Discord presence talks to the Discord application on your PC; the DLAA option
   loads NVIDIA's NGX library from your graphics driver, whose network behavior has not been checked.
10. **For server operators:** run the service behind the shipped proxy, give the relay its room secret (the reference
    relay refuses to start without one unless told `--open`), and know that the relay logs player IP addresses and
    names. Per-address limits count an IPv4 address or an IPv6 /64 as one client: players behind one box share them
    (the relay admits 8 members of a room per address by default, a whole 4 v 4; raise `--max-members-per-source` for
    a larger LAN party).

Also inherent to the game: player names in a relay room are not verified by the game (a name held by two members
seats nobody, but an impostor who takes your name before you join gets your seat; only the server or relay can
guarantee unique names), and match settings and results come from the room's host game, which a modified host can
falsify.

## Accepted limitations

Known, kept as they are, and why:

- **A server's login page may be on any https host.** Logins go through your browser; the launcher cannot know which
  provider a server uses. Check the browser's address bar (risk 2).
- **Updates have no expiry date and releases are not Authenticode-signed.** Every update is signed with minisign and
  can never go back to an older version; someone who takes over the release repository can only withhold updates.
  Authenticode needs a paid certificate.
- **No overall time limit on an update download.** Each request has its own time limits and the size is capped; a
  stalled download is canceled by the player.
- **Fallback build paths not pinned by SHA-256** (the translator built from source, the hactool build script, the
  MSYS2 path): only used when the tools built into the launcher cannot be, and they download from the sources shown
  and accepted on the install screen.
- **The uninstaller can follow a junction placed inside the install folder.** Placing one needs write access to that
  folder, the same access that already lets someone change the programs the launcher runs from it (risk 8).
- **The game's process id is in the status mirrored to the server.** Harmless.
- **The Discord pipe of the launcher and of the game uses the "identification" level.** A program pretending to be
  Discord learns what the presence says and can hide it, nothing more; reads are bounded.
