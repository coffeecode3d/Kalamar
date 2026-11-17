// Splatoon 3: automatic LAN match entry and the match result for a launcher (auto_match.c, docs/auto-match.md).
#pragma once
#include <stdint.h>

extern volatile int automatch_on;          // SPLAT3_MATCH is in effect
void automatch_leave(void);                // the menu's "Leave match": stops the flow as an error (left-by-player, exit code 3)
void automatch_closing(void);              // gfx_window_closed: the window closed during the match (a departure, written first)
void automatch_init(void);                 // boot.c, after scene_start_init
void automatch_file(const char *path);     // romfs.c: the game opens a RomFS file (path relative to the RomFS root)
void automatch_fn_hit(uint64_t addr);      // main.c fnprof_hit while FNPROF_AUTO is on
// SPLAT3_MATCH_RULES mode=: the neutral key of the game's rule id `rule` in the private room's mode selector, NULL
// past the selector's last
const char *automatch_mode_key(int rule);
const char *automatch_option_key(int i);   // the room's other settings' keys (main_abilities_only, ...), NULL past the last
// The game's rule names, its VersusRule enum text in main (loaded at main_base), into names: how many (0 unreadable)
int automatch_rule_names(uint64_t main_base, char (*names)[8], int cap);
int list_match_options(void);              // --list-match-options (match_options.c): modes and stages, from game files
// What was run with spectators (docs/auto-match.md "Spectators"), listed by --list-match-options: the most spectators
// in one match, the modes played with one (JSON strings), whether a host in the stands was run
#define SPECTATORS_VERIFIED_MAX 2
#define SPECTATORS_VERIFIED_MODES "\"turf\", \"zones\""
#define SPECTATOR_HOST_VERIFIED 1
int automatch_selftest(void);              // --probe-automatch: the rules parser and the team rows, without the game
// The window back ends (gfx_window_win.c, gfx_window_sdl.c): 1 when the window must open hidden (SPLAT3_MATCH: the
// game stays out of sight until the match is presented). Asked once, when the window is made.
int automatch_window_hidden(void);
// 1 once the match's own music (the one with "Go!") has been opened in this round: the battle is on. For the mouse
// gyro bench's tests of a match (mgyro_bench.c); 0 without SPLAT3_MATCH.
int automatch_battle_on(void);
// Implemented by the window back ends: show (1: shown, raised and focused as at a normal start, the fullscreen setting
// applied) or hide (0: no window, no taskbar button) the game window. Any thread; no-op before the window exists.
void gfx_window_show(int show);
// audio_mixer.c: 1 silences the game's output (the mix goes on, at volume 0), 0 restores it. Any thread.
void audio_mute(int on);
