# Interface and navigation

What the launcher's window shows, in which mode, and how to look at every screen without a game.

## Design system

The product name lives in `src/brand.ts` (plus `productName` in `tauri.conf.json` and `PREFIX` in `matchcode.rs` for
the match code). The styles are in `src/styles/`:

- `fonts.css`: the only place that names the fonts (`--font-body`, `--font-display`, `--font-mono`);
- `design.css`: tokens and components;
- `screens.css`, `stages.css`, `wizard.css`: the screens;
- `ink.css`: the playful ink skin (palette, pill shapes, drips, hatch and dot textures, squash-and-stretch hover),
  all drawn there.

Fonts are bundled from npm (`@fontsource-variable/archivo`, `@fontsource/jetbrains-mono`, SIL OFL), no CDN. No
image, font or sound of the game is shipped.

## Two interface modes

The **simple mode** (default) is for players: no log, path, environment variable, PID, raw exit code, launch report,
extra argument, second player, manual LAN or identifier. States are sentences, problems say what to do and offer
"Copy the diagnostic" (the technical details, with the Windows user name replaced).

The **advanced mode** (Settings > Advanced mode, `ui.advanced` in `config.json`) shows all of it.

## Navigation

The rail holds Play, Locker, Community (only once a server is added) and Settings, and the player card.

### Locker

Tabs for the character, gear, weapon and splashtag, a side card with the name and the "At launch" options. Under the
chosen piece of the Gear tab, three slot buttons show its sub-abilities (icon and name, or "Empty"); a slot opens a
picker with every ability a sub slot can hold, "Empty" and the "Fill all three with this ability" shortcut, and "Clear"
empties the three. The editor and the "Full stock of ability chunks" option appear only when the installed game applies
them, else one line asks for the game update ([game-data.md](game-data.md), "Sub-abilities and ability chunks").
`npm run dev:mock` shows it (`?gear=old`: the update line, `?gear=none`: nothing).

### Play

One main block: the state in one sentence and one button (Install, Play, or Back to the game + Stop; a game update
with its duration).

- **Without a server**, the main block is the full offline game, with two cards under it: what this launcher is (two
  sentences, four points behind "Learn more") and "Play with others" (its button, "Add a server", opens the dialog).
- **With a server**, the main block is the server in use (`src/pages/play/Online.tsx`): its name, its own description
  (two lines at most), up to four pills from the areas it declares (in the words of its Community tabs), the player's
  state there (logged in as X, not logged in, or, once the Community page or the background connection has the live
  state, a room, a search, a found or running match) and one button (Play online, Log in, Back to the room, See the
  search, See the match), with the player count beside it when the server publishes one ("12 online · 4 playing",
  "2 searching" only above zero). A game not installed, installing or running keeps the server's name and says that
  state instead. The other servers are small cards under it (one click makes one the server in use, the `selected`
  of `online\servers.json`, as in Settings), then the solo block: Full game, Training, Free roam, and the note on
  what this launcher is.

In the advanced mode, the advanced tools come below: last launch, launch options, LAN, profile report, second local
player.

### Community

The page's title is the server in use (its name opens the list, "Add a server", and "Manage", which is
Settings > Servers), with its player count beside it. Changing the server in use is refused, with one sentence,
during a room, a search or a match (`src/servers/switch.ts`, used by the Play screen, the Community page and
Settings). A relay-only server's Community page is the private match by code. What the page does with a server is
described in [community-client.md](community-client.md).

### Update prompts (every page)

A bar at the top of the content area (`src/update/UpdateBar.tsx`, rules in `prompt.ts`) shows the launcher's own
update on every page, in every state (available, downloading, ready, waiting for a game or an installation, failed,
result), then, when there is none, the game's update ("Game update available" / "Game update needed") on every page but
Play, which carries it in full. A dialog asks once per run of the launcher, when it is idle. "Later" lasts until the
next start and leaves a dot on the Settings entry. During a match the bar is one compact line and no dialog opens. The
rules and the texts are in [releasing.md](releasing.md), section 1.

### Settings

Mouse and aim, keyboard and controller, display, language, servers, installed game, advanced mode, about; local
network and save backups in the advanced mode.

## Match screens

During a match the launcher follows the game through the status file of
[auto-match.md](../../engine/docs/auto-match.md) (present and expected players, error, reason) and maps the engine's
error codes to messages and advice.

When a server drafts the stage of a match (`drafting`), a full-frame screen (`src/community/Draft.tsx`, its rules in
`draftState.ts`) shows both teams, the stages as the game shows them (banned ones in the banning team's color, the
chosen one in front), whose turn it is with a countdown ring, an action only for the player who may act, the votes of
the player's team and the turn log. The launcher declares the feature (`launcher.draft` in `PUT /v1/me/game`). A
light sound plays when the player's turn comes; the screen can be reduced and opened again from the match card.

## Browser preview

`npm run dev:mock` serves the interface on made-up data (`src/dev/mock/`, never in the production build). The URL
chooses the situation, for example:

- `?game=crash`, `&adv=1&lang=es`, `&pop=0`, `&stages=none`;
- `&server=service&page=community&match=run`, `&server=multi`, `&server=service&bg=1&room=host`;
- the stage draft: `&match=draft&ds=you|them|vote|timeout|random|done|mode|auto` (`mock/draft.ts`);
- an install at a given point: `&install=running&ist=early|merge|compile|link|waitgame` (`&iupd=1`: an update of a
  playable game; `&tasks=some|all`, `&profile=empty`);
- a game update: `&upd=short|long|full` (`&atclose=1`), on the update bar of the other pages;
- the launcher's own update: `&lup=available|downloading|ready|blocked|relay|later|…` and `&dlg=0` (without it the start
  dialog opens after 1.5 s), see [releasing.md](releasing.md), section 6;
- `?gallery=stages`: the stage cards in every state, on made-up pictures drawn as gradients.

`backend.ts` lists them all. `npm run shots -- --out <folder> --lang fr,es,ja` captures every screen of
`scripts/ui-shots.list.mjs` with the installed Edge, in both modes and two widths.
