// --probe-mgyro: the mouse as the six-axis sensor (mouse_gyro.c) and Splatoon 3's default keys (input_map.c). Needs no game
// files, no window and no gamepad. The game's side (it turns its camera by the newest state's angular velocity times
// 1/60 s, its part about gravity, and by the change of the tilt) is what TOTK_MGYRO_BENCH measured (docs/controls.md);
// the checks below hold mouse_gyro.c to it.
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "host.h"
#include "input.h"
#include "input_map.h"
#include "mouse_gyro.h"

static int fails;
#define CHECK(c, what) do { if (!(c)) { fails++; fprintf(stderr, "FAIL mgyro: %s\n", what); } } while (0)
#define TWO_PI 6.283185307179586
#define FRAME (1.0 / MOUSE_GYRO_GAME_HZ)

static uint32_t rng = 12345;
static double rnd(void) { rng = rng * 1664525u + 1013904223u; return (rng >> 8) / 16777216.0; }

static MouseGyroCfg cfg(int upright, int yaw_body) {
    return (MouseGyroCfg){.deg_per_count = 0.04, .scale_x = 1, .scale_y = 1, .pitch_limit_deg = 80, .upright = upright,
                          .yaw_body = yaw_body};
}

// B' = B * exp([w] dt) for a body-frame rate w (rad/s): the controller turned about its own axes.
static void turn(double B[3][3], const double w[3], double dt) {
    double rate = sqrt(w[0] * w[0] + w[1] * w[1] + w[2] * w[2]), th = rate * dt;
    if (th < 1e-15) return;
    double k[3] = {w[0] / rate, w[1] / rate, w[2] / rate}, s = sin(th), c = 1 - cos(th);
    double K[3][3] = {{0, -k[2], k[1]}, {k[2], 0, -k[0]}, {-k[1], k[0], 0}}, E[3][3], out[3][3];
    for (int i = 0; i < 3; i++)
        for (int j = 0; j < 3; j++) {
            double kk = 0;
            for (int q = 0; q < 3; q++) kk += K[i][q] * K[q][j];
            E[i][j] = (i == j) + s * K[i][j] + c * kk;
        }
    for (int i = 0; i < 3; i++)
        for (int j = 0; j < 3; j++) {
            double v = 0;
            for (int q = 0; q < 3; q++) v += B[i][q] * E[q][j];
            out[i][j] = v;
        }
    memcpy(B, out, sizeof out);
}

static void dir_to_B(const MotionState *s, double B[3][3]) {
    for (int i = 0; i < 3; i++)
        for (int j = 0; j < 3; j++) B[j][i] = s->dir[i * 3 + j];
}

static double max_diff(const double A[3][3], const double B[3][3]) {
    double m = 0;
    for (int i = 0; i < 3; i++)
        for (int j = 0; j < 3; j++) m = fmax(m, fabs(A[i][j] - B[i][j]));
    return m;
}

// The game's yaw for one state, in degrees: its angular velocity's part about gravity (the world's up, -acc in controller
// axes) times 1/60 s. Mouse right turns right: clockwise seen from above, a negative turn about the up axis.
static double game_yaw_deg(const MotionState *s) {
    double n = sqrt(s->acc[0] * s->acc[0] + s->acc[1] * s->acc[1] + s->acc[2] * s->acc[2]), w = 0;
    for (int i = 0; i < 3; i++) w += -s->acc[i] / n * s->gyro[i] * TWO_PI;
    return w * FRAME * 180 / M_PI;
}

static void keys_check(void) {
    input_map_defaults_for(RT_GAME_SPLATOON3);
    input_map_reset();
    uint8_t dn[KEY_COUNT] = {0};
    int32_t st[2];
#define PRESS(k) (memset(dn, 0, sizeof dn), dn[k] = 1, input_map_buttons(dn))
    CHECK(PRESS(KEY_MOUSE_L) == 1ull << B_ZR, "layout: left click is ZR (shoot)");
    CHECK(PRESS(KEY_MOUSE_R) == 1ull << B_R, "layout: right click is R (sub weapon)");
    CHECK(PRESS(KEY_LSHIFT) == 1ull << B_ZL, "layout: Shift is ZL (squid)");
    CHECK(PRESS(KEY_SPACE) == 1ull << B_B, "layout: Space is B (jump)");
    CHECK(PRESS(KEY_E) == 1ull << B_STICK_R && PRESS(KEY_MOUSE_M) == 1ull << B_STICK_R, "layout: E and middle click press the right stick (special)");
    CHECK(PRESS(KEY_Q) == 1ull << B_Y, "layout: Q is Y (recenter)");
    CHECK(PRESS(KEY_F) == 1ull << B_A, "layout: F is A");
    CHECK(PRESS(KEY_M) == 1ull << B_X && PRESS(KEY_TAB) == 1ull << B_X, "layout: M and Tab are X (map)");
    CHECK(PRESS(KEY_ENTER) == 1ull << B_PLUS, "layout: Enter is Plus (Esc belongs to the settings menu)");
    CHECK(PRESS(KEY_UP) == 1ull << B_UP && PRESS(KEY_DOWN) == 1ull << B_DOWN && PRESS(KEY_LEFT) == 1ull << B_LEFT &&
              PRESS(KEY_RIGHT) == 1ull << B_RIGHT, "layout: the arrows are the D-pad");
    memset(dn, 0, sizeof dn);
    dn[KEY_W] = 1;
    input_map_stick(dn, 0, st);
    CHECK(st[0] == 0 && st[1] == 32767, "layout: W is left stick up");
    memset(dn, 0, sizeof dn);
    dn[KEY_UP] = 1;
    input_map_stick(dn, 1, st);
    CHECK(st[0] == 0 && st[1] == 0, "layout: the arrows leave the right stick alone");
    memset(dn, 0, sizeof dn);
    dn[KEY_I] = 1;
    input_map_stick(dn, 1, st);
    CHECK(st[1] == 32767, "layout: I is right stick up (camera without motion controls)");
    CHECK(!input_map_held(dn, B_SHAKE), "layout: nothing shakes");
    CHECK(input_map_buttons((uint8_t[KEY_COUNT]){[KEY_C] = 1, [KEY_X] = 1, [KEY_G] = 1, [KEY_1] = 1}) == 0, "layout: TotK's other keys are free");
    input_map_defaults_for(RT_GAME_TOTK);
    input_map_reset();
    CHECK(PRESS(KEY_MOUSE_L) == 1ull << B_Y && PRESS(KEY_F) == 1ull << B_ZR, "layout: TotK keeps its own defaults");
    input_map_defaults_for(0);
    input_map_reset();
}

int probe_mgyro(void) {
    MotionState s;

    // 1. One frame's counts: the state's angular velocity times 1/60 s is the controller's turn, exactly, at any count
    // (one count to a 360 degree flick), and the attitude has turned as much.
    for (int mode = 0; mode <= 2; mode++) {
        MouseGyroCfg c = cfg(0, mode);
        MouseGyro g;
        mgyro_reset(&g, 0);
        double worst = 0, turned = 0, sum = 0;
        static const double n[] = {1, -1, 7, 120, -300, 2000, 9000, -9000};
        for (size_t i = 0; i < sizeof n / sizeof *n; i++) {
            mgyro_frame(&g, &c, n[i], 0, NULL, FRAME, 0.005, &s);
            double y = game_yaw_deg(&s);
            worst = fmax(worst, fabs(y - (-n[i] * 0.04)) / fmax(1, fabs(n[i] * 0.04)));
            turned += y, sum += -n[i] * 0.04;
        }
        CHECK(worst < 1e-5, "frame: the rate over 1/60 s is the counts' turn (level, every yaw axis)");
        CHECK(fabs(turned - sum) < 1e-3, "frame: the turns add up to the counts");
        CHECK(s.delta_ns == 5000000, "frame: delta_ns is the time since the previous state");
    }

    // 2. Level, the rate about the world's up integrates to the attitude (the controller turned about its own z).
    {
        MouseGyroCfg c = cfg(0, 0);
        MouseGyro g;
        mgyro_reset(&g, 0);
        double B[3][3], want[3][3];
        mgyro_rest(&g, 0.005, &s);
        dir_to_B(&s, B);
        double worst = 0;
        for (int i = 0; i < 200; i++) {
            mgyro_frame(&g, &c, floor(rnd() * 61) - 30, 0, NULL, FRAME, 0.005, &s);
            double w[3] = {s.gyro[0] * TWO_PI, s.gyro[1] * TWO_PI, s.gyro[2] * TWO_PI};
            turn(B, w, FRAME);
            dir_to_B(&s, want);
            worst = fmax(worst, max_diff(B, want));
        }
        CHECK(worst < 2e-4, "level: integrating the rate over the frames reproduces the direction matrix");
    }

    // 3. Pitched 20 degrees up: the yaw rate on the controller's vertical axis divided by the cosine (mode 2) has the
    // whole turn about gravity and nothing about the controller's y (the game moves its pitch with that); about the
    // world's up (mode 0) the turn is whole too; on the vertical axis alone (mode 1) it is cos(20) of it.
    {
        double got[3], roll[3];
        for (int mode = 0; mode <= 2; mode++) {
            MouseGyroCfg c = cfg(0, mode);
            MouseGyro g;
            mgyro_reset(&g, 0);
            mgyro_frame(&g, &c, 0, -500, NULL, FRAME, 0.005, &s);   // 20 degrees up
            mgyro_frame(&g, &c, 250, 0, NULL, FRAME, 0.005, &s);    // 10 degrees right
            got[mode] = game_yaw_deg(&s), roll[mode] = s.gyro[1];
        }
        CHECK(fabs(got[2] + 10) < 1e-4 && fabs(roll[2]) < 1e-9, "pitched: mode 2 turns 10 degrees about gravity, no rate about y");
        CHECK(fabs(got[0] + 10) < 1e-4 && fabs(roll[0]) > 1e-3, "pitched: mode 0 turns 10 degrees, part of it about y");
        CHECK(fabs(got[1] + 10 * cos(20 * M_PI / 180)) < 1e-4, "pitched: mode 1 turns cos(20) of it");
    }

    // 4. Pitch: up looks up, clamped at the limit; the tilt is in the direction matrix and gravity; invert and scales.
    {
        MouseGyroCfg c = cfg(0, 2);
        MouseGyro g;
        mgyro_reset(&g, 0);
        mgyro_frame(&g, &c, 0, -250, NULL, FRAME, 0.005, &s);   // 10 degrees up
        CHECK(fabs(asin(s.dir[5]) * 180 / M_PI - 10) < 1e-4, "pitch: 250 counts up tilt the controller's y axis 10 degrees up");
        CHECK(s.gyro[0] > 0 && fabs(s.gyro[0] * TWO_PI * FRAME * 180 / M_PI - 10) < 1e-4, "pitch: the rate about x is the tilt over the frame");
        CHECK(fabs(s.acc[1] + sin(10 * M_PI / 180)) < 1e-5 && fabs(s.acc[2] + cos(10 * M_PI / 180)) < 1e-5,
              "pitch: gravity tilts with it");
        for (int i = 0; i < 10; i++) mgyro_frame(&g, &c, 0, -1000, NULL, FRAME, 0.005, &s);
        CHECK(fabs(g.pitch * 180 / M_PI - 80) < 1e-9, "pitch: clamped at the limit");
        mgyro_frame(&g, &c, 0, -100, NULL, FRAME, 0.005, &s);
        CHECK(fabs(s.gyro[0]) < 1e-9, "pitch: no rate while pushing into the limit");
        MouseGyroCfg inv = c;
        inv.invert_y = 1, inv.scale_x = 2, inv.scale_y = 0.5;
        mgyro_reset(&g, 0);
        mgyro_frame(&g, &inv, 10, -10, NULL, FRAME, 0.005, &s);
        double k = 0.04 * M_PI / 180;
        CHECK(fabs(g.pitch - (-10 * k * 0.5)) < 1e-12 && fabs(g.yaw - (-10 * k * 2)) < 1e-12,
              "scale: invert_y flips pitch, x and y scale each axis");
        mgyro_recenter(&g);
        CHECK(g.pitch == 0 && g.yaw == 0, "recenter: level and yaw 0");
        // a turn to follow (mouse_native.c): at most rate_deg of it a frame after the counts, in the rate and the matrix,
        // counted in g.followed, and toward a pitch within span_deg of level
        MouseGyroFollow f = {.delta_deg = -11, .rate_deg = 2, .span_deg = 50};
        mgyro_reset(&g, 0);
        mgyro_frame(&g, &c, 0, -250, NULL, FRAME, 0.005, &s);   // 10 degrees up
        mgyro_frame(&g, &c, 0, -25, &f, FRAME, 0.005, &s);      // 1 up by the mouse, 2 down by the follow
        CHECK(fabs(g.pitch * 180 / M_PI - 9) < 1e-9 && fabs(g.followed + 2) < 1e-9, "follow: 2 degrees of the turn after the counts");
        CHECK(fabs(s.gyro[0] * TWO_PI * FRAME * 180 / M_PI + 1) < 1e-4 && fabs(asin(s.dir[5]) * 180 / M_PI - 9) < 1e-4,
              "follow: the rate and the matrix carry the frame's whole turn");
        for (int i = 0; i < 10; i++) {
            f.delta_deg = -g.pitch * 180 / M_PI;   // back to level, asked again each frame
            mgyro_frame(&g, &c, 0, 0, &f, FRAME, 0.005, &s);
        }
        CHECK(fabs(g.pitch) < 1e-12 && fabs(g.followed + 11) < 1e-9, "follow: stops where it was asked to");
        f.delta_deg = 20, f.span_deg = 5;
        mgyro_frame(&g, &c, 0, 0, &f, FRAME, 0.005, &s);
        mgyro_frame(&g, &c, 0, 0, &f, FRAME, 0.005, &s);
        mgyro_frame(&g, &c, 0, 0, &f, FRAME, 0.005, &s);
        CHECK(fabs(g.pitch * 180 / M_PI - 5) < 1e-9, "follow: not past span_deg from level");
        mgyro_frame(&g, &c, 0, -250, NULL, FRAME, 0.005, &s);   // 10 degrees further up by the mouse: 15
        f.delta_deg = 3;
        mgyro_frame(&g, &c, 0, 0, &f, FRAME, 0.005, &s);
        CHECK(fabs(g.pitch * 180 / M_PI - 13) < 1e-9, "follow: a controller past span_deg is brought back toward it");
    }

    // 5. A state between reads: the attitude as it was, no rate, gravity 1 G.
    {
        MouseGyroCfg c = cfg(0, 2);
        MouseGyro g;
        mgyro_reset(&g, 0);
        mgyro_frame(&g, &c, 100, -100, NULL, FRAME, 0.005, &s);
        MotionState r;
        mgyro_rest(&g, 0.003, &r);
        CHECK(!memcmp(r.dir, s.dir, sizeof r.dir) && !memcmp(r.acc, s.acc, sizeof r.acc) && r.gyro[0] == 0 &&
                  r.gyro[1] == 0 && r.gyro[2] == 0 && r.delta_ns == 3000000,
              "rest: the same attitude, no rotation, delta_ns as given");
        CHECK(fabs(sqrt(r.acc[0] * r.acc[0] + r.acc[1] * r.acc[1] + r.acc[2] * r.acc[2]) - 1) < 1e-6, "rest: 1 G");
    }

    // 6. The yaw of a frame (mgyro_frame_yaw): at most lim, the rest in the next frames, none lost; a reversal waits two
    // still reads unless the controller was still for two reads already; a random stream adds up.
    {
        MouseGyro g;
        mgyro_reset(&g, 0);
        double out = mgyro_frame_yaw(&g, 5000, 750, 2), total = out;
        CHECK(out == 750 && g.carry == 4250, "cap: 750 of 5000 in the first frame");
        for (int i = 0; i < 10; i++) total += mgyro_frame_yaw(&g, 0, 750, 2);
        CHECK(total == 5000 && g.carry == 0, "cap: the rest in the next frames, none lost");
        mgyro_reset(&g, 0);
        double a = mgyro_frame_yaw(&g, 20, 750, 2), b = mgyro_frame_yaw(&g, -20, 750, 2), c2 = mgyro_frame_yaw(&g, -20, 750, 2),
               d = mgyro_frame_yaw(&g, -20, 750, 2);
        CHECK(a == 20 && b == 0 && c2 == 0 && d == -60, "reverse: held for two reads, then all of it");
        mgyro_reset(&g, 0);
        mgyro_frame_yaw(&g, 20, 750, 2);
        mgyro_frame_yaw(&g, 0, 750, 2);
        mgyro_frame_yaw(&g, 0, 750, 2);
        CHECK(mgyro_frame_yaw(&g, -20, 750, 2) == -20, "reverse: after two still reads it goes out at once");
        mgyro_reset(&g, 0);
        mgyro_frame_yaw(&g, 20, 750, 0);
        CHECK(mgyro_frame_yaw(&g, -20, 750, 0) == -20, "reverse: hold 0 holds nothing");
        mgyro_reset(&g, 0);
        double in = 0, sent = 0;
        for (int i = 0; i < 5000; i++) {
            double dx = floor(rnd() * 41) - 20 + (i % 300 < 150 ? 6 : -6);
            in += dx;
            sent += mgyro_frame_yaw(&g, dx, 750, 2);
        }
        CHECK(fabs(sent + g.carry - in) < 1e-6, "stream: every count goes out or waits");
    }

    // 7. A PC game's sensitivity: 30 cm a turn at 800 DPI is 9449 counts; at a camera gain of 2.3933 the controller
    // turns 360 / 9449 / 2.3933 degrees a count.
    {
        double d = mgyro_cm360_deg_per_count(30, 800, 2.3933);
        CHECK(fabs(d - 360.0 / (30 / 2.54 * 800) / 2.3933) < 1e-12 && fabs(d - 0.0159193) < 1e-6, "cm360: 30 cm at 800 DPI");
        CHECK(mgyro_cm360_deg_per_count(0, 800, 2.3933) == 0, "cm360: 0 is off");
    }

    // 8. The ring the game reads: numbered one after another, newest first, the frame's state the newest.
    {
        MotionFusion *m = motion_new();
        MouseGyroCfg c = cfg(0, 2);
        MouseGyro g;
        mgyro_reset(&g, 0);
        uint64_t now = 1000000000ull;
        motion_restart(m);
        for (int i = 0; i < 40; i++) {
            if (i % 4 == 3) mgyro_frame(&g, &c, 5, 0, NULL, FRAME, 0.005, &s);
            else mgyro_rest(&g, 0.005, &s);
            motion_push(m, &s, now);
            now += 5000000;
        }
        MotionState r[16];
        int n = motion_read(m, r, 16, now);
        int ok = n == 16;
        for (int i = 1; i < n; i++) ok &= r[i - 1].sampling == r[i].sampling + 1 && r[i].attributes == 1;
        CHECK(ok && r[0].sampling == 40 && r[0].gyro[2] != 0 && r[1].gyro[2] == 0,
              "ring: 16 states, newest first, consecutive numbers, connected; the frame's state first");
        CHECK(motion_read(m, r, 16, now + 300000000ull) == 0, "ring: no state for 200 ms reads as no motion");
    }

    keys_check();
    printf("probe-mgyro: %s (%d failures)\n", fails ? "FAILED" : "passed", fails);
    return fails != 0;
}
