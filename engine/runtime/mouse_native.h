// Native mouse aiming for Splatoon 3 (TOTK_MOUSE_NATIVE, mouse_native.c, docs/controls.md "Mouse as the gyro"): while
// the mouse's virtual six-axis sensor is the one the game reads, the game's own gyro smoothing is switched off so each
// frame's mouse counts reach the aim whole in the frame that reads them, and the pitch turns by the yaw's degrees per
// count over its whole range. A real gamepad's gyro keeps the game's smoothing and pitch curve.
#pragma once

// ov_input.c six_fill, on every read of the connected controller's sensor: mouse is 1 when the states handed out are
// the mouse's. Switches the game's smoothing off or back as the source changes or the setting does.
void mouse_native_read(int mouse);
// 1 while the smoothing is off for the mouse (the last read handed out the mouse's states with the setting on).
int mouse_native_active(void);
// The last frame of the linear pitch, for the bench (mgyro_bench.c): the curve's own result, the pitch handed out,
// the pose's view pitch for the camera's pitch before the frame and the one wanted, the final view pitch aimed at
// (degrees, up positive), the yaw gain used (camera degrees per controller degree), the slope of the final view's
// offset from the pose's that the frame used, and how the frame went: 0 aimed at the view drawn, 1 a resync (the
// curve's result after the game's recentre, the pitch as it is otherwise), 2 and 3 a change of the camera that moved
// the pitch by the turn along the pose of the moment (2) or, during a change of form, along the table of the wide
// camera (3), 4 a narrow camera (squid form) whose turn moved the pitch along the wide camera's table. Then the
// camera's pitch before the frame, the change of the curve's input that counted as the mouse's (degrees of controller
// pitch), the view pitch drawn the frame before, the two bases, the virtual controller's pitch at the read, the view
// pitch the curve's own result would put the pose at minus the pose's view pitch of the pitch handed out (degrees;
// only while the bench runs: how far a frame that took the curve's result would move the view), and MOUSE_NATIVE_*
// flags of what acted.
enum {
    MOUSE_NATIVE_NEW = 1,         // resync: no previous frame (first frame, or the setting or the source came back)
    MOUSE_NATIVE_CAMERA = 2,      // resync: another camera object
    MOUSE_NATIVE_WRITTEN = 4,     // resync: the game wrote the pitch itself since the previous frame
    MOUSE_NATIVE_BASE = 8,        // resync: a base moved (the Y recentre, a reset)
    MOUSE_NATIVE_JUMP = 16,       // resync: the curve's input jumped 30 degrees past the virtual controller's own turn
    MOUSE_NATIVE_CHANGING = 32,   // the camera itself is changing
    MOUSE_NATIVE_BLEND = 64,      // a change of form: the turn moves the pitch along the table
    MOUSE_NATIVE_PINNED = 128,    // resting against a limit
    MOUSE_NATIVE_LIMIT = 256,     // the pitch handed out is at a limit: motion past it dropped
    MOUSE_NATIVE_TABLE = 512,     // the table was taken this frame
    MOUSE_NATIVE_OFF = 1024,      // the curve's result handed out as it is: not the mouse, the setting off, no gain yet
    MOUSE_NATIVE_NARROW = 2048,   // a narrow camera (squid form): the turn moves the pitch along the wide camera's
                                  // table
};
typedef struct {
    float game, out, view, want, target, gain, slope;
    int resync;
    float now, step, seen, base1, base2, ctrl, latent;
    int flags;
    unsigned calls;   // the camera's calls of its pitch curve so far (one a frame of the player camera)
} MouseNativePitch;
MouseNativePitch mouse_native_pitch_debug(void);
