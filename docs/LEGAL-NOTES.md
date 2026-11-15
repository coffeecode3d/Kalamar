# Legal notes

Factual notes for readers. **This is not legal advice**, and it does not say what the law of any country allows. It
describes what the software contains and does.

## 1. What the repository contains, and what it does not

- **The project's own code** (launcher, engine changes, documentation), under the MIT License: `LICENSE` and
  `launcher/LICENSE` ("The Kalamar contributors"), `engine/LICENSE` for the engine (below).
- **Third-party code** under permissive licenses, listed with its source and license in
  [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) and the files it links to.
- **No content of any game**: no program code, data, images, sounds, fonts, text or keys of the supported game, and
  no executable built from it. A check run on every change (`scripts/check-content.py`) refuses game file formats
  and folders, files carrying the code generator's marker, images or binaries not reviewed one by one, and keys in
  console key-file format.

The software does contain **facts about the game's program**, needed to run the player's copy: addresses and offsets
of functions in the game's code (in comments, logs and patch tables such as `engine/recomp/patches-*.txt`), the SHA-256
of the executables a target accepts (to refuse a wrong file), the names of the system functions the game imports
(the runtime answers them), the publisher's domain name in one test that checks the runtime never looks it up, the
32-bit constant the console applies to its shared font files (publicly documented, used to present a stand-in font),
and a few internal stage scene names cited as examples in the engine's documentation.

## 2. What the software does with the player's files

- The install wizard asks for the player's own dump of the game and of its update, and the player's own console keys
  file. Tools on the player's machine read them to decrypt the game's containers (the engine's extraction scripts,
  derived from MacNativeGames, and hactool), merge the update into one RomFS image, and recompile the game's code
  into a native executable. The keys are never shipped, displayed, logged or sent; key values are not read into the
  launcher.
- The result contains the game and stays on the player's machine; the documentation tells players not to share it.
- The launcher's profile feature changes the player's **save file**, which the game stores encrypted. The runtime
  decrypts it, applies the profile and encrypts it again in the game's own format (a backup is made first;
  `engine/docs/save-profile.md`, "The save file"), and a helper script (`launcher/kit-tools/profile/s3save.py`) can
  do the same outside the game. The table the save keys are derived from is never stored in the repository: the
  runtime reads it from the loaded game, the script from the player's own executable at install time.

Whether decrypting one's own dump with one's own keys, recompiling it for personal use, or editing one's own save is
allowed depends on the country; this project gives no opinion on it.

## 3. Names and trademarks

*Splatoon*, *The Legend of Zelda*, *Tears of the Kingdom* and *Nintendo Switch* are trademarks of Nintendo. The
documentation names them to say which game the software works with, and the engine's history (a fork of a
*Tears of the Kingdom* recompilation) explains the others. The project's name, logo and interface are its own.

Some **technical identifiers** contain such names: `totk.ini` (still read where there is no `kalamar.ini`),
`totk.exe`, `totk_xlate.exe`, `splat3.exe`, the engine's target and folder names (`splatoon3-v1130`,
`splatoon3-11.3.0`; `splatoon3-v1120` and `splatoon3-11.2.0` for the version before), the engine's `amiibo` module and the game-data identifiers exchanged with servers (mode keys, style
names, stage scene names). Renaming those breaks interfaces (the server protocol, existing installs); it has not been
done. The environment variables are documented under the project's prefix, `KLM_*` (the older `TOTK_*` and `SPLAT3_*`
names are still read), and the launcher's application identifier is `dev.kalamar.launcher` (the launcher moves a pre-release
launcher's `dev.splat3recomp.launcher` data folder by itself at its first start). The interface shows a few names of the game's own features (character species, the
name card) where the player edits them in their own copy.

## 4. Network and online play

- The game never contacts the publisher's servers; without LAN play or a community server it opens no network
  socket. Multiplayer goes through the game's own **local network mode**, tunnelled by a UDP relay.
- Community servers are **third-party services**: neither this repository nor the launcher ships, names or suggests
  one. Their operators are responsible for their community, moderation, privacy notice and legal compliance (the
  server repository has a privacy notice template).
- No money is involved: nothing is sold, no donations are asked, and the server protocol has no payment, shop or
  currency.

## 5. Licenses

- The engine is a fork of **totk-pc-recomp** (MIT, Valiantsin Dzerakh); its `LICENSE` is kept unchanged and applies to
  the imported code and to the fork's changes ([engine/NOTICE](../engine/NOTICE)).
- The shader translator is **Ryujinx**'s (MIT), vendored unmodified with MacNativeGames' patches; Dear ImGui (MIT),
  AMD FidelityFX FSR 1 (MIT), the fonts (JetBrains Mono and the launcher's fonts under the SIL OFL 1.1, Roboto under
  Apache-2.0) keep their license files.
- **No GPL code** is copied. The engine's rule is that GPL projects (yuzu, suyu and others) may be read to understand
  a behavior, never copied; the documentation cites them by behavior. Two facts to know: the upstream's
  ultrawide tool for its own game (`engine/tools/ultrawide_layout.py`) takes numeric values (pane names and
  distances) from a GPL-licensed mod (data, no code; documented in the engine's notices), and developer builds made
  with MSYS2 link the GCC runtime (GPL with the runtime library exception) and copy an unmodified LGPL `libiconv`
  DLL.
- Every third-party license of the launcher's dependencies is permissive (MIT, Apache-2.0, ISC, BSD, Zlib,
  Unicode-3.0, MPL-2.0 for four unmodified crates).

## 6. Authorship

Most of the code and documentation is written by AI coding agents (Claude Code) under the maintainer's direction.
