// Native mouse aiming for Splatoon 3 (mouse_native.h). The mouse keeps turning the virtual six-axis controller of
// mouse_gyro.c (input.c input_mouse_gyro_read); while that controller is the sensor the game reads and
// TOTK_MOUSE_NATIVE is on, the three stages of the game's gyro processing that smooth a real controller's motion are
// switched off, so the camera takes every frame's counts whole in the frame that reads them, both axes, and the pitch
// turns by the yaw's own degrees per count over its whole range instead of along the game's pitch curve (4, below).
// A gamepad's gyro keeps all of them. Everything else stays the game's: its sensitivity, pitch limits, collisions, the
// camera modes, and what it sends to the other players.
//
// 1. The attitude filter (measured with TOTK_MGYRO_BENCH: an 8 degree pitch step of the controller moved the aim by
//    3%, 5% and 7% of its turn in the first three reads and 89% after nine). Once a frame the game's gyro processor
//    (main+0x2afaf6c, its work in main+0x2afb620) copies the new six-axis states into a ring and hands the cameras an
//    angular velocity and a direction matrix. The angular velocity is the newest state's; the matrix is a filtered
//    attitude: a state about two samples behind the newest picked by how well it continues the previous output, then
//    blended into the previous output carried forward by its angular velocity, with a weight that drops to 0.2 when
//    the picked state is far from that prediction and recovers by 0.2 a frame. Yaw is the angular velocity's part
//    about the matrix's up axis, so it was whole at once; pitch comes from the matrix itself. Two bytes of the game's
//    gyro tuning data choose the newest state's angular velocity and matrix instead (the first is set by default);
//    the gyro processor reads them every frame. While the mouse is the sensor the matrix byte is set, and the game's
//    value is put back when it is not.
// 2. The camera's yaw damping (the reversal easing of mouse_gyro.h mgyro_frame_yaw). The player camera
//    (main+0x28f4e20) multiplies each frame's yaw turn by a gain that drops, to 0.1 at least, when the turn follows
//    motion in the other direction and recovers over about ten frames, and keeps the part it cut to give back later.
//    The multiplication is replaced by a host call (recomp/patches-splatoon3-v1130.txt, set mouse_native): while the
//    mouse is the sensor the turn is taken whole and nothing is kept back; otherwise it multiplies as the game does.
// 3. The camera's pitch follow. The same camera maps the controller's pitch (plus the stick's) through its pitch curve
//    and limits to a target, then moves its pitch (the float at +0x16c, -1 down to 1 up) toward that target by a share
//    of the distance each frame: 0.3 plus or minus 0.08 times the motion sensitivity, less near the vertical, times a
//    gain that drops after motion the other way, as the yaw's (measured with the first stage off: 28% a frame, so a
//    step took seven frames to reach 90%). The share is a product the camera computes just before the move;
//    a second host call takes its place and makes it 1 while the mouse is the sensor, so the pitch is the target.
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "cfg.h"
#include "host.h"
#include "input.h"
#include "mgyro_bench.h"
#include "mouse_native.h"
#include "platform/platform.h"
#include "splat3_addr.h"

#if __has_include("patches.h")
#include "patches.h"
#endif

static volatile int32_t active;   // mouse_native_active: written on the game's six-axis reads, read by the hooks
static int supported = -1;    // the running build has the switch where its row says (checked once)
static volatile uint8_t *attitude_switch;
static uint8_t game_value;    // the switch's value before mouse_native set it
static float *pose_out;       // guest-visible scratch for the linear pitch: the camera pose's position and target

// The switch's address, once: the row's tuning data plus the byte offset of the gyro processor's load, which must be
// an ldrb, so a build whose code differs from the row is never written blind.
static int setup(void) {
    if (supported >= 0) return supported;
    supported = 0;
    const Splat3Addr *s3 = splat3_addr();
    if (!s3 || !s3->gyro_tuning || !s3->gyro_attitude_load || !ldr_count()) return 0;
    uint64_t base = ldr_module(0)->base, load = base + s3->gyro_attitude_load;
    uint32_t w = mem_guest_readable(load, 4) ? *(const uint32_t *)(uintptr_t)load : 0;
    if ((w & 0xffc00000u) != 0x39400000u) {   // ldrb wt, [xn, #imm]
        rt_log("mouse native: main does not have the expected gyro processor at main+%#llx; off",
               (unsigned long long)s3->gyro_attitude_load);
        return 0;
    }
    uint64_t a = base + s3->gyro_tuning + ((w >> 10) & 0xfff);
    if (!mem_guest_readable(a, 1)) return 0;
    attitude_switch = (volatile uint8_t *)(uintptr_t)a;
#ifdef RT_PATCH_PAGE
    if (s3->pitch_curve && s3->camera_pose) pose_out = mem_kernel_alloc(32, 16);   // linear pitch (below)
#endif
    supported = 1;
    return 1;
}

static int setting(void) {
    static unsigned seen = ~0u;
    static int on;
    unsigned v = cfg_version();
    if (v != seen) seen = v, on = cfg_bool("TOTK_MOUSE_NATIVE");
    return on;
}

// The virtual controller's pitch turns that followed the view (input_mouse_gyro_follow), in all, and its pitch, as of
// the state the game was handed at its last read of the mouse's sensor: the linear pitch (part 4) takes the turns out
// of the change of the curve's input, as they are no mouse motion. Read at the read itself, so they always go with the
// state the gyro processor took then.
static double followed_at_read, pitch_at_read;

void mouse_native_read(int mouse) {
    if (mouse) followed_at_read = input_mouse_gyro_followed(&pitch_at_read);
    int want = mouse && setting() && setup();
    if (want == __atomic_load_n(&active, __ATOMIC_RELAXED)) return;
    if (want) {
        game_value = *attitude_switch;
        *attitude_switch = 1;
    } else {
        *attitude_switch = game_value;
    }
    __atomic_store_n(&active, want, __ATOMIC_RELAXED);
    rt_log("mouse native: %s", want ? "on: the mouse's turns reach the aim unsmoothed"
                                    : "off: the game's gyro smoothing is back");
}

int mouse_native_active(void) { return __atomic_load_n(&active, __ATOMIC_RELAXED); }

#ifdef RT_PATCH_PAGE
#define HOST_MOUSE_NATIVE_YAW (RT_PATCH_PAGE + 0xf60)
#define HOST_MOUSE_NATIVE_PITCH (RT_PATCH_PAGE + 0xf50)
#define CAMERA_KEPT_YAW 0x1770

static float sreg(const Cpu *c, int r) {
    uint32_t bits = (uint32_t)c->v[r];
    float f;
    memcpy(&f, &bits, sizeof f);
    return f;
}

static void set_sreg(Cpu *c, int r, float f) {
    uint32_t bits;
    memcpy(&bits, &f, sizeof bits);
    c->v[r] = bits;   // a 32-bit write clears the rest of the vector register
}

// In place of the camera's `fmul s11, s12, s0` (main+0x28f6984): s12 the frame's yaw turn after the game's
// sensitivity, s0 the damping gain, s11 the turn the camera goes on with; x19 the camera, whose float at +0x1770 holds
// the part of earlier turns the damping kept back to give later.
static void yaw_turn(Cpu *c) {
    float turn = sreg(c, 12), out = turn * sreg(c, 0);
    if (mouse_native_active()) {
        out = turn;
        float zero = 0;
        uint64_t kept = c->x[19] + CAMERA_KEPT_YAW;
        if (mem_guest_readable(kept, sizeof zero)) memcpy((void *)(uintptr_t)kept, &zero, sizeof zero);
    }
    set_sreg(c, 11, out);
}

// In place of the camera's `fmul s1, s15, s2` (main+0x28f9ae0): the share of the distance to its pitch target the
// camera moves this frame, which the next instructions apply (pitch += (target - pitch) * s1).
static void pitch_share(Cpu *c) { set_sreg(c, 1, mouse_native_active() ? 1.0f : sreg(c, 15) * sreg(c, 2)); }

// ---- 4. Linear pitch. With the three stages off the pitch still comes from the controller's tilt through the game's
// own maps: the camera's pitch curve takes the tilt plus the stick's offset, in degrees, to a pitch from -1 (its
// lowest) to 1 (its highest), against a base that the Y recentre and pushes past a limit move; its camera pose places
// the camera and its target for that pitch; and the camera's later placement raises or lowers the target, part of it
// with a delay (measured: the target's height went on rising for three frames after the pitch turned back), so the
// view drawn is a little steeper or flatter than the pose's (measured with the curve: 0.098 degrees of view per count
// near level, falling to 0 at the top limit, up to 0.112 below level, while yaw is 0.0957 everywhere).
// While the mouse is the sensor the pitch moves instead by the yaw's own degrees per degree of controller turn, in
// degrees of the view drawn. Each frame the change of the curve's input since the previous frame (the mouse's tilt
// and the stick) is turned into degrees at the yaw's gain and added to the view pitch aimed at. The pose's view pitch
// to reach is the current one plus the distance from the view drawn last frame to that aim, divided by how much the
// view drawn moved per degree of the pose's in the previous frame, and the pitch whose pose has it replaces the
// curve's result. What the later placement adds is so taken back the next frame, and the view drawn ends where the
// counts put it, also while the later placement goes on settling after the mouse stopped (measured below 40 degrees
// down: up to 0.03 degrees a frame, for more than 20 frames); the offset's own drift in the previous frame is taken as
// going on in this one, so that settling is met in the frame it happens rather than a frame late. The aim stays within
// AIM_SPAN of the view
// drawn, is dropped when the view did not follow the pose, becomes the view drawn while the pitch rests against a
// limit, and once the frames since the curve's input last changed have moved the pose by IDLE_SPAN in all, so a move
// of the game's own camera is not fought for more than that and the pitch cannot wander while the mouse rests. Both
// maps are the game's functions, called for the camera's current parameters;
// nothing of them is copied here. The pitch stops at the pose's own ends (the curve's -1 and 1) and motion past them
// is dropped, so a turn back moves the view at once; the base is left to the game. When the game moves the pitch
// itself, the next frame aims from the view drawn: after the Y recentre (a base moved) the frame takes the curve's
// result and its excess as the game computed them (the game sets them to level); after a write to the pitch, another
// camera, the setting or the source coming back, or a jump of the curve's input that the virtual controller's own turn
// does not explain, it hands out the pitch as it is. (The virtual controller follows the view and not the curve, so the
// curve's result is as far from the view as the two have come apart, up to 42 degrees after a Y. Release 0.3.4 took
// it in all those frames and also on any change of the curve's input over 30 degrees, a vertical flick of that much in
// one frame or across a frame hitch: such a flick and its way back left the view 10 to 37 degrees up or down.)
// Without a measured yaw gain yet, or with the setting off, the curve's result is handed out as it is.
// A change of the camera itself (squid form and back, another camera blending in) changes the pose's view pitch of an
// unchanged pitch. The game keeps its pitch through it, so the view a pitch gives comes back with the form. While the
// pose moves at all, the aim starts again from the view drawn each frame and the game's own move of the view is left
// alone. A change of form is fast: 22 to 40 frames, the pose of the pitch handed out moving by 0.2 to 3.8 degrees a
// frame at its height (measured: below level the squid camera maps the pitch about as the human one does, above it to
// a view about 3.4 times narrower, its top near 16 degrees up). The pose's view pitch for 65 pitches is taken while the
// camera holds still (the table), and the last table of a wide camera, whose view rises by WIDE_SPAN at least from the
// level pitch to the top one (the human form's), is kept (ref). Once the pose of the pitch moves by more than BLEND_IN
// in a frame, or its slope there changes by more than BLEND_SLOPE of the table's in a frame (at a pitch both cameras
// put at about the same view, where the pose hardly moves), each of the mouse's turns moves the pitch along ref, at the
// slope last measured, until the camera holds still again for STEADY_READS frames (at most CHANGE_TABLE_MAX frames);
// and so does every turn while the camera is narrow (squid form). The human form's view so ends where the counts put
// it, whatever form they were made in, and a turn and its way back cancel on the pitch, as they do in the game with a
// real controller, whose tilt the pitch follows. The squid camera's view turns about 0.85 times the counts below
// level and about 3.4 times less above it, as with the game's own aiming. (Releases 0.3.3 and 0.3.4 turned the squid
// camera's view by the counts and, during a change, moved the pitch along the table of the camera before it: a turn up
// in squid form above level, or one made while the form changed back, then moved the human form's view about 3.4
// times as far once the form came back, and the camera looked up by itself when the player left squid form: 3 degrees
// of counts in squid form left the view 8 degrees higher than the counts put it, 12 degrees 31 higher, and 60 counts
// up in the half second after ZL was let go, 7 to 11 degrees higher. Turning along the pose of the moment moved the
// pitch further for the same counts near the squid camera's top than near the human one's: a small circle across each
// change moved the pitch by up to 0.14 and 17 changes each way took a level view to 55 degrees up. Holding the view
// instead drove the pitch from 0.48 to 1 and back to 0.83 in one change, and the view jumped by up to 26 degrees on
// the way back.) A slow change is another camera: climbing a wall moves the pose by at most 0.11 degrees a frame for
// seconds while its slope changes by a quarter in 16 frames (at most 3% a frame); there a turn moves the pose of the
// moment, so each turn moves the view drawn by its counts, unless the camera is narrow. The table is taken again once
// the camera has held still for STEADY_READS frames, or every TABLE_EVERY frames of a slow change.
// The virtual controller's tilt is no longer what the view follows, and motion dropped at a limit or a camera change
// left the two apart, so over a game the controller drifted to its own limit (mouse_gyro_pitch): there it stopped
// tilting that way, the pitch no longer answered the mouse in that direction, and past 84 degrees its yaw no longer
// turned the camera (the yaw goes on the controller's own vertical axis divided by the cosine of its pitch, which stops
// at 0.1); the game's Y also stopped levelling the view. So each frame the controller is asked to turn toward the
// aimed view's pitch over the yaw's gain (input_mouse_gyro_follow: at most 1 degree a read, toward a pitch at least 10
// degrees inside its limit, mouse_gyro.h MouseGyroFollow); those turns reach the curve's input like the mouse's and are
// taken out of the step (followed_at_read). The request is the difference left after the frame rather than a pitch to
// reach, so it is 0 while nothing is dropped and the controller turns with the mouse alone.
#define HOST_MOUSE_NATIVE_CURVE (RT_PATCH_PAGE + 0xf40)
#define HOST_MOUSE_NATIVE_GAIN (RT_PATCH_PAGE + 0xf30)
#define CAMERA_PITCH 0x16c       // the camera's pitch, -1 to 1
#define CAMERA_TURN_SCALE 0x64   // the camera update's stack slot with the scale of this frame's yaw turn

#define CAMERA_VIEW 0x54         // the camera's final view direction, from its target toward its position
#define AIM_SPAN 1.0             // degrees the view pitch aimed at may be away from the view drawn
#define IDLE_SPAN 2.0            // degrees of the pose's view pitch the aim may move it by while the mouse rests
#define POSE_MOVED 0.01          // degrees the pose's view pitch of an unchanged pitch moves by when the camera changes
#define DRIFT_MAX 0.2            // degrees a frame of the later placement's own drift taken into account
#define STEADY_READS 3           // frames without a move of the pose that end a change of the camera
#define BLEND_IN 0.15            // degrees a frame the pose of the pitch handed out moves by: a change of form begins
#define BLEND_SLOPE 0.06         // change a frame of the pose's slope over the table's at the pitch: one begins too
#define CHANGE_TABLE_MAX 45      // frames of a change of form at most
#define TABLE_EVERY 8            // frames between two takes of the table while the camera changes slowly
#define TABLE_N 64               // the table's intervals over the pitch's range
#define WIDE_SPAN 40.0           // degrees from the level pitch's view to the top pitch's: at least, a wide camera
                                 // (human form: 67; squid form: 20)
static volatile float yaw_gain;   // degrees of camera yaw per degree of controller yaw this frame; 0: not seen yet
// The previous frame's pitch: camera, pitch handed out, curve input and bases; the view pitch aimed at (when aimed),
// the view pitch drawn for the frame before, the pose's view pitch of that frame's pitch and of the pitch handed out;
// how far the aim moved the pose since the curve's input last changed; followed_at_read as of the previous frame; the
// slope of the view drawn over the pose's, last measured, and the one a change of form turns the table with; whether
// the camera is changing, the frames since that change began and those it has held still since; whether a change of
// form is on, whether one was during this change of the camera, and its frames; the frames since the table was taken
// (age and since stop at TABLE_EVERY); the pose's slope over the table's at the table's entry cell, last frame
static struct {
    uint64_t cam;
    float out, input, base1, base2;
    double target, seen, pose, handed, idle, followed, ctrl, slope, change_slope, ratio;
    int valid, aimed, changing, age, still, blend, blended, blend_age, since, cell;
} pz;
// The table: the pose's view pitch for pitch -1 + 2 i / TABLE_N, of camera cam as it last held still or changed slowly
// (ok: taken); check: the entry table_current compares last besides the one nearest the pitch. ref: the last table of
// a wide camera (the human form's), which every turn follows while the camera is narrow or changes form.
typedef struct {
    double v[TABLE_N + 1];
    uint64_t cam;
    int ok, check;
} Table;
static Table tab, ref;

// The view pitch in degrees (up positive) the camera drew in its last frame, from its final view direction.
static float final_view_pitch(uint64_t cam) {
    float v[3];
    memcpy(v, (const void *)(uintptr_t)(cam + CAMERA_VIEW), sizeof v);
    double n = sqrt((double)v[0] * v[0] + (double)v[1] * v[1] + (double)v[2] * v[2]);
    return n > 0 ? (float)(-asin(fmax(-1.0, fmin(1.0, v[1] / n))) * (180.0 / M_PI)) : 0;
}
static MouseNativePitch dbg;

// In place of the camera's `fmul s12, s8, s9` (main+0x28f66f8): s8 the yaw gain of the motion sensitivity, s9 the
// frame's yaw rate, s12 the turn. The gyro processor scales the angular velocity by pi/54, the camera turns s8 times
// the stack slot's scale radians per unit of that, and the virtual controller reports a frame's turn as an angular
// velocity over 1/60 s: a degree of controller yaw turns the camera (pi/54) (60/360) (180/pi) s8 scale = 5/9 s8 scale
// degrees (measured: 2.393 at the bench's sensitivity, the gain the bench's yaw circles fit).
static void yaw_gain_seen(Cpu *c) {
    float s8 = sreg(c, 8), scale;
    memcpy(&scale, (const void *)(uintptr_t)(c->sp + CAMERA_TURN_SCALE), sizeof scale);
    yaw_gain = s8 * scale * (5.0f / 9.0f);
    set_sreg(c, 12, s8 * sreg(c, 9));
}

// The view pitch in degrees (up positive) of the camera's pose for pitch n: the game's pose function, called with the
// camera, two vectors for its position and target, and n.
static double view_pitch(Cpu *c, uint64_t cam, float n) {
    uint64_t pose = ldr_module(0)->base + splat3_addr()->camera_pose;
    set_sreg(c, 0, n);
    rt_call(c, pose, 4, cam, (uint64_t)(uintptr_t)pose_out, (uint64_t)(uintptr_t)(pose_out + 4), (uint64_t)0);
    double dx = pose_out[4] - pose_out[0], dy = pose_out[5] - pose_out[1], dz = pose_out[6] - pose_out[2];
    return atan2(dy, sqrt(dx * dx + dz * dz)) * (180.0 / M_PI);
}

// The pitch in [-1, 1] whose view pitch is want: false position with the Illinois step, from the current pitch n0
// (view pitch p0) and the ends; the view pitch rises with the pitch.
static float pitch_for(Cpu *c, uint64_t cam, double want, float n0, double p0) {
    double plo = view_pitch(c, cam, -1.0f), phi = view_pitch(c, cam, 1.0f);
    if (want <= plo) return -1.0f;
    if (want >= phi) return 1.0f;
    double a = -1, b = 1, pa = plo, pb = phi;
    if (n0 > -1 && n0 < 1) {
        if (p0 <= want) a = n0, pa = p0;
        else b = n0, pb = p0;
    }
    int kept = 0;
    for (int i = 0; i < 30; i++) {
        double m = a + (want - pa) * (b - a) / (pb - pa);
        if (!(m > a && m < b)) m = 0.5 * (a + b);
        double pm = view_pitch(c, cam, (float)m);
        if (fabs(pm - want) < 1e-4 || b - a < 1e-6) return (float)m;
        if (pm < want) {
            a = m, pa = pm;
            if (kept == -1) pb = want + (pb - want) * 0.5;
            kept = -1;
        } else {
            b = m, pb = pm;
            if (kept == 1) pa = want + (pa - want) * 0.5;
            kept = 1;
        }
    }
    return (float)(0.5 * (a + b));
}

static float table_n(int i) { return (float)(-1.0 + 2.0 * i / TABLE_N); }

// A wide camera: from the level pitch to the top one its view rises by WIDE_SPAN at least.
static int table_wide(const Table *t) { return t->ok && t->v[TABLE_N] - t->v[TABLE_N / 2] >= WIDE_SPAN; }

static void table_take(Cpu *c, uint64_t cam) {
    for (int i = 0; i <= TABLE_N; i++) tab.v[i] = view_pitch(c, cam, table_n(i));
    tab.cam = cam, tab.ok = 1;
    if (table_wide(&tab)) ref = tab;
}

// The table's entry nearest pitch n, not an end; the pose's slope over the table's across the two intervals around
// entry i (1: as the table has it).
static int table_near(float n) {
    int i = (int)lround((n + 1.0) * (TABLE_N / 2.0));
    return i < 1 ? 1 : i > TABLE_N - 1 ? TABLE_N - 1 : i;
}

static double slope_ratio(Cpu *c, uint64_t cam, int i) {
    double t = tab.v[i + 1] - tab.v[i - 1];
    return fabs(t) > 1e-6 ? (view_pitch(c, cam, table_n(i + 1)) - view_pitch(c, cam, table_n(i - 1))) / t : 1;
}

// Whether the table is the camera's as it is now: the entry nearest pitch n and one more, a different one each frame,
// are what the pose gives (the same function of the same camera gives the same value).
static int table_current(Cpu *c, uint64_t cam, float n) {
    if (!tab.ok || tab.cam != cam) return 0;
    int i = (int)lround((n + 1.0) * (TABLE_N / 2.0)), j = tab.check = (tab.check + 1) % (TABLE_N + 1);
    i = i < 0 ? 0 : i > TABLE_N ? TABLE_N : i;
    return fabs(view_pitch(c, cam, table_n(i)) - tab.v[i]) <= POSE_MOVED &&
           fabs(view_pitch(c, cam, table_n(j)) - tab.v[j]) <= POSE_MOVED;
}

// A table's view pitch for pitch n, and the pitch for view pitch v (the view pitch rises with the pitch; past either
// end, that end), both straight between the entries, so one undoes the other.
static double table_view(const Table *t, double n) {
    double x = (n + 1.0) * (TABLE_N / 2.0);
    int i = x <= 0 ? 0 : x >= TABLE_N - 1 ? TABLE_N - 1 : (int)x;
    return t->v[i] + (t->v[i + 1] - t->v[i]) * (x - i);
}

static double table_pitch(const Table *t, double v) {
    if (v <= t->v[0]) return -1;
    if (v >= t->v[TABLE_N]) return 1;
    int lo = 0, hi = TABLE_N;
    while (hi - lo > 1) {
        int m = (lo + hi) / 2;
        if (t->v[m] <= v) lo = m;
        else hi = m;
    }
    double d = t->v[hi] - t->v[lo];
    return -1.0 + 2.0 * (lo + (d > 0 ? (v - t->v[lo]) / d : 0)) / TABLE_N;
}

// In place of the camera's `bl` to its pitch curve (main+0x28f7560 and +0x28f7660): s0 the motion sensitivity, s1 the
// curve's input (tilt plus stick offset, degrees), s2 and s3 the bases, w0 and w1 flags, x2 where the excess past a
// limit goes; the result in s0. The curve is always called as the game would; x19 is the camera.
static void pitch_curve(Cpu *c) {
    Cpu saved;
    memcpy(&saved, c, sizeof saved);
    const Splat3Addr *s3 = splat3_addr();
    float input = sreg(c, 1), base1 = sreg(c, 2), base2 = sreg(c, 3);
    uint64_t cam = c->x[19], excess_at = c->x[2];
    rt_call(c, ldr_module(0)->base + s3->pitch_curve, 3, c->x[0], c->x[1], c->x[2]);
    float game = sreg(c, 0), out = game;
    int on = mouse_native_active() && yaw_gain > 0 && pose_out, keep = mgb_native_keep(1);
    dbg.game = game, dbg.resync = 1, dbg.gain = yaw_gain, dbg.flags = 0, dbg.latent = 0, dbg.calls++;
    dbg.base1 = base1, dbg.base2 = base2, dbg.ctrl = (float)pitch_at_read;
    if (on) {
        float now;
        memcpy(&now, (const void *)(uintptr_t)(cam + CAMERA_PITCH), sizeof now);
        // the change of the curve's input since the previous frame, less the virtual controller's turns that followed
        // the view (they are no mouse motion)
        double followed = followed_at_read, step = (double)input - pz.input - (followed - pz.followed);
        // A jump of the curve's input that the virtual controller's own turn does not explain (the mouse's: its pitch
        // change less the follow's): a reset of the game's sensor processing. A mouse turn of any size is motion.
        // 0.3.4 took any change over 30 degrees for one, so a vertical flick of that much in a frame (750 counts at
        // 0.04 degrees per count, or fewer counts over a frame hitch) resynced.
        double own = pitch_at_read - pz.ctrl - (followed - pz.followed);
        int hold = mgb_native_hold(1);
        int jump = pz.valid && (hold ? fabs(step - own) > 30 : fabs(step) > 30);
        int why = (!pz.valid ? MOUSE_NATIVE_NEW : 0) | (pz.valid && pz.cam != cam ? MOUSE_NATIVE_CAMERA : 0) |
                  (pz.valid && fabsf(now - pz.out) > 1e-5f ? MOUSE_NATIVE_WRITTEN : 0) |
                  (pz.valid && (base1 != pz.base1 || base2 != pz.base2) ? MOUSE_NATIVE_BASE : 0) |
                  (jump ? MOUSE_NATIVE_JUMP : 0);
        int resync = why != 0;
        double seen = final_view_pitch(cam);
        dbg.flags = why, dbg.now = now, dbg.step = (float)step, dbg.seen = (float)seen;
        if (!pz.valid) pz.changing = pz.blend = pz.blended = 0;
        if (resync) {
            pz.aimed = 0;
            // Only the game's recentre (a base moved) takes the curve's result. Otherwise the pitch stays as it is: the
            // virtual controller follows the view and not the curve, so the curve's result is as far from the view as
            // the two have come apart (measured: up to 42 degrees after a Y), which 0.3.4 turned the view by.
            if (hold && !(why & MOUSE_NATIVE_BASE)) {
                out = now;
                float zero = 0;
                memcpy((void *)(uintptr_t)excess_at, &zero, sizeof zero);
                // One log line per such frame, the first 16 and then one in 256 (a player's log shows where the game
                // moves the pitch itself): what moved it and how far the curve's result was from the view kept.
                static unsigned kept;
                if (why != MOUSE_NATIVE_NEW && (kept < 16 || kept % 256 == 0)) {
                    static const struct { int bit; const char *what; } names[] = {
                        {MOUSE_NATIVE_CAMERA, "another camera"}, {MOUSE_NATIVE_WRITTEN, "the game's write"},
                        {MOUSE_NATIVE_JUMP, "a jump of the curve's input"}, {MOUSE_NATIVE_NEW, "a restart"}};
                    char list[120] = "";
                    for (size_t i = 0; i < sizeof names / sizeof *names; i++)
                        if (why & names[i].bit)
                            snprintf(list + strlen(list), sizeof list - strlen(list), "%s%s", list[0] ? ", " : "",
                                     names[i].what);
                    rt_log("mouse native: pitch kept through %s (the curve's result %+.1f degrees of view away)", list,
                           view_pitch(c, cam, game) - view_pitch(c, cam, now));
                }
                if (why != MOUSE_NATIVE_NEW) kept++;
            }
        } else {
            // The view drawn last frame for the current pitch and the pose's view pitch for it. When the pose's view
            // pitch for that pitch is not the one it had when the pitch was handed out, the camera itself changed (a
            // change of form, another camera blending in): the game's pitch is kept and only the mouse's turn moves
            // it, along the table during a change of form, otherwise along the pose of the moment from the view drawn,
            // so the game's own move of the view is left alone.
            double p0 = view_pitch(c, cam, now), drift = 0, shift = keep ? fabs(p0 - pz.handed) : 0;
            if (shift > POSE_MOVED) {
                if (!pz.changing) pz.changing = 1, pz.age = 0, pz.change_slope = pz.aimed ? pz.slope : 0;
                pz.still = 0;
            } else if (pz.changing && ++pz.still >= STEADY_READS) {
                pz.changing = 0;
            }
            if (pz.changing && pz.age < TABLE_EVERY) pz.age++;
            if (!pz.changing) pz.blended = 0, pz.ratio = 1, pz.cell = table_near(now);
            if (pz.blend) {
                if (!pz.changing || ++pz.blend_age > CHANGE_TABLE_MAX || tab.cam != cam) pz.blend = 0;
            } else if (pz.changing && !pz.blended && tab.ok && tab.cam == cam) {
                // a change of form: the pose of the pitch moves fast, or its slope changes fast where both cameras
                // put that pitch at about the same view (the slope compared at the same entry as the frame before)
                double r = slope_ratio(c, cam, pz.cell);
                if (shift > BLEND_IN || fabs(r - pz.ratio) > BLEND_SLOPE) pz.blend = pz.blended = 1, pz.blend_age = 0;
                int i = table_near(now);
                pz.ratio = i == pz.cell ? r : slope_ratio(c, cam, i), pz.cell = i;
            }
            if (pz.since < TABLE_EVERY) pz.since++;
            if (keep && !pz.blend && (!pz.changing || (pz.age >= TABLE_EVERY && pz.since >= TABLE_EVERY)) &&
                !table_current(c, cam, now)) {
                table_take(c, cam), pz.since = 0, pz.ratio = 1, pz.cell = table_near(now);
                dbg.flags |= MOUSE_NATIVE_TABLE;
            }
            // The table a turn follows while the camera changes form or is narrow (squid form): the wide camera's, so
            // a turn made in squid form or while the form changes moves the human form's view by its counts.
            int use_ref = keep && mgb_native_ref(1) && ref.ok && ref.cam == cam;
            const Table *bt = use_ref ? &ref : &tab;
            int narrow = use_ref && !pz.blend && tab.ok && tab.cam == cam && !table_wide(&tab);
            dbg.flags |= (pz.changing ? MOUSE_NATIVE_CHANGING : 0) | (pz.blend ? MOUSE_NATIVE_BLEND : 0) |
                         (narrow ? MOUSE_NATIVE_NARROW : 0);
            if (pz.blend || narrow) {
                // the turn moves the pitch along the table (during a change of form at the slope the camera had
                // before it); the aim, for the follow, is the wide camera's view of the pitch handed out, or the view
                // drawn next
                double turn = (double)yaw_gain * step / (pz.blend ? 1 + pz.change_slope : 1);
                out = step != 0 ? (float)table_pitch(bt, table_view(bt, now) + turn) : now;
                pz.handed = out == now ? p0 : view_pitch(c, cam, out);
                pz.aimed = 0, pz.slope = 0;
                pz.target = use_ref ? table_view(&ref, out) : seen + (pz.handed - p0);
                dbg.view = (float)p0, dbg.want = (float)pz.handed, dbg.target = (float)pz.target, dbg.slope = 0;
                dbg.resync = pz.blend ? 3 : 4;
            } else {
                if (pz.changing) pz.aimed = 0, dbg.resync = 2;
                // How the offset between the two moved since the previous frame: per degree of the pose's when that
                // frame moved the pose enough to tell (the slope, kept until measured again), otherwise by itself (the
                // drift of the later placement's slow settling, which goes on into the next frame: measured below 40
                // degrees down, up to 0.04 degrees a frame).
                if (pz.aimed) {
                    double dp = p0 - pz.pose, moved = (seen - p0) - (pz.seen - pz.pose);
                    if (fabs(dp) > 0.5) {
                        double s = moved / dp;
                        if (fabs(s) > 0.5) pz.aimed = 0;   // the view did not follow the pose: the game moved it
                        else pz.slope = fmax(-0.15, fmin(0.15, s));
                    } else {
                        drift = fmax(-DRIFT_MAX, fmin(DRIFT_MAX, moved - pz.slope * dp));
                    }
                }
                if (!pz.aimed) pz.slope = 0, drift = 0;
                double slope = pz.slope;
                int pinned = (now >= 1.0f && step >= 0) || (now <= -1.0f && step <= 0);   // resting against a limit
                if (step != 0) pz.idle = 0;
                if (!pz.aimed || pz.idle >= IDLE_SPAN || pinned) pz.target = seen, pz.aimed = 1;
                pz.target = fmax(seen - AIM_SPAN, fmin(seen + AIM_SPAN, pz.target)) + (double)yaw_gain * step;
                double want = p0 + (pz.target - seen - (pinned ? 0 : drift)) / (1 + slope);
                if (step == 0) pz.idle += fabs(want - p0);
                out = now, pz.handed = p0;
                if (fabs(want - p0) > 1e-4) {
                    out = pitch_for(c, cam, want, now, p0);
                    pz.handed = view_pitch(c, cam, out);
                    if (out <= -1.0f || out >= 1.0f)   // at a limit: the motion past it is dropped
                        pz.target = seen + (pz.handed - p0) * (1 + slope), dbg.flags |= MOUSE_NATIVE_LIMIT;
                }
                if (pinned) dbg.flags |= MOUSE_NATIVE_PINNED;
                dbg.view = (float)p0, dbg.want = (float)want, dbg.target = (float)pz.target, dbg.slope = (float)slope;
                if (dbg.resync == 1) dbg.resync = 0;
            }
            pz.seen = seen, pz.pose = p0;
            float zero = 0;
            memcpy((void *)(uintptr_t)excess_at, &zero, sizeof zero);   // nothing past a limit: the base stays
        }
        if (resync) pz.handed = view_pitch(c, cam, out);   // the curve's result, as the pose has it now
        // The virtual controller follows the view, so its tilt stays the view's aim over the yaw's gain and never runs
        // into its own limit while the view is elsewhere: the turn asked for is what is left between the two after this
        // frame (the aim includes the frame's mouse turn, as the controller's pitch at the read does).
        if (keep) input_mouse_gyro_follow((resync ? seen : pz.target) / yaw_gain - pitch_at_read);
        // the bench: how far the view would move in a frame that took the curve's result instead (a resync)
        if (mgb_on()) dbg.latent = (float)(view_pitch(c, cam, game) - pz.handed);
        pz.cam = cam, pz.out = out, pz.input = input, pz.base1 = base1, pz.base2 = base2, pz.valid = 1;
        pz.ctrl = pitch_at_read;
    } else {
        pz.valid = 0;
        dbg.flags = MOUSE_NATIVE_OFF;
    }
    pz.followed = followed_at_read;
    dbg.out = out;
    memcpy(c, &saved, sizeof *c);
    set_sreg(c, 0, out);
}

MouseNativePitch mouse_native_pitch_debug(void) { return dbg; }

__attribute__((constructor)) static void mouse_native_register(void) {
    rt_register_host(HOST_MOUSE_NATIVE_YAW, yaw_turn);
    rt_register_host(HOST_MOUSE_NATIVE_PITCH, pitch_share);
    rt_register_host(HOST_MOUSE_NATIVE_CURVE, pitch_curve);
    rt_register_host(HOST_MOUSE_NATIVE_GAIN, yaw_gain_seen);
}
#else
MouseNativePitch mouse_native_pitch_debug(void) { return (MouseNativePitch){0}; }
#endif
