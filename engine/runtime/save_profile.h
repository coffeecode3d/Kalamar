// Launcher profile for Splatoon 3 (save_profile.c), save_profile (TOTK_SAVE_PROFILE=<profile.json>).
#pragma once
#include <stddef.h>
#include <stdint.h>

// fs_host.c storage_init, once the save storage is recovered: applies the profile to <save_dir>/save.dat (a backup goes
// into <profile_dir>/save-profile-backup/<time>/), or notes that the game creates the save in this run.
void save_profile_at_start(const char *save_dir, const char *profile_dir);
// fs_host.c, after each save Commit: when there was no save at start, applies the profile to the save the game just
// committed in save_dir and restarts the game so it loads it. Nothing otherwise.
void save_profile_after_commit(const char *save_dir);
// The profile's "name", NULL without a profile or a name (ov_system.c GetNickname).
const char *save_profile_name(void);
// The match's copy of the save (fs_host.c, SPLAT3_MATCH; docs/auto-match.md "The match's save"). No backup is made: the
// copy is deleted when the game ends.
// SPLAT3_MATCH_LOADOUT: applies the profile.json at path to <save_dir>/save.dat. 1 when applied (0 changes included),
// 0 when the profile is unreadable or refused, with the reason in why.
int save_profile_apply_copy(const char *save_dir, const char *path, char *why, size_t cap);
// The weapon and gear ids the applied match profile asks for (-1: left as the save has them); 0 without one.
int save_profile_match_loadout(int64_t *weapon, int64_t gear[3]);
// Raises the player data's Season/CurrentSeasonId to the newest Season of RSDB/VersusSceneInfo's listed stages, so the
// private room's stage list holds every stage the game's stage lists show. The season (>= 0), or -1 and why.
int64_t save_profile_open_stages(const char *save_dir, char *why, size_t cap);
// The gear (part 0 head, 1 clothes, 2 shoes) that the game removes from the ownership maps when it loads a save:
// unlock_all leaves it out and a profile cannot equip it.
int save_profile_gear_dropped(int part, int64_t id);
// The appearance fields (style, hair, eyebrows, legwear, skin, eyes) that a profile given as JSON text lacks, comma
// separated in names: how many, -1 when json is not an object. A new save needs all of them.
int save_profile_missing_appearance(const char *json, char *names, size_t cap);
// A profile's gear_abilities (JSON text) checked against an ability enum string in the form main holds it
// ("None = -1 , <name> = 0 , ..."): the parts given as a bit mask (1 head, 2 clothes, 4 shoes) with their three sub
// abilities in sub (enum values, -1 empty), or -1 and why when it is refused.
int save_profile_gear_abilities_check(const char *json, const char *enum_s, int64_t sub[3][3], char *why, size_t cap);
// A profile's main_abilities (JSON text) checked against an ability enum string in that form and, per part (0 head,
// 1 clothes, 2 shoes), the enum values a main slot of that part may take (allowed[part], a list ending with -2, as the
// game's gear tables give them at run time; allowed NULL: any value of the enum from 0): the parts given as a bit mask
// with their main ability in main_v (an enum value), or -1 and why when it is refused.
int save_profile_main_abilities_check(const char *json, const char *enum_s, const int64_t *const allowed[3],
                                      int64_t main_v[3], char *why, size_t cap);
// The value of the entry at position pos of main's ability enum (0: its first entry, the empty value -1), as the save
// stores an ability; -2 when main has no such entry (auto_match.c: the match record numbers abilities by position).
int64_t save_profile_ability_at(int pos);
// Keeps the newest `keep` yyyymmdd-hhmmss folders of dir (a folder named save-profile-backup), deleting the older
// ones by that name pattern, never following a link; keep 0 keeps all. Returns the number deleted.
int save_profile_prune_backups(const char *dir, int keep);
// --probe-profile (save_profile_probe.c): no game files.
int probe_profile(void);

// A RomFS table, RSDB/<name>.Product.<tag>.rstbl.byml.zs (the build's tag: splat3_addr.h; an array of rows of
// scalars), read from the RomFS image: NULL when it is missing or not such a table. A field's value, or def / "" / 0
// when the row has no such field of that type.
typedef struct RsdbTable RsdbTable;
RsdbTable *rsdb_open(const char *name);
uint32_t rsdb_rows(const RsdbTable *t);
int64_t rsdb_int(const RsdbTable *t, uint32_t row, const char *field, int64_t def);
const char *rsdb_str(const RsdbTable *t, uint32_t row, const char *field);
int rsdb_bool(const RsdbTable *t, uint32_t row, const char *field);
void rsdb_close(RsdbTable *t);
