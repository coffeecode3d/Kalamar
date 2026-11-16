// The mouse as the six-axis sensor (Splatoon 3's motion aiming): mouse counts turn a virtual controller, whose states
// carry the angular velocity of that turn and the attitude (direction matrix, gravity) it leads to.
// Pure maths, no window system or SDL: input.c feeds it, mouse_gyro_probe.c tests it.
#pragma once
#include <stdint.h>

#include "input.h"

typedef struct {
    double deg_per_count;       // yaw and pitch at scale 1, in degrees of controller turn per mouse count
    double scale_x, scale_y;    // multiplies each axis
    int invert_y;               // mouse up looks down
    double pitch_limit_deg;     // the controller never tilts further than this from level, up or down
    int upright;                // 0: held flat (face up, nose forward); 1: held upright facing the player, as in handheld mode
    int yaw_body;               // mgyro_frame: the yaw rate about the world's up (0), the controller's own vertical axis
                                // (1), or that axis divided by the cosine of the pitch (2)
} MouseGyroCfg;

// The virtual controller: yaw about the world's up axis, pitch about its own right axis (limited), both from the start
// pose. World axes: x right, y forward, z up. Controller axes (nn::hid): x right, y toward the top edge, z out of the face.
// carry, last_sign, still: mgyro_frame_yaw's yaw counts waiting, the sign of the last frame that turned, and the frames
// sent without a turn since. followed: degrees of pitch mgyro_frame turned the controller by to follow (below), in all.
typedef struct {
    double yaw, pitch;
    double angle[3];
    int upright;
    double carry;
    int last_sign, still;
    double followed;
} MouseGyro;

// A turn of the controller's pitch besides the mouse's (mouse_native.c, while it makes the pitch linear, so the
// controller follows the view): toward the pitch delta_deg away, or the nearest within span_deg of level, at most
// rate_deg in one frame; so a controller the mouse took further than span_deg is brought back inside. rate_deg 0: none.
typedef struct { double delta_deg, rate_deg, span_deg; } MouseGyroFollow;

enum { MOUSE_GYRO_RATE_HZ = 200 };   // the Switch's six-axis rate: one state per 5 ms
enum { MOUSE_GYRO_GAME_HZ = 60 };    // Splatoon 3 multiplies the newest angular velocity by 1/60 s each frame

void mgyro_reset(MouseGyro *g, int upright);                 // level, yaw 0, integrated angle 0
void mgyro_recenter(MouseGyro *g);                           // level again, yaw 0 (the angle keeps counting)
// The controller's three axes in the world frame (columns of the attitude matrix) for a yaw, pitch and holding.
void mgyro_attitude(double yaw, double pitch, int upright, double B[3][3]);

// Splatoon 3 turns its camera once a frame by the angular velocity of the newest state it reads times a fixed 1/60 s
// (yaw: its part about gravity), and by the change of the controller's tilt in the direction matrix (pitch); it ignores
// the older states and delta_ns (measured with TOTK_MGYRO_BENCH, docs/controls.md). So the state made at the game's
// read carries every count since the previous read: the controller turns by them and the angular velocity is that turn
// over frame_s (1/60), so the game turns by exactly the counts. dx, dy: raw counts (dy > 0 is down); dt: the time since
// the previous state (its delta_ns). Fills out->{delta_ns, acc, gyro, angle, dir}; the sampling number and attributes
// are the caller's (motion_push). Angular velocity in revolutions per second, acceleration in G as gravity reads,
// dir[i * 3 + j] = the j-th world component of controller axis i (the fusion's convention). follow: NULL or a pitch the
// controller turns toward after the mouse's counts; that turn is part of the state's angular velocity and adds to
// g->followed.
void mgyro_frame(MouseGyro *g, const MouseGyroCfg *c, double dx, double dy, const MouseGyroFollow *follow,
                 double frame_s, double dt, MotionState *out);
// A state between the game's reads: the attitude as it is, no angular velocity.
void mgyro_rest(MouseGyro *g, double dt, MotionState *out);
// The yaw counts the state of this read carries: dx and the counts waiting, at most lim either way (the game takes a
// frame's turn modulo a whole turn), the rest waiting for the next read. The game eases a turn that reverses the last
// one in over about 10 frames unless the controller was still for 2 of its reads (measured: 54% of a 10-frame leg
// turned); so a reversal is held for hold reads that report the controller still, and then goes out whole. With
// mouse_native on (mouse_native.c) the game's easing is off while the mouse is the sensor.
double mgyro_frame_yaw(MouseGyro *g, double dx, double lim, int hold);
// Degrees of controller turn per count for a PC game's sensitivity: cm of mouse travel per whole turn of the camera at
// dpi, the game turning game_gain times the controller. 0 for cm360 or dpi not above 0.
double mgyro_cm360_deg_per_count(double cm360, double dpi, double game_gain);
