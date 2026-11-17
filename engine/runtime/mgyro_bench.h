// TOTK_MGYRO_BENCH: a test bench for the mouse as the six-axis sensor (mgyro_bench.c, docs/controls.md "Mouse as the
// gyro: measurements"). While it runs, a simulated mouse sends counts into the motion thread in place of the real mouse
// and the input script, the player's own input is ignored, and every six-axis read of the game, every sample made and
// every present go to a CSV file with the game's aim direction read from its memory.
#pragma once
#include <stdint.h>

#include "input.h"

// 1 when TOTK_MGYRO_BENCH is set (read once): every hook below returns at once otherwise.
int mgb_on(void);
// Motion thread (input.c mouse_gyro_collect): the counts the simulated mouse sent since the last call.
void mgb_take(uint64_t now, double *dx, double *dy);
// input.c: the yaw axis of the mouse gyro's states (MouseGyroCfg.yaw_body): def, unless a bench test sets it.
int mgb_yaw_body(int def);
// input.c: 1 when the game's Y levels the virtual controller too: def, unless the recenter test sets it.
int mgb_level_on_y(int def);
// input.c: the reads held still at a reversal of the yaw (mgyro_frame_yaw): def, unless a bench test sets it.
int mgb_reverse_hold(int def);
// mouse_native.c: 1 when the linear pitch keeps the game's pitch through a change of the camera and has the virtual
// controller follow the view: def, unless a bench test turns both off to compare.
int mgb_native_keep(int def);
// mouse_native.c: 1 when a turn made in squid form or during a change of form moves the pitch along the wide camera's
// table (the human form's): def, unless a bench test turns it off to compare with release 0.3.4.
int mgb_native_ref(int def);
// mouse_native.c: 1 when a frame that resyncs keeps the camera's pitch (all but the game's recentre) and a large turn
// of the mouse is no resync: def, unless a bench test turns it off to compare with release 0.3.4.
int mgb_native_hold(int def);
// mouse_native.c: 1 when a frame in which the game holds the camera's turns (a yaw gain of 0, as after a respawn) keeps
// the camera's pitch: def, unless a bench test turns it off to compare with release 0.3.7 (the curve's result then).
int mgb_native_held(int def);
// Motion thread (input.c mouse_gyro_tick): the samples about to be pushed, oldest first. The bench numbers and logs
// them, and its field tests may change what they report.
void mgb_samples(MotionState *s, int n, uint64_t now);
// Game thread (ov_input.c six_fill): the states the game is handed, newest first, n of them live (0: at rest).
void mgb_read(const MotionState *s, int n, uint64_t now);
// Game thread (nvn_api_obj.c nvnQueuePresentTexture, after the back end took it): the presented texture (a tex_t),
// the game's present number n, when the game presented and the display time the present is held to.
void mgb_present(const void *tex, uint64_t n, uint64_t now, uint64_t slot);
// Present thread (vk_backend.c present_main): the host present of the frame held to display time slot went out at now.
void mgb_flip(uint64_t slot, uint64_t now);
// main.c fnprof_hit while FNPROF_BENCH is on: a function entry, for TOTK_MGYRO_BENCH_OBJ (the objects it watches).
void mgb_fn_hit(uint64_t addr, uint64_t x0);
// Any thread: a timeline mark "M,<t>,<what>,<thread>,<v>" (frame-order study: submits, acquires, reads by caller).
void mgb_mark(const char *what, uint64_t v);
// 1 while the image test runs with TOTK_MGYRO_BENCH_DETAIL=1: command-buffer recordings are marked too (mgb_mark2).
int mgb_detail(void);
// Any thread: "M,<t>,<what>,<thread>,<a>,<b>".
void mgb_mark2(const char *what, uint64_t a, uint64_t b);
