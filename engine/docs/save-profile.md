# Splatoon 3 save profile

`save_profile = <path of a profile.json>` (`KLM_SAVE_PROFILE`) lets a launcher choose the player's character and
start the game without its first-launch flow (character creation, tutorial): at start the runtime writes the profile
into the save, then the game boots straight to the plaza with that character. `runtime/save_profile.c`.

## profile.json

```json
{
  "version": 1,
  "name": "Player",
  "style": "octoling_f",
  "hair": 33,
  "eyebrows": 21,
  "legwear": 1,
  "legwear_variation": 2,
  "skin": 2,
  "eyes": 18,
  "skip_first_launch": true,
  "splashtag": {"banner": 11020, "adjective": 200, "subject": 300, "badges": [0, 0, 0]},
  "gear": {"head": 1018, "clothes": 5001, "shoes": 6003},
  "weapon": 50,
  "emote": 7,
  "unlock_all": false,
  "gear_abilities": {"head": ["<ability>", "<ability>", null], "clothes": [3, 3, 3]},
  "chunks": "max"
}
```

| Key | Values | Save field |
| --- | --- | --- |
| `name` | 1 to 10 characters | The game shows the system nickname (GetNickname), which the runtime answers with `KLM_NICKNAME` (LAN play), else `player_name`, else this name. Also written to the player data's `UserName`. |
| `style` | `inkling_f`, `inkling_m`, `octoling_f`, `octoling_m` | `Coordinates/ModelType` 0 to 3 (main's `spl::PlayerModelType`: SquidF, SquidM, OctopusF, OctopusM) |
| `hair` | ids of RSDB `HairInfo` rows with `Order` >= 0, not Side Order: 0 to 15 (Inkling), 30 to 37 (Octoling) | `HairId` |
| `eyebrows` | `EyebrowInfo`: 0 to 3 (Inkling), 20 to 23 (Octoling) | `Eyebrow` |
| `legwear`, `legwear_variation` | `BottomInfo`: 0 to 8; a variation below the row's `VariationNum` (legwear 1 has 3) | `BottomId`, `BottomVariation` |
| `skin`, `eyes` | 0 to 8 and 0 to 20: the values RSDB `NpcInfo` rows use; no table lists the player's | `SkinColor`, `EyeColor` |
| `skip_first_launch` | default `true` | `Plaza/IsDoneFirstBootSeq` = true |
| `splashtag` | `banner`: `NamePlateBgInfo` id; `adjective`, `subject`: `BynameAdjectiveInfo`, `BynameSubjectInfo` ids; `badges`: three `BadgeInfo` ids, 0 for an empty slot; optional `title_gender` | `NamePlateEditor/NamePlate` and the player data's `LastPlayNamePlate` |
| `gear` | `head`, `clothes`, `shoes`: `GearInfoHead` / `GearInfoClothes` / `GearInfoShoes` ids, not `HowToGet: Impossible` | `Coordinates/GearHeadId` ... (variation 0) |
| `weapon` | `WeaponInfoMain` ids of `Type: Versus` | `Coordinates/WeaponId` |
| `emote` | `EmoteInfo` ids | `Coordinates/WinEmote` |
| `unlock_all` | `true` owns every weapon, gear, banner, title word and emote of those tables | the ownership maps below |
| `gear_abilities` (prototype) | `head`, `clothes`, `shoes`: three sub abilities each, as a name of main's ability enum, the enum's value, or `null` (also `"None"`, -1) for an empty slot; only the abilities a sub slot can hold (the enum's values below 100) | `ExSkillArray` and `Rarity` of the equipped piece's ownership record ([Sub abilities](#sub-abilities)) |
| `chunks` (prototype) | `"max"` (999) or a count 0 to 999 | `HaveSkillChipMap[ability]` = the count, for every ability a sub slot can hold |

Every id is checked against the game's own RomFS tables (`RSDB/*.rstbl.byml.zs`); a profile with a bad value leaves the
save untouched and logs why. The appearance goes into both copies the save keeps (`Common/Coordinates` and the player
data's `LastPlayCoordinates`).

An item the save does not own yet is added the way a new save records its starter items:

| Item | Player data (root 1) | Game data (root 0) |
| --- | --- | --- |
| weapon | `HaveWeaponMap[id]`: the starter's record with its counters at 0 | `Customize/HaveWeaponMap[id]`: `DateTimeUtc` now, `IsLookedCustomize` true |
| gear | `HaveGear{Head,Clothes,Shoes}Map[id]`: `Rarity` the table's, `MainSkill` its main ability (the ability enum's value), `ExSkillArray` rarity + 1 empty (-1) slots, the rest 0 | `Customize/HaveGear...Map[id]` as for a weapon |
| banner | `HaveNamePlateBgMap[id]`: `AcquiredDateTimeUtc` 0 | |
| title word | `HaveBynameAdjectiveMap[id]`, `HaveBynameSubjectMap[id]`: true | |
| emote | `HaveEmoteMap[id]`: true | `Customize/HaveEmoteMap[id]` as for a weapon |

With `unlock_all` the game keeps every added item when it saves again, except six gear ids it drops from the
ownership maps at load, for a reason not found (all `HowToGet: Shop`): head 3007, clothes 1030 and 6003, shoes 2011,
3009 and 3012. Do not equip those.

The list lives in `runtime/save_profile.c` (`gear_dropped`, the same as `GEAR_DROPPED` in the launcher's `launcher/kit-tools/profile/s3save.py`): `unlock_all`
leaves those ids out and a profile that equips one is refused, so applying the same profile again finds nothing to add.

Badges are only equipped when the save already owns them: no save seen so far owns one, so how the game records a
badge (`Common/HaveNamePlateBadgeMap`, `LastPlayHaveBadgeMap`) is not known.

## Sub abilities

Prototype (branch `gear`). What the save holds, measured on test saves by editing them and decoding what the game wrote
back at its next commit:

- A piece's abilities exist only in its ownership record, `HaveGear{Head,Clothes,Shoes}Map[id]` of the player data:
  `MainSkill`, `ExSkillArray` (one integer per sub slot: an ability enum value, -1 when empty), `Rarity`, `Exp`,
  `TotalExp`, `LastPlayDateTimeUtc`, `RandomContext`. The equipped gear (`Coordinates/Gear...Id`) is an id only: it
  names the record. No field holds a slot count or a brand; the brand is the gear table's.
- The game ties the slot count to `Rarity` when it loads the save: a record of rarity 0 given three sub abilities came
  back with one, rarity 2 kept three, a fourth entry was dropped, rarity 5 was kept as written with three slots. So
  `gear_abilities` raises `Rarity` to 2 on a piece that has less, which the game shows as that piece's stars.
- Kept as written: the same ability three times, an empty slot between two filled ones.
- An ability of the main-only group (enum value 100 and more) in a sub slot came back as 0, the first ability of the
  enum, not as an empty slot: the profile refuses those instead of writing them.
- `HaveSkillChipMap` (empty in a new save) kept the entries `ability value: count` written as plain integers. Whether
  the game's own screens spend them was not looked at.

`gear_abilities` applies to the pieces equipped once the profile's `gear` is in (the profile's, else the save's), and
only to the parts it names. A profile whose `gear_abilities` or `chunks` is malformed, names an unknown ability or puts
a main-only ability in a sub slot is refused whole, as for the other fields. In a match the game sends each player's
gear abilities to the others: the result file lists them ([auto-match.md](auto-match.md), "The result"). With the
room's `main_abilities_only=1` the record of both players' games held every sub slot empty while the saves kept
theirs. That an ability in the record changes the play (speed, ink use) was not measured.

## When it is applied

- A save exists: at the first use of the save storage, before the game reads it. When something changes, the old file
  is first copied to `<profile>/save-profile-backup/<time>/save.dat`. Only the fields above change; progress stays.
  When nothing differs the apply is a strict no-op: no write, no backup, and the log says `0 changes`.
- Partial profiles: with a save, every absent or `null` field means "leave as it is", the appearance fields too
  (`legwear` without `legwear_variation` keeps the save's variation when the new legwear has it, else 0).
  `skip_first_launch` absent counts as true only when the profile also gives the whole appearance. Hair and eyebrows
  must belong to the species of the style (the profile's, else the save's): a profile whose hair or eyebrows are of the
  other species, or that changes the style and leaves the save's hair of the old species, is refused whole with a
  message. A new save needs `style`, `hair`, `eyebrows`, `legwear`, `skin` and `eyes`; without them the log names the
  missing ones and the game creates its own character.
- Backups: the newest `save_profile_backups` (default 10; 0 keeps all) are kept; older `yyyymmdd-hhmmss` folders of
  `save-profile-backup` are deleted after a new backup, never a link followed, nothing else touched.
- No save yet: the game creates a new save at its first boot and continues into its first-launch flow. Once it has
  committed that save, the profile goes into it and the game restarts, then starts in the plaza.
- An automatic match (`KLM_MATCH`): the game plays on a copy of the save and the profile goes into the copy only,
  without a backup, followed by the match's own loadout (`KLM_MATCH_LOADOUT`, a profile of the same format); the
  player's save is not written and the copy is deleted when the game ends. Without a save the match stops
  (`save-unavailable`) instead of taking the first-launch path. [auto-match.md](auto-match.md), "The match's save".

## Start scene

`start_scene = <value>` (`KLM_START_SCENE`; `KLM_START` overrides it for one launch) picks the scene the game goes
to after its Boot scene, in place of the plaza with its title screen. It needs a save that finished the first launch
(`Plaza/IsDoneFirstBootSeq`, which `skip_first_launch` writes); without one the game creates the character as usual.

| Value | Scene | Result (Splatoon 3 11.2.0) |
| --- | --- | --- |
| `title` (or `off`) | | The untouched start: the title screen, then the plaza. The same as leaving the setting empty. |
| `plaza` | `Plaza` | The plaza in free play, without the title screen and the news (about 30 s after the start). |
| `lobby` | `LobbyVersus` | The online lobby. It starts the internet connection, which the runtime never gives: the game says "a communication error has occurred" and goes back to the plaza, as a console without internet would. |
| `lobby-lan` | `LobbyLocal` in LAN mode | The local lobby in LAN mode, with `KLM_LAN` set ([lan-play.md](lan-play.md)): free to move, `L` opens the Private Battle / Salmon Run menu. Without `KLM_LAN` the LAN start fails and the game goes back to the plaza. |
| `range` | `ShootingRange` | The test range, free to move and shoot. |
| `story` | `SmallWorld` | Not usable yet: the screen stays on the ink transition (`BigWorld` does the same). The game's own way there sets the hero mode's state first (`main+0x3adf500` writes a byte of the state at `+0x6698` before it requests `BigWorld`); that state is not set up here. |
| `scene:<Name>` | any scene pack | For experiments. |

How (`runtime/scene_start.c`): when the Boot scene ends, `main+0x375338c` reads the first-boot flag and requests
`PlayerMake` or `Plaza` through the scene request `main+0x3d2f440(scene manager, const char *const *name, flags)`.
Every lifted function has an entry hook (`FN_ENTER`); while a start scene waits, the hook catches the request whose
return address is the boot choice's `Plaza` call and points its name argument at the wanted scene, then switches
itself off. `lobby-lan` also sets the local lobby's `IsLan` byte, as the game does when the player holds ZL+ZR and the
left stick for three seconds in the local lobby (`main+0x3955360`); the lobby then starts the network in mode 3 (LAN,
job `InitLan`) instead of mode 2 (local wireless, `InitLocal`). The title screen and the news belong to the plaza's
start from Boot: another scene skips them, and `plaza` stores 6 as the plaza's entry kind (its state at `+0x6658`,
field `+0x30`, where the boot choice stores 1), the value the game uses when it sends the player back to the plaza
after a network error, so the plaza starts in free play.

## The save file

The runtime changes the player's own save in the running game's memory, where the game's own code has already read
it; the launcher's helper (`launcher/kit-tools/profile/s3save.py`) does the same on the player's own file. What the
profile code relies on:

- The file is a container around two MessagePack maps: game data (`Common`, `Plaza`, `Customize`, `NamePlateEditor`,
  `Tutorial`, `Mission`, ...) and player data (`UserName`, `LastPlay*`, `Have*Map`, `Money`, ...). Field names are
  hashes of the names; item ids are plain integer keys.
- The container's protection is the game's own. Its parameters are read from the player's own loaded game at run
  time; none of them is in this repository.
- The game reads integers of any MessagePack width and sorts the ownership maps by id when it loads them.
