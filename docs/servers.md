# Adding a community server

Without a server, Kalamar is an offline tool: the Play screen starts the full game, offline, and shows one card
*Play with others*, whose button *Add a server* opens the dialog described below. Online play — private rooms, queues, friends, rankings, tournaments — is
provided only by **independent community servers**, run by whoever chooses to, for their own community. No server
is built in, none is suggested, and none is run by the game's publisher.

A server shows only what its operator declared: one server may offer 1v1 duels only, another 4v4 queues with ratings,
another a weekly tournament. The launcher draws what the server describes (as data, never code from the server).

## Adding one

In the launcher: *Play with others* › *Add a server* (later: the Community page's title, or Settings › Servers), then type the line the operator gave you:

```
play.example.org          the usual form: a host name (HTTPS on port 443)
play.example.org:8443     another HTTPS port
```

The port, when there is one, is the HTTPS port of the server's web address, never the game's UDP port: the relay that
carries the game traffic is announced by the server itself. A pasted `https://` prefix is tolerated. IPv6 addresses
are refused (the game is IPv4 only). Names are preferred to bare addresses; a server behind a reverse proxy is fine.

The launcher then:

1. fetches `https://play.example.org/.well-known/kalamar-server.json` (at most 64 KiB, 10 s, at most 3 redirects,
   never to plain `http`; certificates are checked with the system's store, and never turned off), then the
   service's `GET /v1/meta`;
2. shows you the server's **name, operator and contact, region, languages, the areas it offers, its rules, its
   privacy notice link**, and the API's host when it differs from the one you typed;
3. adds it **only when you confirm**.

A server is refused when it shares no version of the protocol with the launcher, no match contract the launcher
implements, or no login the launcher can use. When no service answers, the line can still be added, after you
confirm, as a **relay only**: private matches by code, no accounts.

Plain `http` is accepted only for a test server on this computer (`127.0.0.1`, `localhost`, `[::1]`).

Removing the server (or choosing another one) logs out and stops every connection to it.

## What the launcher sends, and when

The launcher has no telemetry. Apart from the check for its own updates (published releases only: GitHub's release
API 20 s after each start and every 6 hours while the launcher stays open, can be turned off in Settings › About; nothing about you or your servers is sent), its
network accesses with servers added are only these:

| When | To | What |
|---|---|---|
| You add the server | the server's web address | the two requests above (the server sees your IP address, as any website does) |
| You open its **Community** page, or the launcher starts while you ticked *Stay connected in the background* for it (off by default) | the server's API | login, one event connection (WebSocket), the requests of the page you look at |
| You log in | the server's API; with an OAuth provider (for example Discord), your browser and that provider | a `direct` login sends the fields the server asks for (usually a display name of at most 10 characters); an OAuth login goes through your system browser, with PKCE and a redirect to this computer: the launcher never sees your provider password, the server receives an identifier from the provider |
| After login | the server's API | a description of your game engine (`PUT /v1/me/game`): the match contract versions, a build label that is the same for every player of the same build (the launcher's version, the engine snapshot and the game version), the mode keys and stage identifiers your copy offers, the engine's capability list as the engine prints it (`--list-match-options`, with mode and stage labels in your language read from your own game files) |
| A match starts | the server's API | that the game was launched; every change of the game's status file (state, players present, room settings); then the game's **result file** (member names, teams, points, per-player statistics, weapon and gear identifiers) or an error report |
| A match starts | the relay the server named | the game's own LAN traffic, over UDP; the relay sees your IP address and forwards the traffic only to the other players of that match, who see a virtual address, not yours |
| A match starts; you press *Check the name*; a match code naming a host is shown | your system's DNS resolver | one lookup (IPv4 answers only) of the relay's host name; the game itself never resolves names |

What it **never** sends: your game files, your save, your keys, your Windows user name, files from your disk, a
hardware inventory, an e-mail address, a password of another service. The session it keeps for a server (access and
refresh tokens) is stored in the launcher's data folder encrypted for your Windows account (DPAPI), never shown or
logged; logging out deletes it.

Links a server shows open only when you click them, in your browser; the launcher never loads an image or a page from
a server by itself.

## Privacy

Each server is run by its operator, who decides what it stores and is responsible for it. Before you confirm, the
launcher shows the operator and the link to their privacy notice: read it. The protocol asks servers to store as
little as possible (a display name, a keyed hash of your login identity, your matches, ratings and moderation data),
never to require an e-mail address or a real name, and never to log tokens or result payloads.

Every conformant server must let you **export** everything it holds about you and **erase** your account
(`GET /v1/me/export`, `DELETE /v1/me` of the protocol). The launcher offers both in the server's **Account** tab,
under "Your data": "Save a copy of my data" writes the server's answer to a JSON file you choose, and "Erase my
account" deletes the account on the server after you type your display name to confirm. Erasing cannot be undone; in
other players' matches you then appear as a deleted player, and the server's privacy notice says what it keeps while a
ban or a queue penalty runs.

Results are reported by the players' own games; a server compares the reports of a match but cannot see the match
itself. A server's rules page should say what that means for its rankings.

The launcher's Discord presence (off by default, Settings › Discord) never shows a server's address, the other
players' names or a room code; the server's name only when you tick "Show the community server's name".

## For operators

How to run a server: the server repository (`kalamar-server`): its README, `docs/operator-first-steps.md`, and
`PRIVACY-TEMPLATE.md`. How to write your own: [write-a-server.md](write-a-server.md).
