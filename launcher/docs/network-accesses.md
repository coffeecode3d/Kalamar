# Network accesses of the launcher

What the launcher protects players from, and the risks that remain online, are in
[security-model.md](security-model.md).

The launcher sends no telemetry. Its network accesses, all of them:

0. The launcher's own update check, only in a build that names an update repository (`KALAMAR_UPDATE_REPO`, set by
   `scripts/make-release.ps1`; a build without it has no update feature at all): 20 s after every
   start and then every 6 hours while the launcher stays open, if "Check automatically" is on (Settings › About, on by
   default), and on "Check for updates": GitHub's public
   release API (`api.github.com`, no token or identifier, an ETag), then, for a newer release only, its signed manifest
   and signature from `github.com` (redirected only to GitHub's release-asset hosts). The program itself is downloaded
   only when the player clicks "Update". GitHub sees the IP address. See [releasing.md](releasing.md).
1. With no community server added: nothing else, the launcher is otherwise offline (the game is played locally;
   same-network play needs no server).
2. For a server the player added: one DNS lookup (system resolver, IPv4 answers only, 6 s timeout) of that server's relay
   host name, at each private match launch, on the "Check the name" button and when a match code naming a host is shown;
   the result ("name → address") is shown to the player and only the last address is kept, for display.
3. For a server with a community service (Kalamar protocol 1.1, [community-client.md](community-client.md)): when the player adds it, its discovery document
   and `GET /v1/meta`; then nothing until the player opens its **Community** page (or, if the player ticked "stay
   connected in the background" for it, from the launcher's start), from which the launcher talks to that service only
   (HTTPS, or plain http to this computer for a test server): login, the event WebSocket, the requests of the page (the
   export and the erasure of the account among them, each on the player's click), and the reports of the matches it
   plays. Links of the server open in the browser only on a click; no image or URL is
   fetched by itself. Logging out, choosing another server or removing it stops it. The Play page shows such a server
   from this computer only (`proto_peek`: the saved login's name, the description and areas of the last meta read,
   kept in `online\<id>\meta-summary.json`), with one exception: a server that declares the `population` area gets
   `GET /v1/population` (no login sent, at most 4 KiB, 6 s; `proto/population.rs`) when the Play page or its Community
   page opens, then every 30 s while one of them stays on screen, never while the window is hidden or the game runs;
   a failure shows nothing. And for a server that lists **several relays** (`meta.relays`, protocol 1.1.9), once the
   player is logged in to it: a lookup of each relay's host name (the system resolver) and one UDP datagram to each
   relay address (the relay protocol's `HELLO`, 64 bytes from a fresh socket, answered by a `CHALLENGE`; it joins
   nothing) to measure the latency from this computer, so each of those relays sees the player's IP address. When:
   after a login, at each (re)connection of the event stream, and on `meta.changed` (the meta is read again after a
   random delay of up to 5 s, once per revision), in the background (nothing waits for it), never while the game runs;
   logging out, choosing another server or removing it cancels it. Limits (`proto/relayping.rs`): one `HELLO` per
   address (IPv4 + port) per 10 minutes whatever server, id or list names it, one lookup per name per 10 minutes, and
   at most 16 `HELLO`s and 16 lookups per server per sliding 10 minutes, one measurement at a time per server.
   Never measured: ports below 1024, link-local, multicast, broadcast (including the subnets of this computer),
   loopback unless the server itself is on loopback, private ranges (10/8, 172.16/12, 192.168/16, 100.64/10) unless the
   server's own address is in one. The values (`null` for any failure) go to that server with the game's capabilities
   (`launcher.relay_rtt_ms` in `PUT /v1/me/game`, 2 s after the datagrams whatever answered) so it can give a match the
   relay that suits its players, and the Account tab shows them. A server with one relay or none is never measured.
4. The install wizard, listed separately: build tools it starts download from the sources listed on its confirmation
   screen, only after the player ticks the confirmation.

Not a network access of the launcher, listed for what it makes visible: the **Discord presence**, only in a build that
names a Discord application (`KALAMAR_DISCORD_CLIENT_ID`) and only once the player switches it on (Settings › Discord,
off by default). The launcher then gives the Discord application of this PC, through its local pipe, what the player
does at the chosen level (in the launcher, in game, a room, a search, a match, the stands; on request the mode and stage
names of the player's own game data and the server's saved name), and Discord shows it to the player's contacts. Never
another player's name, a room code or a server's address. Details: [discord-presence.md](discord-presence.md).
