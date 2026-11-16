# Getting started

Install the launcher, let it build the game from your own files, play offline. No account and no server are needed.

## What you need

- Windows 10 or 11, x64 (not ARM), with a CPU that has AVX2 and FMA (x86-64-v3): the install wizard checks it.
- A graphics card with Vulkan 1.3 and `VK_EXT_external_memory_host`. Tested mainly on NVIDIA cards; **AMD cards are
  not verified** with this game (a crash at load and graphical defects were reported on one).
- About 15 GB free on the install drive for a first install, about 13 GB used afterwards (the wizard checks the
  space before every step).
- Your own files, dumped from your own console: the game, its **11.3.0** update, and your console's keys file. They are
  only read; the wizard never copies, uploads or modifies them.

## 1. Install and build

1. Install the launcher: download `Kalamar.exe` or the installer from the
   [releases page](https://github.com/CyriacFR/Kalamar/releases). To build it from source instead, see
   [CONTRIBUTING.md](../CONTRIBUTING.md#building-and-testing-the-launcher) (`npm run tauri build` in `launcher/`).
2. At first start the install wizard takes the whole window: **Welcome → Your files → What will be installed →
   Install → Done**.
3. *Your files*: pick the game, the update and the keys file, and an install folder. The wizard checks the update's
   version before any long step (the SHA-256 of its main executable against the engine's target table) and stops
   early on a wrong file.
4. *What will be installed* lists the build tools it will download into the install folder (a portable Python, the
   llvm-mingw compiler, Ninja, SDL 3 and a few libraries: about 230 MB) with each download source. Nothing is
   installed on the system: no administrator prompt, no PATH or registry change. Nothing is downloaded before you
   tick the confirmation, and nothing of the game is downloaded.
5. *Install* runs, without console windows: extraction of your files, the engine's code generator, the compile, a
   self test (about a quarter of an hour in all on an 8-core CPU). It can be canceled and resumed; every step writes
   its log under `<install>\logs\`.

The steps, what they write and how uninstalling works are described in
[launcher/docs/install-wizard.md](../launcher/docs/install-wizard.md).

## 2. Play offline

The **Play** screen's big button starts the full game, offline (Enter works too). Two more ways to start are
offered as buttons once a community server is added, and always from the command line
(`Kalamar.exe --play training`, `--play roam`, `--play full`):

| Button | Starts |
|---|---|
| Full game | the game as it starts normally, offline |
| Training | the shooting range, directly |
| Free roam | the hub, free to walk around, directly |

The **Locker** sets your name, character, gear,
weapon and name card; they are applied to your save when the game starts (a backup of the save is made first,
[engine/docs/save-profile.md](../engine/docs/save-profile.md)). Names and icons in the Locker are read from your own
game files at run time; none are shipped.

In the Gear tab, under the piece you chose, a slot holds its **main ability** and three slots its **sub-abilities**.
The main slot shows the piece's own ability until you pick another one; it offers the main abilities that pieces of that
kind have in the game (some are found only on headgear, clothing or shoes), and *Piece's own main ability* gives the
original back. For a sub slot, pick a slot, then an ability (or *Empty*; tick *Fill all three with this ability* to use
it in every slot); *Clear* empties the three. Only some of the game's abilities fit a sub slot, and a piece with
sub-abilities shows with 2 stars in the game. A server room set to "main abilities only" ignores the sub-abilities but
keeps the main ability you chose. The option *Full stock of ability chunks* (off by default) gives you 999 chunks of
each ability that fits a sub slot. Each appears only when your game is recent enough; otherwise the Locker says a game
update is needed.

Offline means offline: the game opens no network socket. Manual LAN play on one network (or through a relay you name)
is in the Play screen's *Advanced* drawer; see [engine/docs/lan-play.md](../engine/docs/lan-play.md).

## 3. Settings

- **In the launcher**: *Settings* edits the game's settings file (`kalamar.ini`) from a schema, plus the launcher's own
  choices (minimize while playing, language).
- **In the game**: the settings menu (F1 or Esc; five tabs on this game: aim and controls, display, audio, game,
  advanced) and the performance overlay (F10). Some settings of the engine's upstream game have no effect here;
  [engine/docs/settings.md](../engine/docs/settings.md), "The Esc menu on the Splatoon 3 target", says which and why.
- **References**: every player setting in [engine/kalamar.ini.example](../engine/kalamar.ini.example); developer
  settings, diagnostics and test hooks in [engine/docs/settings.md](../engine/docs/settings.md); keyboard, mouse (as the
  gyro) and gamepads in [engine/docs/controls.md](../engine/docs/controls.md).

## 4. Where things are

| What | Where |
|---|---|
| The built game, your saves, logs, caches | `<install>\work\splatoon3-11.3.0\` (`userdata\` holds saves, logs and caches) |
| The launcher's data (profile, servers, settings of the launcher) | `%APPDATA%\dev.kalamar.launcher\` (a pre-release launcher's `dev.splat3recomp.launcher` folder is moved there at the first start) |
| The engine source the wizard compiled | `<install>\src\` |

Everything in the install folder except your saves and settings is derived from your game: keep it on your machines.

## When something goes wrong

- **A wizard step fails**: the screen shows a sentence, the last lines of that step's log and advice; **Try again**
  resumes at that step, and closing the launcher in the middle is safe (the next start offers to resume). Each step's
  log is in `<install>\logs\`.
- **"Wrong update" or a version error**: the wizard recognizes the update by the hash of its program; only the
  **11.3.0** update is supported. The 11.2.0 update, which earlier versions of Kalamar used, is named as such and
  refused.
- **"Game update needed" on the Play page**: your game was built for 11.2.0. It still starts for offline play; online
  play needs 11.3.0. Click **Update the game**: the wizard asks for the 11.3.0 update file (the base game and the
  keys file again only if it cannot find them), builds 11.3.0 next to your game, copies your saves and profile into
  it, and deletes the 11.2.0 folder (about 14 GB) only after the new game has passed its checks. If the update
  fails, nothing is deleted and your 11.2.0 game stays as it was.
- **Not enough space**: the wizard checks before every step and says how much it needs; free space or choose another
  drive.
- **The game does not start or crashes**: update your graphics driver first. The launcher's simple mode offers
  **Copy the diagnostic** (the details of the last launch, with your Windows user name replaced); the game's logs are
  in `userdata\logs\` of the game folder (table above). AMD cards are not verified with this game.
- **Graphical defects**: the game's settings menu (F1 or Esc) has an *advanced* tab; known issues are tracked in the
  repository's issues.

To report a problem, use the bug report form of the repository's issues: it says what to attach. Read the logs
before you attach them and remove what is personal (your user name in paths, IP addresses, other players' names).
Never attach game files, keys or saves. More: [SUPPORT.md](../SUPPORT.md).

## Next

- [Adding a community server](servers.md), for online matches with others.
- [Writing your own server](write-a-server.md).
- Building by hand without the launcher: [engine/docs/building.md](../engine/docs/building.md).
