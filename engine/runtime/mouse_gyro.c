// The mouse as the six-axis sensor (mouse_gyro.h). Maths adapted in approach from weeping-of-the-empire's
// pc_mouse_gyro.h @ engine/runtime (MIT, Copyright (c) 2026 daniel-madrid-07): counts to a rate about the controller's
// yaw and pitch axes, the attitude kept in step. Here the attitude is built from yaw and pitch directly and the body-frame
// angular velocity is derived from it exactly, so integrating the reported rate reproduces the reported matrix.
//
// Attitude B = Rz(yaw) Rx(pitch) B0: columns are the controller's x, y, z axes in the world frame (x right, y forward,
// z up). B0 is the start pose: flat (the identity: face up, top edge forward) or upright (face toward the player, top
// edge up: x -> x, y -> up, z -> back). Pitch > 0 looks up. Body angular velocity: the yaw rate about world up, whose
// body coordinates are B^T z, plus the pitch rate about the world's x axis at the start pose, B0^T x (Rz commutes with
// the world up axis, and Rx(pitch) with the world x axis).
#include <math.h>
#include <string.h>

#include "mouse_gyro.h"

#define TWO_PI 6.283185307179586

// B0[j][i] = world component j of controller axis i.
static void start_pose(int upright, double B0[3][3]) {
    memset(B0, 0, 9 * sizeof(double));
    if (!upright) {
        B0[0][0] = B0[1][1] = B0[2][2] = 1;
        return;
    }
    B0[0][0] = 1;    // x -> world x
    B0[2][1] = 1;    // y -> world up
    B0[1][2] = -1;   // z -> world back
}

void mgyro_attitude(double yaw, double pitch, int upright, double B[3][3]) {
    double B0[3][3], cy = cos(yaw), sy = sin(yaw), cp = cos(pitch), sp = sin(pitch);
    start_pose(upright, B0);
    // Rz(yaw) Rx(pitch)
    const double R[3][3] = {{cy, -sy * cp, sy * sp}, {sy, cy * cp, -cy * sp}, {0, sp, cp}};
    for (int i = 0; i < 3; i++)
        for (int j = 0; j < 3; j++) {
            double v = 0;
            for (int q = 0; q < 3; q++) v += R[i][q] * B0[q][j];
            B[i][j] = v;
        }
}

void mgyro_reset(MouseGyro *g, int upright) {
    memset(g, 0, sizeof *g);
    g->upright = upright;
}

void mgyro_recenter(MouseGyro *g) { g->yaw = g->pitch = 0; }

// The state for a turn already made: the attitude, the angular velocity w (rad/s, body axes) and the angle advanced by
// w times frame_s; delta_ns is dt. Acceleration: gravity as the accelerometer reads it, the world's down in controller
// axes.
static void fill(MouseGyro *g, const double w[3], double frame_s, double dt, MotionState *out) {
    double B[3][3];
    mgyro_attitude(g->yaw, g->pitch, g->upright, B);
    memset(out, 0, sizeof *out);
    out->delta_ns = (int64_t)(dt * 1e9 + 0.5);
    for (int i = 0; i < 3; i++) {
        double rps = w[i] / TWO_PI;
        g->angle[i] += rps * frame_s;
        out->gyro[i] = (float)rps;
        out->angle[i] = (float)g->angle[i];
        out->acc[i] = (float)-B[2][i];
        for (int j = 0; j < 3; j++) out->dir[i * 3 + j] = (float)B[j][i];
    }
}

void mgyro_frame(MouseGyro *g, const MouseGyroCfg *c, double dx, double dy, const MouseGyroFollow *follow,
                 double frame_s, double dt, MotionState *out) {
    const double k = c->deg_per_count * M_PI / 180.0;
    double dyaw = -dx * k * c->scale_x;
    double dpitch = -dy * k * c->scale_y * (c->invert_y ? -1 : 1);
    double lim = c->pitch_limit_deg * M_PI / 180.0;
    if (lim < 0) lim = 0;
    double pitch = g->pitch + dpitch;
    if (pitch > lim) pitch = lim;
    if (pitch < -lim) pitch = -lim;
    if (follow && follow->rate_deg > 0) {
        double rate = follow->rate_deg * M_PI / 180.0, span = follow->span_deg * M_PI / 180.0;
        double goal = pitch + follow->delta_deg * M_PI / 180.0;
        if (goal > span) goal = span;
        if (goal < -span) goal = -span;
        double d = goal - pitch, next = pitch + (d > rate ? rate : d < -rate ? -rate : d);
        if (next > lim) next = lim;
        if (next < -lim) next = -lim;
        g->followed += (next - pitch) * 180.0 / M_PI;
        pitch = next;
    }
    dpitch = pitch - g->pitch;
    if (g->upright != c->upright) g->upright = c->upright;
    double B0[3][3], mid[3][3], w[3];
    start_pose(g->upright, B0);
    mgyro_attitude(g->yaw + dyaw / 2, g->pitch + dpitch / 2, g->upright, mid);
    // yaw about the world's up (its body coordinates at the middle of the turn), or about the controller's own axis that
    // is up when level (the start pose's), as is (1) or divided by the cosine of the pitch (2) so its part about the
    // world's up is the turn; pitch about the controller's x
    double cp = cos(g->pitch + dpitch / 2), ys = c->yaw_body == 2 && cp > 0.1 ? 1 / cp : 1;
    for (int i = 0; i < 3; i++)
        w[i] = frame_s > 0 ? (dyaw * (c->yaw_body ? ys * B0[2][i] : mid[2][i]) + dpitch * B0[0][i]) / frame_s : 0;
    g->yaw += dyaw;
    g->pitch = pitch;
    if (g->yaw > M_PI) g->yaw -= TWO_PI;
    if (g->yaw < -M_PI) g->yaw += TWO_PI;
    fill(g, w, frame_s, dt, out);
}

void mgyro_rest(MouseGyro *g, double dt, MotionState *out) {
    const double w[3] = {0, 0, 0};
    fill(g, w, 0, dt, out);
}

double mgyro_frame_yaw(MouseGyro *g, double dx, double lim, int hold) {
    double total = g->carry + dx;
    int s = total > 1e-9 ? 1 : total < -1e-9 ? -1 : 0;
    if (s && g->last_sign && s != g->last_sign && g->still < hold) {   // a reversal: still reads first
        g->carry = total;
        g->still++;
        return 0;
    }
    double out = total > lim ? lim : total < -lim ? -lim : total;
    g->carry = total - out;
    if (out > 1e-9 || out < -1e-9) {
        g->last_sign = out > 0 ? 1 : -1;
        g->still = 0;
    } else {
        g->still++;
    }
    return out;
}

double mgyro_cm360_deg_per_count(double cm360, double dpi, double game_gain) {
    return cm360 > 0 && dpi > 0 && game_gain > 0 ? 360.0 / (cm360 / 2.54 * dpi) / game_gain : 0;
}
