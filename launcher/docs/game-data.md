# Game data and the player profile

How the launcher reads names and icons from the player's own game files, and how the Locker's profile reaches the game.

## Names and icons come from your own files, at run time

The Locker shows the game's own names and icons. They are read from **your** RomFS image
(`<game folder>\romfs\romfs.bin` + `index.json`) when the page opens and are never shipped, bundled or committed:

- `game/romfs.rs` random access through `index.json` (no extraction) + zstd (`ruzstd`).
- `game/byml.rs`, `sarc.rs`, `msbt.rs` parsers for the parameter tables (`RSDB/*.rstbl.byml.zs`), the message
  archives (`Mals/EUfr|EUen|USes|JPja`, one per interface language) and their texts. `game/bntx.rs` BNTX textures: Tegra block-linear deswizzle
  (`tegra_swizzle`), ASTC/BCn decode (`texture2ddecoder`), PNG (`png`).
- **Build tags.** The RomFS names its tables and message archives with a tag that changes between game builds
  (`RSDB/<table>.Product.<tag>.rstbl.byml.zs`, `Mals/<lang>.Product.<tag>.sarc.zs`: `b20` in 11.2.0, `b30` in 11.3.0). Nothing
  in the launcher assumes a tag: `game/catalogue.rs` finds the table or archive by its name and `.Product.` whatever follows, and
  `kit-tools/profile/s3save.py` (`rsdb`) takes the one tag the RomFS index holds for that table (an index with none, or
  with two, is an error that says so). A test builds a synthetic RomFS with mixed tags.
- `game/catalogue.rs` builds the typed catalog; `game/service.rs` caches it as JSON, keyed by image size and
  index hash, in `<data dir>\game-cache\<key>\`; `game/icons.rs` decodes icons on demand (custom protocol
  `gameicon`, 4 worker threads) into `<data dir>\game-cache\<key>\icons\`. The page can also extract every icon in
  the background. If the RomFS is missing, the page says what is needed and the Character tab falls back to a neutral
  built-in list (ids only).
- Selectability rules (what counts as a real player item) are documented at the top of `game/catalogue.rs`.
- **Stages.** The catalog also lists the versus stages (`RSDB/VersusSceneInfo`: the scene name the engine, the
  protocol and the result files use), named from `CommonMsg/VS/VSStageName.msbt` in the four interface languages and
  pictured by the 640x360 screenshots of the game's stage menus (`UI/Icon/StageL/Vss_<label>.bntx.zs`, ASTC 4x4). The
  pictures are served scaled (`gameicon` key `<key>@480` for a card, `<key>@160` for a miniature: `icons::SIZES`, area
  average in linear light); building the cache decodes them all into `icons\` (about 7 MB). The page asks for them
  with `gamedata_stages` (`src/stages.tsx`: `StageCard`, `StagePicker` to choose or ban, `StageField`, `StageTag`,
  `StageStrip`), only on a screen that shows a stage. Without game data the cards are neutral, named by the engine's
  list, the server, or the id made readable, and the room form keeps its drop-down list.
- `cargo test --test game_data -- --ignored --nocapture` with `KALAMAR_GAME_ROOT` set runs the real-data check
  (`KALAMAR_STAGE_DUMP=<folder outside the repository>` keeps the stage pictures to look at them).

## profile.json and how it reaches the game

`profile.json` (launcher data dir), schema 1; every number is a game id, `null` = not chosen:

```json
{ "schema": 1, "name": "Kay",
  "style": 0, "skin": 2, "eyes": 5, "hair": 12, "eyebrows": 1, "legwear": 3, "legwear_variation": 0,
  "head": 1000, "clothes": 8031, "shoes": 4024, "weapon": 10,
  "banner": 11001, "title": { "adjective": 142, "subject": 0 }, "badges": [null, null, null] }
```

At launch (`profile::prepare_launch`) the launcher converts it to the runtime's `save_profile` schema
([save-profile.md](../../engine/docs/save-profile.md)), writes it atomically to `<root>\userdata\launcher-save-profile.json` and passes
`KLM_SAVE_PROFILE=<that file>`, `KLM_PLAYER_NAME` and `KLM_NICKNAME` (the name). The game itself applies it to its
encrypted save while starting (previous save copied to `profile\save-profile-backup\<time>\`). Afterwards the Play screen
reads the `save profile:` lines of the run log and says whether it was applied, already up to date, refused (with the
runtime's reason) or not passed.

- Styles: launcher style id = save `ModelType` = index in `inkling_f, inkling_m, octoling_f, octoling_m`
  (SquidF 0, SquidM 1, OctopusF 2, OctopusM 3). The creation screen's four kind buttons sit left to right, the first two
  under the Inkling heading and the last two under the Octoling one (pane positions in `Tuto_MakePlayer_00`).
- `null` fields are left out of the file: unchanged in the save. **A save exists** (a `save\<uid>\save.dat` in the
  user data folder): the profile may be partial, every field including the appearance is optional, and applying it
  twice changes nothing (the runtime logs `0 changes`, writes nothing and makes no backup). **No save yet**: the game
  creates one and the profile must hold the whole appearance (style, hair, eyebrows, legwear, skin, eyes); the launcher
  says exactly what is missing before launching and refuses to launch (the Play screen offers "launch without the
  profile": the game then makes its own character).
- A partial profile never sends `skip_first_launch: true` (the runtime would count it as ending the first launch of a
  save that is still in it); a complete one does.
- Options (Locker): apply the profile, skip the intro and tutorial (default on), unlock everything (default on), and
  a full stock of ability chunks (default off; shown only when the installed game applies sub-abilities).
- The runtime's refusals (`hair 33 belongs to Octolings, not to the style inkling_f ...`, the six dropped gear ids...)
  are read from the log, mapped to the offending field (`profile::classify_refusal`) and shown translated next to that
  field in the Locker and in the Play report, with the runtime's own sentence unchanged below.
- Backups: the Locker and the Play report show the folder (`<user data>\profile\save-profile-backup`) and the number
  of copies; how many the runtime keeps is the `save_profile_backups` setting (Settings, "Save" tab; default 10, 0 = all).
- Unavailable, shown with the reason: the six gear ids the game drops (head 3007, clothes 1030 and 6003, shoes 2011, 3009,
  3012) and every badge (how the save records an acquired badge is unknown).
- Hair and eyebrows must match the style's species; the legwear variation must exist.

## Sub-abilities and ability chunks

Each gear piece has a main ability and up to three sub-abilities. "Unlock everything" owns every piece with its
sub-ability slots empty, so the Gear tab of the Locker lets the player choose the three sub-abilities of the piece
worn on each part (headgear, clothing, shoes), and the Locker's options add a full stock of ability chunks.

- **The profile.** `profile.json` keeps `"sub_abilities": {"head": ["NameA", null, "NameA"], "clothes": null, "shoes":
  null}` (schema stays 1: a file without the member loads with nothing chosen, and a launcher that does not know it
  ignores it). Per part, `null` means never edited: the save keeps what it has. Three slots mean an edit: each is an
  ability name of the player's game or `null` for an empty slot; the same ability three times and gaps are allowed.
  Names are checked against the sub-capable set below (`profile::validate_profile`).
- **What the runtime gets** (`profile::to_runtime`): `gear_abilities` holds the parts that have a chosen piece in the
  profile and an edit, each as three names or `null`s (`{"head": ["NameA", null, "NameA"]}`). A part with no chosen
  piece is left out, because the runtime applies the abilities to whatever piece the save wears. Three empty slots are
  sent as `[null, null, null]`: the runtime writes three empty slots, which is how abilities written earlier are
  cleared (a part left out, or `null`, changes nothing). `chunks` is `"max"` when the option is on and absent
  otherwise (an absent member leaves the stock as it is). The runtime raises a piece with fewer than two stars to two
  when it writes its sub-abilities (the game ties the slot count to the stars), so the piece shows with 2 stars in
  the game. A room option "main abilities only" on a community server makes a match ignore sub-abilities. The main
  ability has a section of its own below.
- **Which abilities fit a sub slot.** The game's ability enum numbers the abilities a sub slot can hold from 0 and
  those only a main slot has from 100. The enum (name to value) is what the install wizard's keys step reads from the
  player's own `main` into `kit-tools\profile\s3save.keys.json` (member `abilities`). `game/subs.rs` reads that member
  and nothing else of the file, keeps the names with a value from 0 to 99 and offers them in enum order; the Locker
  names them and draws their icons from the catalogue (`catalogue.abilities`). No ability name is in the launcher.
  The keys file is found in the install folder (the wizard's choice, else two levels above the game folder).
- **The gate.** The editor and the chunk option appear when the installed game's engine lists the capability
  `gear-abilities` and the ability list could be read (`gear_abilities_info`). The engine prints it as a string in the
  `capabilities` array of the `--list-match-options` document (cached per executable like the rest of the list,
  `proto/engine.rs`, `Caps::gear_abilities`). A game built from an older engine ignores `gear_abilities` and `chunks`
  without a word, so without the capability the Locker shows one line that sub-abilities need a game update, with the
  "Game update available" button that opens Play, where the update is offered. Stored choices are kept meanwhile.
- **A match's loadout.** The loadout file (`KLM_MATCH_LOADOUT`, the same format) carries `gear_abilities` for each part
  it names gear for, from the player's choices in the Locker (`contract::loadout_profile`), including an explicit
  empty. A part without a choice adds nothing. The match's copy of the save also gets the Locker's own profile.
- **Refusals** of the runtime for these fields (a name the game does not know, a main-only ability, a malformed part,
  gear the save does not own, a bad `chunks`) are mapped in `profile::classify_refusal` to `sub_abilities.<part>` and
  `chunks` and shown in the Gear tab (`refusal.sub_*` in the locales).
- `kit-tools\profile\s3save.py profile` is a developer aid the launcher never runs: it is not extended for these
  fields.

## Main abilities

The Gear tab also lets the player choose the main ability of the piece worn on each part, in a main slot beside the
three sub slots.

- **The profile.** `profile.json` keeps `"main_abilities": {"head": "NameA", "clothes": null, "shoes": null}` (schema
  stays 1, as for `sub_abilities`). Per part, `null` means never chosen: the save keeps the piece's main ability as it
  is, and the slot shows the piece's own (`Entry::ability` in the catalogue). A choice is one ability name of the
  player's game; a main slot is never empty. Choosing the piece's own ability back ("Piece's own main ability") is a
  choice too, sent like any other: that is how a main ability written by an earlier launch is undone. Like the
  sub-abilities, the choice belongs to the part, not to the piece: another piece chosen for the part gets it too.
- **Which abilities fit a main slot of a part.** The engine takes any ability of the game's enum but the empty one,
  provided a player piece of that part has it as its main ability in the game: every ability a sub slot can hold, plus
  the main-only abilities of that part (the engine's `docs/save-profile.md`, "Main abilities"). Measured: the game keeps
  any main ability written in the save, but a match where a worn piece has another part's main-only ability does not
  start, so the engine refuses that profile. `game/subs.rs` reads the enum's names from 0 (`read_main_capable`), and
  `profile::part_main_abilities` the main abilities of the part's selectable pieces in the catalogue; the picker offers
  the names found in both (`subSlots.ts`, `mainChoices`), by name, with the piece's own marked, and `profile::validate_profile`
  checks a choice the same way. No ability name is in the launcher.
- **What the runtime gets** (`profile::to_runtime`): `main_abilities` holds the parts that have a chosen piece in the
  profile and a choice, each as one name (`{"head": "NameA"}`). A part with no chosen piece is left out, as for the
  sub-abilities.
- **The gate.** The main slot appears when the installed game's engine lists the capability `main-abilities` and the
  ability list could be read (`gear_abilities_info`: `main_supported`, `main_update_needed`, `main_abilities`;
  `Caps::main_abilities`). An engine built before it applies the sub-abilities but ignores `main_abilities` without a
  word, so the Gear tab then shows the sub slots and one line that the main ability needs a game update, with the same
  "Game update available" button. Stored choices are kept meanwhile.
- **Presets** store the main abilities with the rest of the loadout (`loadouts.json`, `main_abilities`), only when the
  installed game applies them, and apply them the same way; a name the part cannot take is dropped, and a preset saved
  before this field applies none.
- **A match's loadout** carries `main_abilities` for each part it names gear for, from the player's choice in the
  Locker (`contract::loadout_profile`); an engine without the capability ignores the member. What is reported to the
  server stays ids only.
- **Rooms.** A room set to "main abilities only" ignores the sub-abilities, not the main ability: measured, the
  record of such a match listed the chosen main ability for that player in both players' result files. So such a
  room does not mean the pieces' own main abilities once players can choose them.
- **Refusals** of the runtime for this field (a name the game does not know, an ability no piece of that part has, the
  empty value, a malformed member, gear the save does not own) are mapped in `profile::classify_refusal` to
  `main_abilities.<part>` and shown in the Gear tab (`refusal.main_*` in the locales).
