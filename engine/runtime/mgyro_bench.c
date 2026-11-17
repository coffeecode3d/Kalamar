// TOTK_MGYRO_BENCH=<tests>: a test bench for the mouse as the six-axis sensor (mgyro_bench.h). A simulated mouse sends
// counts into the motion thread where the real mouse's raw counts enter it (input.c mouse_gyro_collect), so the counts
// go through the same conversion, sample grid and sensor ring as a player's. The game's aim is read from its memory: at
// the start the bench finds the unit vectors in the game's heap that turn with the virtual controller (a scan of every
// float triple of length 1, then three turns that must move them by consistent angles about one fixed axis), and from
// then on each six-axis read of the game logs those vectors with the controller's own yaw and pitch.
//
// <tests>: "all" or a list (commas or spaces) of look, idle, lin, step, phase, rate, rt, fields, pitchyaw, yawaxis,
// reverse, cap, recenter, limits, hitch, fps, image, circle, vsine, squid, drift, ink, wall, pokecmp, native, sweep,
// surface, surfacenew, surfacegame, swim, situ1, situ2, flick, walk; the finder always runs first, after a Y that
// recentres the camera. "match" or "matchold" alone runs the match test without the finder, "respawn", "respawnmove",
// "respawnwalk", "respawnrun" or "respawnrunold" alone a respawn test (a match too). TOTK_MGYRO_BENCH_OBJ,
// _FIELDS and _POKE: below (watches, pokes).
// TOTK_MGYRO_BENCH_OUT: the CSV file (default mgyro-bench.csv);
// TOTK_MGYRO_BENCH_DELAY: seconds after the first live six-axis read before the bench starts (default 40, the test
// range is up by then); TOTK_MGYRO_BENCH_EXIT=1 closes the game when the tests are done. tools/mgyro_bench.py turns the
// CSV into the tables of docs/controls.md.
//
// CSV lines (times in ns of kern_now_ns, angles in degrees, counts as the mouse sends them: x right, y down):
//   H,<key>,<value>                                  bench settings
//   E,<t>,<read>,<text>                              a test's step ("lin axis=x v=1200 n=1200", "end", ...)
//   T,<t>,<dx>,<dy>                                  counts the motion thread took from the simulated mouse
//   S,<sampling>,<t>,<yaw>,<pitch>,<n>               a sample pushed: the controller's attitude in it, n samples in its tick
//   R,<read>,<t>,<sampling>,<new>,<present>,<yaw>,<pitch>,<inj x>,<inj y>,<aim vectors x,y,z...>
//                                                    a six-axis read: the newest sample handed to the game, how many
//                                                    are new since the last read, the controller in that sample, every
//                                                    count sent so far, and the aim vectors now
//   P,<present>,<t>,<slot>,<read>                    the game's present call and the display time it is held to
//   F,<slot>,<t>                                     the host present of that frame
//   W,<read>,<values>                                the watched fields (TOTK_MGYRO_BENCH_FIELDS)
//   N,<read>,<curve>,<out>,<view>,<want>,<target>,<gain>,<slope>,<resync>,<now>,<step>,<seen>,<base1>,<base2>,<ctrl>,
//     <latent>,<flags>,<calls>,<input>
//                                                    mouse_native's linear pitch in the frame before the read
//                                                    (mouse_native.h MouseNativePitch)
#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "auto_match.h"
#include "cfg.h"
#include "gfx.h"
#include "gfx_wsys.h"
#include "host.h"
#include "input.h"
#include "mgyro_bench.h"
#include "mouse_native.h"
#include "platform/platform.h"
#include "ui/menu.h"

enum { AIMS = 10, SRING = 8192 };

static int bench_on = -1;
int mgb_on(void) {
    if (bench_on < 0) {
        const char *e = cfg_get("TOTK_MGYRO_BENCH");
        bench_on = e && *e && strcmp(e, "0") != 0;
    }
    return bench_on;
}

// ---- the CSV: lines appended under out_lock by any thread, written by the bench thread
static PlatMutex out_lock = PLAT_MUTEX_INIT;
static char *obuf;
static size_t olen, ocap;
static FILE *ofile;
static volatile int32_t closed;   // the bench is done: nothing more is kept

static void out(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
static void out(const char *fmt, ...) {
    if (__atomic_load_n(&closed, __ATOMIC_RELAXED)) return;
    char line[1600];
    va_list ap;
    va_start(ap, fmt);
    int printed = vsnprintf(line, sizeof line, fmt, ap);
    va_end(ap);
    if (printed <= 0) return;
    // vsnprintf reports the length it wanted: a line longer than the buffer is kept cut at the buffer's end
    size_t n = (size_t)printed < sizeof line ? (size_t)printed : sizeof line - 1;
    plat_mutex_lock(&out_lock);
    if (olen + n + 1 > ocap) {
        size_t c = ocap ? ocap * 2 : (size_t)1 << 20;
        while (c < olen + n + 1) c *= 2;
        char *p = realloc(obuf, c);
        if (!p) { plat_mutex_unlock(&out_lock); return; }
        obuf = p, ocap = c;
    }
    memcpy(obuf + olen, line, n);
    olen += n;
    obuf[olen++] = '\n';
    plat_mutex_unlock(&out_lock);
}

static void out_flush(void) {
    plat_mutex_lock(&out_lock);
    char *b = obuf;
    size_t n = olen;
    obuf = NULL, olen = ocap = 0;
    plat_mutex_unlock(&out_lock);
    if (b && ofile && n) fwrite(b, 1, n, ofile);
    free(b);
    if (ofile) fflush(ofile);
}

// ---- the simulated mouse: one move at a time, its counts sent as packets at the polling rate. A straight move sends
// (nx, ny) over dur; a circle (radius > 0) goes round radius counts from where it starts, cycles times in dur, clockwise
// on screen from its rightmost point (x right, then y down), and ends where it started.
static PlatMutex mv_lock = PLAT_MUTEX_INIT;
// xscale: the circle's horizontal radius over its vertical one (0: up and down only); sx, sy: sent so far
static struct { int active; uint64_t t0, dur; double rate, radius, cycles, xscale; int32_t nx, ny, sx, sy; } mv;
static int64_t inj_x, inj_y;   // every count sent (mv_lock)

void mgb_take(uint64_t now, double *dx, double *dy) {
    *dx = *dy = 0;
    plat_mutex_lock(&mv_lock);
    if (mv.active && now >= mv.t0) {
        double frac = 1;
        if (mv.dur) {   // packets at t0 + i / rate, each bringing the total to round(n * i / (rate * dur))
            double i = floor((double)(now - mv.t0) * 1e-9 * mv.rate);
            frac = i / (mv.rate * (double)mv.dur * 1e-9);
            if (frac > 1) frac = 1;
        }
        int32_t tx = (int32_t)lround(mv.nx * frac), ty = (int32_t)lround(mv.ny * frac);
        if (mv.radius > 0) {   // the point on the circle at this packet, relative to the start
            double a = 2 * M_PI * mv.cycles * frac;
            tx = (int32_t)lround(mv.radius * mv.xscale * (cos(a) - 1)), ty = (int32_t)lround(mv.radius * sin(a));
        }
        int32_t ddx = tx - mv.sx, ddy = ty - mv.sy;
        mv.sx = tx, mv.sy = ty;
        if (frac >= 1) mv.active = 0;
        inj_x += ddx, inj_y += ddy;
        *dx = ddx, *dy = ddy;
        if (ddx || ddy) out("T,%llu,%d,%d", (unsigned long long)now, ddx, ddy);
    }
    plat_mutex_unlock(&mv_lock);
}

// ---- samples: numbered as the sensor ring numbers them (every push to the mouse gyro's sensor passes here)
typedef struct { int64_t no; uint64_t t; double yaw, pitch; } Smp;
static PlatMutex s_lock = PLAT_MUTEX_INIT;
static Smp sring[SRING];
static int64_t s_no;
static double s_yaw, s_last;
static int s_have;
// Field tests: 0 the samples as made; 1 the rate only (attitude, gravity and angle held at the mode's first sample);
// 2 the attitude only (rate 0, angle held); 3 delta_ns doubled.
static volatile int32_t field_mode;
static MotionState held;
static int held_set;

static double clamp1(double v) { return v > 1 ? 1 : v < -1 ? -1 : v; }

static volatile int32_t yaw_body_set = -1;   // the yawaxis test's choice of yaw axis, -1: input.c's own
int mgb_yaw_body(int def) {
    int v = __atomic_load_n(&yaw_body_set, __ATOMIC_RELAXED);
    return v >= 0 ? v : def;
}

void mgb_samples(MotionState *s, int n, uint64_t now) {
    if (!mgb_on()) return;
    int mode = __atomic_load_n(&field_mode, __ATOMIC_RELAXED);
    if (!mode) held_set = 0;
    for (int i = 0; i < n; i++) {
        // the controller's y axis (dir[3..5]) in the world, held flat: yaw about the world's up, pitch above level
        double yaw = atan2(-s[i].dir[3], s[i].dir[4]) * 180 / M_PI, pitch = asin(clamp1(s[i].dir[5])) * 180 / M_PI;
        if (!s_have) s_last = s_yaw = yaw, s_have = 1;
        double d = yaw - s_last;
        while (d > 180) d -= 360;
        while (d < -180) d += 360;
        s_yaw += d, s_last = yaw;
        plat_mutex_lock(&s_lock);
        int64_t no = ++s_no;
        sring[no % SRING] = (Smp){no, now, s_yaw, pitch};
        plat_mutex_unlock(&s_lock);
        out("S,%lld,%llu,%.6f,%.6f,%d", (long long)no, (unsigned long long)now, s_yaw, pitch, n);
        if (mode && !held_set) held = s[i], held_set = 1;
        if (mode == 1) {
            memcpy(s[i].dir, held.dir, sizeof held.dir);
            memcpy(s[i].acc, held.acc, sizeof held.acc);
            memcpy(s[i].angle, held.angle, sizeof held.angle);
        } else if (mode == 2) {
            memset(s[i].gyro, 0, sizeof s[i].gyro);
            memcpy(s[i].angle, held.angle, sizeof held.angle);
        } else if (mode == 3) {
            s[i].delta_ns *= 2;
        }
    }
}

// ---- reads and presents
static volatile int64_t reads;              // six-axis reads so far
static volatile uint64_t last_read_ns, period_ns;
static int64_t last_s0;
static uint64_t aim_addr[AIMS];
static volatile int32_t naims;
static volatile int64_t stall_ns;           // the next read holds the game thread that long (hitch test)
static volatile int32_t shots_left;         // the next reads each ask for a shot of the frame presented after them
static char shots_dir[512];
static volatile uint64_t presents;
static volatile int32_t started;

static void bench_main(void *arg);

// ---- watches: objects of the game taken at a function's entry, and floats of them logged after each read.
// TOTK_MGYRO_BENCH_OBJ: main offsets (hex) of functions whose x0 on entry is the object to watch, object k being the
// k-th function's; TOTK_MGYRO_BENCH_FIELDS: <k>+<hex offset>[*<n>] entries, n floats (default 1) of object k at that
// offset. Each read then adds W,<read>,<values> (nan until the object is known). The function-entry hook runs only
// while the bench looks for the objects (watch_find), since every lifted function calls it then.
enum { WOBJ = 8, WFIELDS = 64, WSEEN = 4 };
static uint64_t wfn[WOBJ];
static uint64_t wseen[WOBJ][WSEEN];   // the distinct objects seen per function, the first one watched
static volatile uint64_t wobj[WOBJ];
static int nwfn, nwf;
static struct { int k; uint32_t off, n; } wf[WFIELDS];
static PlatMutex w_lock = PLAT_MUTEX_INIT;

static uint64_t main_base(void) { return ldr_count() ? ldr_module(0)->base : 0; }

static void watch_parse(void) {
    const char *o = cfg_get("TOTK_MGYRO_BENCH_OBJ"), *f = cfg_get("TOTK_MGYRO_BENCH_FIELDS");
    for (const char *p = o; p && *p && nwfn < WOBJ;) {
        char *end;
        unsigned long long off = strtoull(p, &end, 16);
        if (end == p) break;
        wfn[nwfn++] = main_base() + off;
        p = end + (*end == ',');
    }
    for (const char *p = f; p && *p && nwf < WFIELDS;) {
        int k, used = 0;
        unsigned off, n = 1;
        if (sscanf(p, "%d+%x%n", &k, &off, &used) != 2 || k < 0 || k >= nwfn) break;
        p += used;
        if (*p == '*') {
            char *end;
            n = (unsigned)strtoul(p + 1, &end, 10);
            p = end;
        }
        wf[nwf].k = k, wf[nwf].off = off, wf[nwf].n = n < 1 ? 1 : n > 16 ? 16 : n;
        nwf++;
        while (*p && *p != ',') p++;
        p += *p == ',';
    }
}

void mgb_fn_hit(uint64_t addr, uint64_t x0) {
    for (int k = 0; k < nwfn; k++) {
        if (addr != wfn[k] || !x0) continue;
        plat_mutex_lock(&w_lock);
        for (int i = 0; i < WSEEN; i++) {
            if (wseen[k][i] == x0) break;
            if (!wseen[k][i]) {
                wseen[k][i] = x0;
                if (!i) __atomic_store_n(&wobj[k], x0, __ATOMIC_RELEASE);
                break;
            }
        }
        plat_mutex_unlock(&w_lock);
    }
}

static void watch_line(int64_t r) {
    if (!nwf) return;
    char line[1500];
    int k = snprintf(line, sizeof line, "W,%lld", (long long)r);
    for (int i = 0; i < nwf && k < (int)sizeof line - 24; i++) {
        uint64_t o = __atomic_load_n(&wobj[wf[i].k], __ATOMIC_ACQUIRE), a = o + wf[i].off;
        int ok = o && mem_guest_readable(a, 4ull * wf[i].n);
        for (uint32_t j = 0; j < wf[i].n && k < (int)sizeof line - 24; j++)
            k += snprintf(line + k, sizeof line - (size_t)k, ok ? ",%.7g" : ",nan",
                          ok ? (double)((const float *)(uintptr_t)a)[j] : 0.0);
    }
    out("%s", line);
}

// ---- pokes: TOTK_MGYRO_BENCH_POKE=<main hex offset>=<hex byte>[,...], bytes of main's data the pokecmp test writes
// for its second half and puts back after: switches of the game, to compare its behavior with and without them
enum { NPOKE = 8 };
static struct { uint64_t off; uint8_t val, orig; } pk[NPOKE];
static int npk;

static void poke_parse(void) {
    for (const char *p = cfg_get("TOTK_MGYRO_BENCH_POKE"); p && *p && npk < NPOKE;) {
        unsigned long long off;
        unsigned v;
        int used = 0;
        if (sscanf(p, "%llx=%x%n", &off, &v, &used) != 2) break;
        pk[npk].off = off, pk[npk].val = (uint8_t)v, npk++;
        p += used;
        p += *p == ',';
    }
}

static void poke_apply(int on) {
    for (int i = 0; i < npk; i++) {
        uint64_t a = main_base() + pk[i].off;
        if (!mem_guest_readable(a, 1)) continue;
        volatile uint8_t *b = (volatile uint8_t *)(uintptr_t)a;
        if (on) pk[i].orig = *b, *b = pk[i].val;
        else *b = pk[i].orig;
        out("H,poke,main+%#llx=%#x", (unsigned long long)pk[i].off, (unsigned)*b);
    }
}

void mgb_read(const MotionState *s, int n, uint64_t now) {
    if (!mgb_on()) return;
    if (!__atomic_load_n(&started, __ATOMIC_RELAXED) && !plat_xchg32(&started, 1))
        if (!plat_thread_spawn(bench_main, NULL, "mgyro bench", 0)) rt_log("mgyro bench: cannot start its thread");
    int64_t s0 = n ? s[0].sampling : 0;
    double cy = NAN, cp = NAN;
    plat_mutex_lock(&s_lock);
    const Smp *e = &sring[s0 % SRING];
    if (s0 > 0 && e->no == s0) cy = e->yaw, cp = e->pitch;
    plat_mutex_unlock(&s_lock);
    char aims[AIMS * 48] = "";
    int k = 0, na = __atomic_load_n(&naims, __ATOMIC_ACQUIRE);
    for (int i = 0; i < na; i++) {
        const float *v = (const float *)(uintptr_t)aim_addr[i];
        k += snprintf(aims + k, sizeof aims - (size_t)k, ",%.8f,%.8f,%.8f", v[0], v[1], v[2]);
    }
    plat_mutex_lock(&mv_lock);
    int64_t ix = inj_x, iy = inj_y;
    plat_mutex_unlock(&mv_lock);
    int64_t r = __atomic_load_n(&reads, __ATOMIC_RELAXED) + 1;
    out("R,%lld,%llu,%lld,%lld,%llu,%.6f,%.6f,%lld,%lld%s", (long long)r, (unsigned long long)now, (long long)s0,
        (long long)(s0 - last_s0), (unsigned long long)__atomic_load_n(&presents, __ATOMIC_RELAXED), cy, cp,
        (long long)ix, (long long)iy, aims);
    watch_line(r);
    MouseNativePitch np = mouse_native_pitch_debug();   // the linear pitch's previous frame (mouse_native.c)
    out("N,%lld,%.6f,%.6f,%.4f,%.4f,%.4f,%.5f,%.5f,%d,%.6f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%d,%u,%.4f", (long long)r,
        np.game, np.out, np.view, np.want, np.target, np.gain, np.slope, np.resync, np.now, np.step, np.seen, np.base1,
        np.base2, np.ctrl, np.latent, np.flags, np.calls, np.input);
    last_s0 = s0;
    uint64_t lr = __atomic_load_n(&last_read_ns, __ATOMIC_RELAXED);
    if (lr && now > lr && now - lr < 100000000ull) {   // the frame period, smoothed over about 16 reads
        uint64_t p = __atomic_load_n(&period_ns, __ATOMIC_RELAXED);
        __atomic_store_n(&period_ns, p ? p + ((int64_t)(now - lr) - (int64_t)p) / 16 : now - lr, __ATOMIC_RELAXED);
    }
    __atomic_store_n(&last_read_ns, now, __ATOMIC_RELAXED);
    __atomic_store_n(&reads, r, __ATOMIC_RELEASE);
    int64_t st = __atomic_exchange_n(&stall_ns, 0, __ATOMIC_RELAXED);
    if (st > 0) {
        out("E,%llu,%lld,stall ns=%lld", (unsigned long long)kern_now_ns(), (long long)r, (long long)st);
        plat_sleep_ns((uint64_t)st);
    }
}

void mgb_present(const void *tex, uint64_t n, uint64_t now, uint64_t slot) {
    if (!mgb_on()) return;
    __atomic_store_n(&presents, n, __ATOMIC_RELAXED);
    int64_t r = __atomic_load_n(&reads, __ATOMIC_RELAXED);
    out("P,%llu,%llu,%llu,%lld", (unsigned long long)n, (unsigned long long)now, (unsigned long long)slot, (long long)r);
    if (__atomic_load_n(&shots_left, __ATOMIC_RELAXED) > 0) {   // after the present: the picture of this frame
        char p[600];
        snprintf(p, sizeof p, "%s/p%06llu-r%06lld.png", shots_dir, (unsigned long long)n, (long long)r);
        rt_shot_small((const tex_t *)tex, p);
        __atomic_sub_fetch(&shots_left, 1, __ATOMIC_RELAXED);
    }
}

void mgb_flip(uint64_t slot, uint64_t now) {
    if (!mgb_on()) return;
    out("F,%llu,%llu", (unsigned long long)slot, (unsigned long long)now);
}

void mgb_mark(const char *what, uint64_t v) {
    if (!mgb_on()) return;
    out("M,%llu,%s,%u,%llx", (unsigned long long)kern_now_ns(), what, plat_thread_id(), (unsigned long long)v);
}

static volatile int32_t detail;
int mgb_detail(void) { return __atomic_load_n(&detail, __ATOMIC_RELAXED); }
static int64_t nreads_now(void) { return __atomic_load_n(&reads, __ATOMIC_ACQUIRE); }

void mgb_mark2(const char *what, uint64_t a, uint64_t b) {
    if (!mgb_on()) return;
    out("M,%llu,%s,%u,%llx,%llx", (unsigned long long)kern_now_ns(), what, plat_thread_id(), (unsigned long long)a,
        (unsigned long long)b);
}

// The detail window: until n more reads, poll the aim vectors and log "A,<t>,<aim>,<x>,<y>,<z>" each time one changes,
// so the timeline shows when each copy of the view takes a turn.
static void detail_poll(int64_t n) {
    int64_t target = nreads_now() + n;
    float last[AIMS][3];
    int na = __atomic_load_n(&naims, __ATOMIC_ACQUIRE);
    for (int i = 0; i < na; i++) memcpy(last[i], (const void *)(uintptr_t)aim_addr[i], sizeof last[i]);
    uint64_t t0 = kern_now_ns();
    while (nreads_now() < target && kern_now_ns() - t0 < 5000000000ull) {
        for (int i = 0; i < na; i++) {
            float v[3];
            memcpy(v, (const void *)(uintptr_t)aim_addr[i], sizeof v);
            if (memcmp(v, last[i], sizeof v)) {
                memcpy(last[i], v, sizeof v);
                out("A,%llu,%d,%.6f,%.6f,%.6f", (unsigned long long)kern_now_ns(), i, v[0], v[1], v[2]);
            }
        }
        plat_yield();
    }
}

// ---- the bench thread
static void ev(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
static void ev(const char *fmt, ...) {
    char t[400];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(t, sizeof t, fmt, ap);
    va_end(ap);
    out("E,%llu,%lld,%s", (unsigned long long)kern_now_ns(), (long long)__atomic_load_n(&reads, __ATOMIC_ACQUIRE), t);
}

static int64_t nreads(void) { return __atomic_load_n(&reads, __ATOMIC_ACQUIRE); }

static void wait_reads(int64_t n) {
    int64_t target = nreads() + n;
    uint64_t t0 = kern_now_ns();
    while (nreads() < target) {
        plat_sleep_ns(1000000);
        if (kern_now_ns() - t0 > 30000000000ull) { rt_log("mgyro bench: the game stopped reading the sensor"); return; }
    }
    out_flush();
}

static void move_start(int nx, int ny, double cps, double rate, uint64_t t0) {
    double n = fmax(fabs((double)nx), fabs((double)ny));
    plat_mutex_lock(&mv_lock);
    mv.active = 1, mv.t0 = t0 ? t0 : kern_now_ns(), mv.dur = cps > 0 ? (uint64_t)(n / cps * 1e9) : 0;
    mv.rate = rate, mv.nx = nx, mv.ny = ny, mv.sx = mv.sy = 0, mv.radius = 0, mv.cycles = 0, mv.xscale = 1;
    plat_mutex_unlock(&mv_lock);
}

// cycles turns of a circle of radius counts, each in period seconds, packets at rate Hz
static void circle_start(double radius, double period, double cycles, double rate, double xscale) {
    plat_mutex_lock(&mv_lock);
    mv.active = 1, mv.t0 = kern_now_ns(), mv.dur = (uint64_t)(period * cycles * 1e9);
    mv.rate = rate, mv.nx = mv.ny = 0, mv.sx = mv.sy = 0, mv.radius = radius, mv.cycles = cycles, mv.xscale = xscale;
    plat_mutex_unlock(&mv_lock);
}

static void move_wait(void) {
    for (;;) {
        plat_mutex_lock(&mv_lock);
        int a = mv.active;
        plat_mutex_unlock(&mv_lock);
        if (!a) return;
        plat_sleep_ns(500000);
    }
}

static void move(int nx, int ny, double cps, double rate) {
    move_start(nx, ny, cps, rate, 0);
    move_wait();
}

// ---- finding the aim: unit vectors that turn with the controller about one fixed axis
typedef struct { uint64_t a; float v0[3], v1[3]; int pass; double d1, d2, d12, d3; float dv; } Cand;

// A unit vector that can turn: no component at +-1, and at most one near 0 (a level camera's right vector has one).
static int unit_ok3(float x, float y, float z) {
    if (!(fabsf(x) <= 0.9995f && fabsf(y) <= 0.9995f && fabsf(z) <= 0.9995f)) return 0;
    return (fabsf(x) < 1e-3f) + (fabsf(y) < 1e-3f) + (fabsf(z) < 1e-3f) <= 1;
}

static size_t scan(Cand **out_c) {
    uint64_t base = mem_layout()->heap.base, size = mem_heap_size();
    size_t cap = 1 << 20, n = 0;
    Cand *c = malloc(cap * sizeof *c);
    if (!c) return 0;
    uint64_t chunk = 1ull << 21, skipped = 0;
    for (uint64_t off = 0; off + 12 <= size; off += chunk) {
        // 2 MiB at a time; a chunk with a page that cannot be read is taken page by page
        if (off % (1ull << 21) == 0) chunk = plat_mem_readable(base + off, size - off < (1ull << 21) ? size - off : 1ull << 21) ? 1ull << 21 : 4096;
        uint64_t len = size - off < chunk ? size - off : chunk;
        if (chunk == 4096 && !plat_mem_readable(base + off, len)) { skipped += len; continue; }
        if (len + 8 <= size - off && plat_mem_readable(base + off + len, 8)) len += 8;   // triples across the boundary
        const float *f = (const float *)(uintptr_t)(base + off);
        size_t m = (size_t)(len / 4), stop = (size_t)((size - off < chunk ? size - off : chunk) / 4);
        for (size_t i = 0; i + 2 < m && i < stop; i++) {
            float x = f[i], y = f[i + 1], z = f[i + 2];
            float q = x * x + y * y + z * z;
            if (!(fabsf(q - 1) <= 2e-5f) || !unit_ok3(x, y, z)) continue;
            if (n == cap) {
                if (cap >= (size_t)8 << 20) break;
                Cand *p = realloc(c, cap * 2 * sizeof *c);
                if (!p) break;
                c = p, cap *= 2;
            }
            c[n++] = (Cand){.a = base + off + i * 4, .v0 = {x, y, z}};
        }
    }
    if (skipped) rt_log("mgyro bench: %llu bytes of the heap could not be read", (unsigned long long)skipped);
    *out_c = c;
    return n;
}

// The angle between two vectors, in degrees.
static double chord(const float *a, const float *b) {
    double d = (double)a[0] * b[0] + (double)a[1] * b[1] + (double)a[2] * b[2];
    double n = sqrt(((double)a[0] * a[0] + (double)a[1] * a[1] + (double)a[2] * a[2]) *
                    ((double)b[0] * b[0] + (double)b[1] * b[1] + (double)b[2] * b[2]));
    return n > 0 ? acos(clamp1(d / n)) * 180 / M_PI : 0;
}

static int by_pitch_moved(const void *a, const void *b) {
    float x = ((const Cand *)a)->dv, y = ((const Cand *)b)->dv;
    return x < y ? 1 : x > y ? -1 : 0;
}

static void read3(uint64_t a, float v[3]) {
    const float *p = (const float *)(uintptr_t)a;
    v[0] = p[0], v[1] = p[1], v[2] = p[2];
}

// Unit vectors that follow the controller: a turn of +A moves them by c1 degrees, -2A brings them as far to the other
// side (by c1 from the start, about 2 c1 from where they were), +A back to the start. Whatever the axis they turn about
// (the game may turn its camera in the camera's own frame), so the view's vectors and the game's copies of the sensor
// both pass; the copies turn by exactly the controller's angle. Then how far a pitch moves each.
static int find_aims(void) {
    const int A = 500, P = 250;   // counts: 20 and 10 degrees of controller turn at 0.04
    ev("find pre");
    // The right stick turns the camera and not the controller, so the view's vectors and the game's copies of the
    // sensor no longer hold the same values; then off the start pose, so no component is exactly 0 or 1.
    input_inject("RSTICK 26000 0 20");
    wait_reads(60);
    move(137, -53, 2000, 1000);
    wait_reads(45);
    uint64_t t0 = kern_now_ns();
    Cand *c = NULL;
    size_t n = scan(&c);
    rt_log("mgyro bench: %zu unit vectors in the heap (%.1f GiB scanned in %.2f s)", n,
           mem_heap_size() / (double)(1ull << 30), (kern_now_ns() - t0) / 1e9);
    ev("find scan n=%zu", n);
    move(A, 0, 2000, 1000);
    wait_reads(45);
    size_t m = 0;
    for (size_t i = 0; i < n; i++) {
        float v1[3];
        read3(c[i].a, v1);
        if (!(fabsf(v1[0] * v1[0] + v1[1] * v1[1] + v1[2] * v1[2] - 1) <= 1e-4f)) continue;
        double c1 = chord(c[i].v0, v1);
        if (c1 < 2) continue;
        c[i].d1 = c1;
        memcpy(c[i].v1, v1, sizeof v1);
        c[m++] = c[i];
    }
    ev("find yaw1 n=%zu", m);
    move(-2 * A, 0, 2000, 1000);
    wait_reads(45);
    for (size_t i = 0; i < m; i++) {
        float v2[3];
        read3(c[i].a, v2);
        c[i].d2 = chord(c[i].v0, v2);
        c[i].d12 = chord(c[i].v1, v2);
        // to the other side of the start, by about as much (the camera's turn for a turn need not be the same each time)
        c[i].pass = c[i].d2 > 0.5 * c[i].d1 && c[i].d2 < 2 * c[i].d1 && c[i].d12 > 1.5 * fmin(c[i].d1, c[i].d2);
    }
    move(A, 0, 2000, 1000);
    wait_reads(45);
    size_t m3 = 0;
    for (size_t i = 0; i < m; i++) {
        float v3[3];
        read3(c[i].a, v3);
        c[i].d3 = chord(c[i].v0, v3);
        c[i].pass &= c[i].d3 < 0.3 * c[i].d1;   // the camera need not come back exactly: that is measured later
        m3 += c[i].pass;
    }
    ev("find yaw3 n=%zu", m3);
    move(0, -P, 2000, 1000);   // up
    wait_reads(45);
    ev("find pitch");
    for (size_t i = 0; i < m; i++) {
        float v4[3];
        read3(c[i].a, v4);
        c[i].dv = (float)chord(c[i].v0, v4);
    }
    move(0, P, 2000, 1000);
    wait_reads(45);
    for (size_t i = 0; i < m && i < 1500; i++)
        out("H,cand,heap+%#llx c1=%.4f c2=%.4f c12=%.4f c3=%.4f cp=%.4f pass=%d v0=%.5f %.5f %.5f",
            (unsigned long long)(c[i].a - mem_layout()->heap.base), c[i].d1, c[i].d2, c[i].d12, c[i].d3, c[i].dv,
            c[i].pass, c[i].v0[0], c[i].v0[1], c[i].v0[2]);
    // The view: vectors that turned by another angle than the controller (the game's copies of the sensor turn by
    // exactly A counts, less for a vector off the horizontal), those the pitch moved most first (the forward vector moves
    // by the whole pitch), one per distinct value; then one copy of the sensor, as a reference.
    // Before them all, the camera's forward vector even when its turns did not add up (the camera turned 37, 59 and 15
    // degrees for +20, -40 and +20 with the 200 Hz states of the first version): nearly level, turned far, moved by the
    // pitch about as much as the camera turns.
    qsort(c, m, sizeof *c, by_pitch_moved);
    for (size_t i = 0; i < m; i++)
        c[i].pass |= (c[i].d1 > 25 && c[i].dv > 10 && c[i].dv < 40 && fabsf(c[i].v0[1]) < 0.1f) << 1;
    int chosen = 0;
    for (int pass = 0; pass < 3 && chosen < AIMS; pass++)
        for (size_t i = 0; i < m && chosen < AIMS; i++) {
            int copy = fabs(c[i].d1 - A * 0.04) < 0.02;
            int pitched = c[i].dv > 1;
            if (pass == 0 ? !(c[i].pass & 2) : !(c[i].pass & 1) || (pass == 1 && copy) || (pass == 2 && (!copy || !pitched)))
                continue;
            int dup = 0;
            for (int j = 0; j < chosen; j++) {
                float w[3], v[3];
                read3(aim_addr[j], w);
                read3(c[i].a, v);
                dup |= fabsf(w[0] - v[0]) < 1e-5f && fabsf(w[1] - v[1]) < 1e-5f && fabsf(w[2] - v[2]) < 1e-5f;
            }
            if (dup) continue;
            aim_addr[chosen] = c[i].a;
            out("H,aim%d,heap+%#llx c1=%.4f c3=%.4f cp=%.4f kind=%s v0=%.6f %.6f %.6f", chosen,
                (unsigned long long)(c[i].a - mem_layout()->heap.base), c[i].d1, c[i].d3, c[i].dv,
                copy ? "sensor" : pitched ? "view" : "view-flat", c[i].v0[0], c[i].v0[1], c[i].v0[2]);
            const float *ctx = (const float *)(uintptr_t)(c[i].a - 48);
            char line[400];
            int k = 0;
            for (int j = 0; j < 28; j++) k += snprintf(line + k, sizeof line - (size_t)k, " %.5f", ctx[j]);
            out("H,ctx%d,from -48:%s", chosen, line);
            rt_log("mgyro bench: aim %d at heap+%#llx: turned %.3f degrees for %.1f, %.3f for the pitch (%s)", chosen,
                   (unsigned long long)(c[i].a - mem_layout()->heap.base), c[i].d1, A * 0.04, c[i].dv,
                   copy ? "the game's copy of the sensor" : "view");
            chosen++;
            if (pass == 2) break;   // one sensor copy is enough
        }
    free(c);
    __atomic_store_n(&naims, chosen, __ATOMIC_RELEASE);
    ev("find done aims=%d", chosen);
    out_flush();
    return chosen;
}

// ---- tests
static void settle(void) { wait_reads(40); }

static void t_idle(int secs) {
    ev("idle s=%d", secs);
    wait_reads(60 * secs);
    ev("end");
}

static void t_lin(int axis) {
    static const double v[] = {6, 15, 30, 60, 120, 300, 600, 1200, 3000, 6000, 12000, 30000, 60000, 120000};
    for (size_t i = 0; i < sizeof v / sizeof *v; i++) {
        int n = axis ? (int)fmin(fmax(v[i] * 0.5, 30), 500) : (int)fmin(fmax(v[i], 30), 2400);
        for (int sgn = 1; sgn >= -1; sgn -= 2) {
            ev("lin axis=%c v=%g n=%d", axis ? 'y' : 'x', v[i], sgn * n);
            move(axis ? 0 : sgn * n, axis ? sgn * n : 0, v[i], 1000);
            settle();
            ev("end");
        }
    }
}

static void t_rt(void) {
    static const struct { int axis, n; double v; int cycles; } r[] = {
        {0, 200, 1200, 100}, {0, 600, 30000, 100}, {1, 100, 1200, 100}, {1, 300, 30000, 100}};
    for (size_t i = 0; i < sizeof r / sizeof *r; i++) {
        ev("rt axis=%c n=%d v=%g cycles=%d", r[i].axis ? 'y' : 'x', r[i].n, r[i].v, r[i].cycles);
        for (int k = 0; k < r[i].cycles; k++) {
            move(r[i].axis ? 0 : r[i].n, r[i].axis ? r[i].n : 0, r[i].v, 1000);
            move(r[i].axis ? 0 : -r[i].n, r[i].axis ? -r[i].n : 0, r[i].v, 1000);
        }
        settle();
        ev("end");
    }
    // slow one way, fast the other: any speed dependence of the gain adds up
    ev("rt axis=x n=300 v=300/30000 cycles=50");
    for (int k = 0; k < 50; k++) {
        move(300, 0, 300, 1000);
        move(-300, 0, 30000, 1000);
    }
    settle();
    ev("end");
}

static void t_step(void) {
    static const struct { int axis, n; } s[] = {{0, 10}, {0, 50}, {0, 200}, {0, 800}, {1, 50}, {1, 200}};
    for (size_t i = 0; i < sizeof s / sizeof *s; i++)
        for (int k = 0; k < 6; k++) {
            int n = k & 1 ? -s[i].n : s[i].n;
            ev("step axis=%c n=%d", s[i].axis ? 'y' : 'x', n);
            move(s[i].axis ? 0 : n, s[i].axis ? n : 0, 0, 1000);
            settle();
            ev("end");
        }
}

// A burst a given time before the predicted next read: is it in that read, or the next one?
static void t_phase(void) {
    for (int rep = 0; rep < 3; rep++)
        for (int phi = 0; phi <= 16; phi++) {
            int64_t r0 = nreads();
            while (nreads() == r0) plat_sleep_ns(200000);
            uint64_t p = __atomic_load_n(&period_ns, __ATOMIC_RELAXED), last = __atomic_load_n(&last_read_ns, __ATOMIC_RELAXED);
            if (!p) p = 16666667;
            uint64_t t = last + p - (uint64_t)phi * 1000000 - 500000, now = kern_now_ns();
            while (t < now + 1000000) t += p;
            int n = (rep * 17 + phi) & 1 ? -60 : 60;
            ev("phase phi_us=%d n=%d t0=%llu", phi * 1000 + 500, n, (unsigned long long)t);
            move_start(n, 0, 0, 1000, t);
            move_wait();
            wait_reads(20);
            ev("end");
        }
}

static void t_rate(void) {
    static const double hz[] = {125, 250, 500, 1000, 2000, 4000, 8000};
    static const struct { double v; int n; } sp[] = {{1200, 1200}, {6000, 3000}};
    for (size_t s = 0; s < 2; s++)
        for (size_t i = 0; i < sizeof hz / sizeof *hz; i++)
            for (int sgn = 1; sgn >= -1; sgn -= 2) {
                ev("rate v=%g hz=%g n=%d", sp[s].v, hz[i], sgn * sp[s].n);
                move(sgn * sp[s].n, 0, sp[s].v, hz[i]);
                settle();
                ev("end");
            }
}

static void t_fields(void) {
    for (int axis = 0; axis < 2; axis++)
        for (int mode = 0; mode <= 3; mode++) {
            __atomic_store_n(&field_mode, mode, __ATOMIC_RELAXED);
            wait_reads(3);
            for (int sgn = 1; sgn >= -1; sgn -= 2) {   // a move at 600 counts/s, not a packet: every sample carries counts
                int n = sgn * (axis ? 100 : 200);
                ev("field axis=%c mode=%d n=%d", axis ? 'y' : 'x', mode, n);
                move(axis ? 0 : n, axis ? n : 0, 600, 1000);
                settle();
                ev("end");
            }
            __atomic_store_n(&field_mode, 0, __ATOMIC_RELAXED);
            wait_reads(10);
        }
}

static void t_pitchyaw(void) {
    static const int pc[] = {-500, 500};   // up 20 degrees, down 20 degrees
    for (int i = 0; i < 2; i++) {
        move(0, pc[i], 1200, 1000);
        settle();
        for (int sgn = 1; sgn >= -1; sgn -= 2) {
            ev("pitchyaw pitch_counts=%d n=%d", pc[i], sgn * 500);
            move(sgn * 500, 0, 1200, 1000);
            settle();
            ev("end");
        }
        move(0, -pc[i], 1200, 1000);
        settle();
    }
}

// The yaw gain with the controller pitched 20 degrees up and down, the yaw rate about the world's up then about the
// controller's own vertical axis.
static void t_yawaxis(void) {
    static const int pc[] = {-500, 500};
    for (int i = 0; i < 2; i++) {
        move(0, pc[i], 1200, 1000);
        settle();
        for (int body = 0; body < 3; body++) {
            __atomic_store_n(&yaw_body_set, body, __ATOMIC_RELAXED);
            for (int sgn = 1; sgn >= -1; sgn -= 2) {
                ev("yawaxis body=%d pitch_counts=%d n=%d", body, pc[i], sgn * 500);
                move(sgn * 500, 0, 1200, 1000);
                settle();
                ev("end");
            }
        }
        __atomic_store_n(&yaw_body_set, -1, __ATOMIC_RELAXED);
        move(0, -pc[i], 1200, 1000);
        settle();
    }
}

// Back and forth at 1200 counts/s (240 counts, about 12 frames each way), straight away or after 1, 2, 4 or 8 reads
// without counts: how the game takes a reversal of the turn, with input.c's hold at a reversal (hold=-1) and without.
static volatile int32_t hold_set = -1;
int mgb_reverse_hold(int def) {
    int v = __atomic_load_n(&hold_set, __ATOMIC_RELAXED);
    return v >= 0 ? v : def;
}

static void t_reverse(void) {
    static const int pause[] = {0, 1, 2, 4, 8}, hold[] = {0, -1};
    for (size_t h = 0; h < 2; h++)
        for (size_t p = 0; p < sizeof pause / sizeof *pause; p++) {
            __atomic_store_n(&hold_set, hold[h], __ATOMIC_RELAXED);
            ev("reverse hold=%d pause=%d cycles=5 n=240 v=1200", hold[h], pause[p]);
            for (int k = 0; k < 5; k++)
                for (int sgn = 1; sgn >= -1; sgn -= 2) {
                    ev("leg hold=%d pause=%d sign=%d", hold[h], pause[p], sgn);
                    move(sgn * 240, 0, 1200, 1000);
                    if (pause[p]) wait_reads(pause[p]);
                }
            settle();
            ev("end");
        }
    __atomic_store_n(&hold_set, -1, __ATOMIC_RELAXED);
}

// Packets of more and more counts at once: the fastest turn the game takes in one frame.
static void t_cap(void) {
    static const int n[] = {100, 200, 400, 800, 1200, 1600, 2400, 3200, 4800};
    for (size_t i = 0; i < sizeof n / sizeof *n; i++)
        for (int sgn = 1; sgn >= -1; sgn -= 2) {
            ev("cap n=%d", sgn * n[i]);
            move(sgn * n[i], 0, 0, 1000);
            settle();
            ev("end");
        }
}

// The game's recentre (Y) with the controller pitched 10 degrees down or up, with the virtual controller levelled at
// the same time (as Y does) or not: where the camera's pitch ends, and how it moves when the mouse comes back.
static volatile int32_t level_set = -1;   // -1: input.c's own choice
int mgb_level_on_y(int def) {
    int v = __atomic_load_n(&level_set, __ATOMIC_RELAXED);
    return v >= 0 ? v : def;
}

static void t_recenter(void) {
    static const struct { const char *what; int dy, y; } st[] = {
        {"down10", 250, 0}, {"Y", 0, 1}, {"up10", -250, 0}, {"up10b", -250, 0}, {"down10b", 250, 0}};
    for (int lv = 1; lv >= 0; lv--) {
        // from level: the controller levelled with a Y, a small move each way (the game takes the levelling's turn
        // then), a Y that does not level: controller and camera level
        __atomic_store_n(&level_set, 1, __ATOMIC_RELAXED);
        input_inject("Y 6");
        wait_reads(90);
        move(0, 25, 1200, 1000);
        move(0, -25, 1200, 1000);
        settle();
        __atomic_store_n(&level_set, 0, __ATOMIC_RELAXED);
        input_inject("Y 6");
        wait_reads(90);
        __atomic_store_n(&level_set, lv, __ATOMIC_RELAXED);
        for (size_t i = 0; i < sizeof st / sizeof *st; i++) {
            ev("recenter level=%d step=%s", lv, st[i].what);
            if (st[i].y) {
                input_inject("Y 6");
                wait_reads(90);
            } else {
                move(0, st[i].dy, 1200, 1000);
                settle();
            }
            ev("end");
        }
    }
    __atomic_store_n(&level_set, -1, __ATOMIC_RELAXED);
}

// From level, up past every limit in 125-count steps (5 degrees of controller pitch at 0.04), back down 8 steps, down
// past every limit (the controller stops at mouse_gyro_pitch), back up 8 steps, then level again.
static void t_limits(void) {
    static const struct { int steps, dy; } seq[] = {{24, -125}, {8, 125}, {32, 125}, {8, -125}};
    for (size_t s = 0; s < sizeof seq / sizeof *seq; s++)
        for (int k = 1; k <= seq[s].steps; k++) {
            move(0, seq[s].dy, 2500, 1000);
            wait_reads(20);
            ev("lim leg=%zu step=%d dy=%d", s, k, seq[s].dy);
        }
    plat_mutex_lock(&s_lock);
    double pitch = sring[s_no % SRING].pitch;   // the controller's, degrees above level
    plat_mutex_unlock(&s_lock);
    move(0, (int)lround(pitch / cfg_double("TOTK_MOUSE_GYRO_SENS")), 2500, 1000);
    settle();
    ev("lim level");
}

// The pitch over its whole range in small steps: after a Y, up in 25-count steps (1 degree of controller pitch at
// 0.04) until 70 steps, down 140, up 70, each step settled for 12 reads, so the aim's degrees per count can be read
// as a function of where it points, both ways.
static void t_sweep(void) {
    input_inject("Y 6");
    wait_reads(90);
    static const struct { int steps, dy; } seq[] = {{70, -25}, {140, 25}, {70, -25}};
    for (size_t s = 0; s < sizeof seq / sizeof *seq; s++)
        for (int k = 1; k <= seq[s].steps; k++) {
            move(0, seq[s].dy, 2500, 1000);
            wait_reads(12);
            ev("sweep leg=%zu step=%d dy=%d", s, k, seq[s].dy);
        }
    settle();
    ev("end");
}

static void t_hitch(void) {
    static const int64_t ms[] = {50, 100, 150};
    for (int sgn = 1; sgn >= -1; sgn -= 2) {
        ev("hitch v=1200 n=%d stalls=%s", sgn * 2400, sgn > 0 ? "50,100,150" : "none");
        move_start(sgn * 2400, 0, 1200, 1000, 0);
        for (int i = 0; i < 3; i++) {
            wait_reads(25);
            if (sgn > 0) __atomic_store_n(&stall_ns, ms[i] * 1000000, __ATOMIC_RELAXED);
        }
        move_wait();
        settle();
        ev("end");
    }
}

static void t_fps(void) {
    static const char *lim[] = {"30", "45"};
    for (int i = 0; i < 2; i++) {
        cfg_set("TOTK_FPS_LIMIT", lim[i]);
        wait_reads(60);
        for (int sgn = 1; sgn >= -1; sgn -= 2) {
            ev("fps limit=%s lin v=1200 n=%d", lim[i], sgn * 1200);
            move(sgn * 1200, 0, 1200, 1000);
            settle();
            ev("end");
        }
        for (int sgn = 1; sgn >= -1; sgn -= 2) {
            ev("fps limit=%s step n=%d", lim[i], sgn * 200);
            move(sgn * 200, 0, 0, 1000);
            settle();
            ev("end");
        }
    }
    cfg_set("TOTK_FPS_LIMIT", "60");
    wait_reads(60);
}

static void shots_setup(void) {
    const char *o = cfg_get("TOTK_MGYRO_BENCH_OUT");
    snprintf(shots_dir, sizeof shots_dir, "%s-shots", o && *o ? o : "mgyro-bench.csv");
    plat_file_mkdir_p(shots_dir);
}

// A picture at rest and after each turn: right 10 degrees and back, up 5 and back, up 40, right 20 there and back,
// down to level again. Shows what turns on screen and how the view's vectors move with it.
static void t_look(void) {
    shots_setup();
    static const struct { int dx, dy; } m[] = {{0, 0}, {250, 0}, {-250, 0}, {0, -125}, {0, 125}, {0, -1000}, {500, 0},
                                               {-500, 0}, {0, 1000}};
    for (size_t i = 0; i < sizeof m / sizeof *m; i++) {
        if (m[i].dx || m[i].dy) move(m[i].dx, m[i].dy, 1200, 1000);
        settle();
        ev("look dx=%d dy=%d", m[i].dx, m[i].dy);
        __atomic_store_n(&shots_left, 1, __ATOMIC_RELAXED);
        wait_reads(5);
    }
}

static void t_image(void) {
    shots_setup();
    for (int sgn = 1; sgn >= -1; sgn -= 2) {
        ev("image n=%d", sgn * 400);
        const char *dt = cfg_get("TOTK_MGYRO_BENCH_DETAIL");
        int det = dt && atoi(dt);
        if (det) __atomic_store_n(&detail, 1, __ATOMIC_RELAXED);
        __atomic_store_n(&shots_left, 16, __ATOMIC_RELAXED);
        if (det) detail_poll(3);
        else wait_reads(3);
        move_start(sgn * 400, 0, 0, 1000, 0);
        if (det) detail_poll(16);
        move_wait();
        if (!det) wait_reads(16);
        __atomic_store_n(&detail, 0, __ATOMIC_RELAXED);
        settle();
        ev("end");
    }
}

// The mouse along circles: radius counts (250 is 10 degrees of controller turn at 0.04), a period from 2 s to 0.33 s,
// clockwise on screen. A 1:1 aim draws the same circle, delayed by the input lag only; tools read its shape from the
// injected counts and the aim vectors read by read. Each circle starts after a Y, from a level camera, so it stays
// inside the pitch limits whatever the test before left.
static void level_camera(void) {
    input_inject("Y 6");
    wait_reads(90);
}

static void circle_run(double radius, double period, double cycles, const char *tag) {
    level_camera();
    ev("circle r=%g period=%g cycles=%g%s", radius, period, cycles, tag);
    circle_start(radius, period, cycles, 1000, 1);
    move_wait();
    settle();
    ev("end");
}

static void t_circle(void) {
    static const struct { double r, period, cycles; } c[] = {
        {250, 2, 2}, {250, 1, 3}, {250, 0.5, 4}, {250, 0.333, 6}, {60, 0.5, 4}};
    for (size_t i = 0; i < sizeof c / sizeof *c; i++) circle_run(c[i].r, c[i].period, c[i].cycles, "");
}

// The mouse up and down only, the vertical of a circle (radius counts each way from where it starts): the pitch alone,
// over a wide part of its range at 500 counts, with no yaw to mix in. Each starts from a level camera.
static void t_vsine(void) {
    static const struct { double r, period, cycles; } c[] = {{250, 1, 3}, {500, 2, 2}, {500, 1, 3}, {60, 0.5, 4}};
    for (size_t i = 0; i < sizeof c / sizeof *c; i++) {
        level_camera();
        ev("vsine r=%g period=%g cycles=%g", c[i].r, c[i].period, c[i].cycles);
        circle_start(c[i].r, c[i].period, c[i].cycles, 1000, 0);
        move_wait();
        settle();
        ev("end");
    }
}

// mgb_native_keep: -1 leaves mouse_native.c's own choice
static volatile int32_t keep_set = -1;
int mgb_native_keep(int def) {
    int v = __atomic_load_n(&keep_set, __ATOMIC_RELAXED);
    return v >= 0 ? v : def;
}

// mgb_native_ref: -1 leaves mouse_native.c's own choice
static volatile int32_t ref_set = -1;
int mgb_native_ref(int def) {
    int v = __atomic_load_n(&ref_set, __ATOMIC_RELAXED);
    return v >= 0 ? v : def;
}

// mgb_native_hold: -1 leaves mouse_native.c's own choice
static volatile int32_t hold_resync_set = -1;
int mgb_native_hold(int def) {
    int v = __atomic_load_n(&hold_resync_set, __ATOMIC_RELAXED);
    return v >= 0 ? v : def;
}

// mgb_native_held: -1 leaves mouse_native.c's own choice
static volatile int32_t native_held_set = -1;
int mgb_native_held(int def) {
    int v = __atomic_load_n(&native_held_set, __ATOMIC_RELAXED);
    return v >= 0 ? v : def;
}

// Squid form and back (ZL held, then let go of) while the mouse turns, from a level camera, near the top limit and near
// the bottom one: in each form a yaw step of 100 counts and a pitch step of 50 (their signs alternate, so the view
// stays where it is), and a small circle (60 counts in 0.5 s) across each switch, so the mouse moves on both axes while
// the form changes. 17 cycles at each place: 51 switches each way. The hold of ZL ends halfway through the circle that
// follows the squid half. First with mouse_native.c's keeping of the game's pitch and of the controller off
// (mgb_native_keep), 6 cycles at each place, to compare, then on.
static void t_squid(void) {
    static const struct { const char *at; int dy; } place[] = {{"level", 0}, {"top", -640}, {"bottom", 620}};
    for (int keep = 0; keep < 2; keep++) {
        __atomic_store_n(&keep_set, keep, __ATOMIC_RELAXED);
        for (size_t p = 0; p < sizeof place / sizeof *place; p++) {
            level_camera();
            if (place[p].dy) move(0, place[p].dy, 2500, 1000);
            wait_reads(20);
            for (int k = 0; k < (keep ? 17 : 6); k++) {
                int s = k & 1 ? -1 : 1;
                for (int form = 0; form < 2; form++) {
                    const char *f = form ? "squid" : "human";
                    ev("squid keep=%d at=%s cycle=%d form=%s axis=x n=%d", keep, place[p].at, k, f, 100 * s);
                    move(100 * s, 0, 0, 1000);
                    wait_reads(15);
                    ev("end");
                    ev("squid keep=%d at=%s cycle=%d form=%s axis=y n=%d", keep, place[p].at, k, f, 50 * s);
                    move(0, 50 * s, 0, 1000);
                    wait_reads(15);
                    ev("end");
                    ev("squid keep=%d at=%s cycle=%d switch=%s", keep, place[p].at, k, form ? "human" : "squid");
                    if (!form) input_inject("ZL 92");
                    circle_start(60, 0.5, 1, 1000, 1);
                    move_wait();
                    wait_reads(15);
                }
            }
        }
    }
    __atomic_store_n(&keep_set, -1, __ATOMIC_RELAXED);
    ev("squid done");
}

// Counts that turn the view by deg at the game's yaw gain at the bench profile's motion sensitivity (2.3933 camera
// degrees per degree of controller turn).
static int view_counts(double deg) { return (int)lround(deg / (input_mouse_gyro_deg_per_count() * 2.3933)); }

// The view pushed past its bottom limit again and again, a third of each push taken back (1500 counts down, 500 up,
// eight times), then a step each way on both axes; once more past the limit and 100 counts back, and the steps again;
// then a Y, which should level the view. The same at the top. A virtual controller that keeps tilting while the view
// rests at a limit ends at its own limit (mouse_gyro_pitch), where it no longer tilts that way and, close to 90 degrees,
// its yaw no longer turns the camera. With mouse_native.c's keeping off (mgb_native_keep) then on.
static void t_drift(void) {
    static const struct { int x, y; } st[] = {{100, 0}, {-100, 0}, {0, 50}, {0, -50}};
    for (int keep = 0; keep < 2; keep++) {
        __atomic_store_n(&keep_set, keep, __ATOMIC_RELAXED);
        for (int top = 0; top < 2; top++) {
            const char *at = top ? "top" : "bottom";
            level_camera();
            int s = top ? -1 : 1;   // counts y > 0 look down
            for (int k = 0; k < 8; k++) {
                move(0, 1500 * s, 2500, 1000);
                move(0, -500 * s, 2500, 1000);
                wait_reads(5);
            }
            for (int pass = 0; pass < 2; pass++) {
                if (pass) move(0, 1500 * s, 2500, 1000), move(0, -100 * s, 2500, 1000);
                wait_reads(20);
                for (size_t i = 0; i < sizeof st / sizeof *st; i++) {
                    ev("drift keep=%d at=%s pass=%d axis=%c n=%d", keep, at, pass, st[i].x ? 'x' : 'y',
                       st[i].x ? st[i].x : st[i].y);
                    move(st[i].x, st[i].y, 0, 1000);
                    wait_reads(15);
                    ev("end");
                }
            }
            level_camera();
            ev("drift keep=%d at=%s after Y", keep, at);
        }
    }
    __atomic_store_n(&keep_set, -1, __ATOMIC_RELAXED);
}

// Swimming up a wall: the view turned 90 degrees left, toward the brick wall of the test range's corridor, the wall
// inked (ZR held 2.5 s), then ZL and the left stick forward for 5 s, the squid going to the wall and up it, with steps
// of 100 counts each way on both axes while it swims.
static void t_wall(void) {
    static const struct { int x, y; } st[] = {{100, 0}, {-100, 0}, {0, 100}, {0, -100}};
    level_camera();
    move(-view_counts(90), 0, 2500, 1000);
    wait_reads(10);
    input_inject("ZR 150");
    wait_reads(170);
    input_inject("ZL 300");
    input_inject("LSTICK 0 32767 300");
    wait_reads(20);
    for (int rep = 0; rep < 2; rep++)
        for (size_t i = 0; i < sizeof st / sizeof *st; i++) {
            ev("wall form=squid rep=%d axis=%c n=%d", rep, st[i].x ? 'x' : 'y', st[i].x ? st[i].x : st[i].y);
            move(st[i].x, st[i].y, 0, 1000);
            wait_reads(15);
            ev("end");
        }
    wait_reads(200);
    ev("wall done");
}

// Squid form in ink. The floor around the player is inked first (the view 60 degrees down, ZR held while it turns round
// once in 3 s); then, from a level camera, in each form: steps of 100 counts each way on both axes, and pictures at rest,
// after 50 counts right and after 50 counts up (each taken back), whose shifts compare the two axes on screen whatever
// the camera reports. ZL is held through the whole squid half.
static void t_ink(void) {
    static const struct { int x, y; } st[] = {{100, 0}, {-100, 0}, {0, 100}, {0, -100}};
    static const struct { int x, y; } sh[] = {{0, 0}, {50, 0}, {0, -50}};
    level_camera();
    int down = view_counts(60), turn = view_counts(360);
    move(0, down, 2500, 1000);
    input_inject("ZR 200");
    move(turn, 0, turn / 3.0, 1000);
    wait_reads(40);
    move(0, -down, 2500, 1000);
    level_camera();
    shots_setup();
    for (int form = 0; form < 2; form++) {
        const char *f = form ? "squid" : "human";
        if (form) {
            input_inject("ZL 500");
            wait_reads(40);
        }
        for (int rep = 0; rep < 2; rep++) {
            for (size_t i = 0; i < sizeof st / sizeof *st; i++) {
                ev("ink form=%s axis=%c n=%d", f, st[i].x ? 'x' : 'y', st[i].x ? st[i].x : st[i].y);
                move(st[i].x, st[i].y, 0, 1000);
                wait_reads(15);
                ev("end");
            }
            for (size_t i = 0; i < sizeof sh / sizeof *sh; i++) {
                if (sh[i].x || sh[i].y) move(sh[i].x, sh[i].y, 0, 1000);
                wait_reads(20);
                ev("ink form=%s shot dx=%d dy=%d", f, sh[i].x, sh[i].y);
                __atomic_store_n(&shots_left, 1, __ATOMIC_RELAXED);
                wait_reads(5);
                if (sh[i].x || sh[i].y) move(-sh[i].x, -sh[i].y, 0, 1000);
            }
        }
        if (form) wait_reads(220);   // ZL let go of
    }
    ev("ink done");
}

// One set of moves for a comparison, every event tagged: pitch steps (8 and 2 degrees of controller pitch at 0.04), yaw
// steps (8 and 0.4 degrees), five back-and-forth cycles of the yaw at 1200 counts/s, circles.
static void cmp_moves(const char *tag) {
    static const struct { int axis, n; } st[] = {{1, -200}, {1, 200}, {1, -200}, {1, 200}, {1, -50}, {1, 50},
                                                 {0, 200}, {0, -200}, {0, 10}, {0, -10}, {0, 10}, {0, -10}};
    for (size_t i = 0; i < sizeof st / sizeof *st; i++) {
        ev("step axis=%c n=%d%s", st[i].axis ? 'y' : 'x', st[i].n, tag);
        move(st[i].axis ? 0 : st[i].n, st[i].axis ? st[i].n : 0, 0, 1000);
        settle();
        ev("end");
    }
    ev("reverse n=240 v=1200 cycles=5%s", tag);
    for (int k = 0; k < 10; k++) move(k & 1 ? -240 : 240, 0, 1200, 1000);
    settle();
    ev("end");
    circle_run(250, 1, 3, tag);
    circle_run(250, 0.5, 4, tag);
    circle_run(60, 0.5, 4, tag);
}

// The same moves without and with the pokes (TOTK_MGYRO_BENCH_POKE).
static void t_pokecmp(void) {
    if (!npk) { ev("pokecmp: no TOTK_MGYRO_BENCH_POKE"); return; }
    cmp_moves(" poke=0");
    poke_apply(1);
    cmp_moves(" poke=1");
    poke_apply(0);
}

// The same moves with native mouse aiming off (the game's gyro smoothing, mouse_native.c) and on, the setting
// switched while the game runs; the setting's own value is put back after.
static void t_native(void) {
    char was[16];
    snprintf(was, sizeof was, "%s", cfg_value("TOTK_MOUSE_NATIVE"));
    cfg_set("TOTK_MOUSE_NATIVE", "0");
    wait_reads(10);
    cmp_moves(" native=0");
    cfg_set("TOTK_MOUSE_NATIVE", "1");
    wait_reads(10);
    cmp_moves(" native=1");
    cfg_set("TOTK_MOUSE_NATIVE", was);
    wait_reads(10);
}

static void wait_until(int64_t r) {
    if (nreads() < r) wait_reads(r - nreads());
}

// The view aimed at deg degrees above level from a level camera (counts at the yaw's gain).
static void aim_from_level(double deg) {
    level_camera();
    if (deg) move(0, -view_counts(deg), 2500, 1000);
    wait_reads(20);
}

// Leaving squid form. From a level camera aimed start degrees up: ZL held 100 frames (squid form), sq degrees of
// counts up in squid form once the camera settled, then the release; from the release, the mouse still (during 0),
// up (1) or down (-1) by dur_counts over half a second, or round a small circle (2: 60 counts in 0.5 s), as a player
// aiming while the form changes. Events: "surface ... phase=dive|squid|release|after". mode: how the case runs
// (surface_mode: new, old or the game's own aiming).
static const char *const surface_modes[] = {"game", "old", "new"};

// 2 native aiming as built, 1 with mgb_native_ref and mgb_native_hold off (release 0.3.4's), 0 the game's own aiming
// (mouse_native = 0; the virtual controller started over before each case, as nothing follows the view then).
static void surface_mode(int mode) {
    cfg_set("TOTK_MOUSE_NATIVE", mode ? "1" : "0");
    __atomic_store_n(&ref_set, mode == 1 ? 0 : -1, __ATOMIC_RELAXED);
    __atomic_store_n(&hold_resync_set, mode == 1 ? 0 : -1, __ATOMIC_RELAXED);
    wait_reads(10);
}

static void surface_case(int mode, double start, double sq, int during, int dur_counts, int swim) {
    if (!mode) input_motion_restart(), wait_reads(5);
    aim_from_level(start);
    const char *dn = during == 0 ? "still" : during == 1 ? "up" : during == -1 ? "down" : "circle";
    char tag[160];
    snprintf(tag, sizeof tag, "mode=%s start=%g sq=%g during=%s n=%d swim=%d", surface_modes[mode], start, sq, dn,
             dur_counts, swim);
    int64_t r0 = nreads();
    ev("surface %s phase=dive", tag);
    input_inject("ZL 100");
    if (swim) input_inject("LSTICK 0 32767 100");
    wait_reads(45);   // the change into squid form takes 22 to 40 frames
    ev("surface %s phase=squid", tag);
    if (sq) move(0, -view_counts(sq), 2500, 1000);
    wait_until(r0 + 98);
    ev("surface %s phase=release", tag);
    if (during == 1 || during == -1) move_start(0, -during * dur_counts, dur_counts / 0.5, 1000, 0);
    else if (during == 2) circle_start(60, 0.5, 1, 1000, 1);
    move_wait();
    wait_until(r0 + 100 + 60);
    ev("surface %s phase=after", tag);
}

// Ink the floor around the player: the view 60 degrees down, ZR held while it turns round once in 3 s.
static void ink_floor(void) {
    level_camera();
    int down = view_counts(60), turn = view_counts(360);
    move(0, down, 2500, 1000);
    input_inject("ZR 200");
    move(turn, 0, turn / 3.0, 1000);
    wait_reads(40);
    move(0, -down, 2500, 1000);
}

// modes: the surface_mode values to run, in order (2 as built, 1 0.3.4's, 0 the game's own aiming).
static void t_surface(const char *ms) {
    char was[16];
    snprintf(was, sizeof was, "%s", cfg_value("TOTK_MOUSE_NATIVE"));
    static const double start[] = {0, 20}, sq[] = {-6, 3, 6, 12, 25};
    static const struct { double start; int during, n; } dur[] = {
        {0, 1, 60}, {0, -1, 60}, {0, 1, 150}, {0, 2, 0}, {20, 1, 60}, {20, -1, 60}, {20, 1, 150}, {20, 2, 0},
        {-20, 1, 60}, {-20, 2, 0}, {40, 1, 60}, {40, 0, 0}};
    for (const char *m = ms; *m; m++) {
        int mode = *m - '0';
        if (mode < 0 || mode > 2) continue;
        surface_mode(mode);
        for (size_t i = 0; i < sizeof start / sizeof *start; i++) {
            surface_case(mode, start[i], 0, 0, 0, 0);
            for (size_t j = 0; j < sizeof sq / sizeof *sq; j++) surface_case(mode, start[i], sq[j], 0, 0, 0);
        }
        for (size_t i = 0; i < sizeof dur / sizeof *dur; i++)
            surface_case(mode, dur[i].start, 0, dur[i].during, dur[i].n, 0);
        surface_case(mode, 0, 6, 1, 60, 0);
    }
    __atomic_store_n(&ref_set, -1, __ATOMIC_RELAXED);
    __atomic_store_n(&hold_resync_set, -1, __ATOMIC_RELAXED);
    cfg_set("TOTK_MOUSE_NATIVE", was);
    wait_reads(10);
}

// Leaving squid form after a swim in ink, still moving forward: the floor inked first, then from start degrees up a
// swim of 100 frames with the left stick forward, released while it moves.
static void t_swim(void) {
    static const struct { double start, sq; int during, n; } c[] = {
        {0, 0, 0, 0}, {0, 0, 1, 60}, {0, 6, 0, 0}, {20, 0, 0, 0}, {20, 0, 1, 60}, {20, -6, 0, 0}};
    for (int mode = 2; mode >= 1; mode--) {
        surface_mode(mode);
        for (size_t i = 0; i < sizeof c / sizeof *c; i++) {
            if (i % 3 == 0) ink_floor();
            surface_case(mode, c[i].start, c[i].sq, c[i].during, c[i].n, 1);
        }
    }
    __atomic_store_n(&ref_set, -1, __ATOMIC_RELAXED);
    __atomic_store_n(&hold_resync_set, -1, __ATOMIC_RELAXED);
}

// Situations a player meets, each from a level camera aimed `at` degrees up, with the mouse still (mode 0), round a
// small circle (1: 60 counts, 0.5 s a turn, both axes) or up and down (2: 120 counts each way, 1 s a cycle) for 2.5 s,
// the action 10 reads after the motion starts. Events "situ <name> mode=<m> at=<deg>" and "end".
static void situ_motion(int mode, double secs) {
    if (mode == 1) circle_start(60, 0.5, secs / 0.5, 1000, 1);
    else if (mode == 2) circle_start(120, 1.0, secs / 1.0, 1000, 0);
}

static void situ(const char *name, int mode, double at) {
    aim_from_level(at);
    ev("situ %s mode=%d at=%g", name, mode, at);
    situ_motion(mode, 2.5);
    wait_reads(10);
    if (!strcmp(name, "jump")) input_inject("B 8");
    else if (!strcmp(name, "squidjump")) input_inject("ZL 50"), wait_reads(25), input_inject("B 8");
    else if (!strcmp(name, "shotsurface")) input_inject("ZL 60"), wait_reads(60), input_inject("ZR 20");
    else if (!strcmp(name, "sub")) input_inject("R 8");
    else if (!strcmp(name, "special")) input_inject("RCLICK 8");
    else if (!strcmp(name, "shoot")) input_inject("ZR 60");
    else if (!strcmp(name, "recentre")) input_inject("Y 6");
    else if (!strcmp(name, "menu")) menu_toggle(), wait_reads(60), menu_toggle();
    else if (!strcmp(name, "map")) input_inject("X 6"), wait_reads(70), input_inject("X 6");
    else if (!strcmp(name, "focus")) gfx_window_focus_changed(0), wait_reads(60), gfx_window_focus_changed(1);
    else if (!strcmp(name, "hitch"))
        for (int i = 1; i <= 3; i++) {
            __atomic_store_n(&stall_ns, (int64_t)i * 100000000, __ATOMIC_RELAXED);
            wait_reads(30);
        }
    else if (!strcmp(name, "yflick")) move(0, -1000, 0, 1000), wait_reads(3), move(0, 1000, 0, 1000);
    else if (!strcmp(name, "xflick")) move(3000, 0, 0, 1000), wait_reads(3), move(-3000, 0, 0, 1000);
    move_wait();
    wait_reads(90);   // a change of form back takes up to 40 frames after the action
    ev("end");
}

// The game's recentre (Y) with the view down or up, then, after the virtual controller has followed the view for 2 s,
// a vertical flick past 30 degrees of controller pitch in one frame and back (each a frame the linear pitch takes the
// curve's result as the game computed it), with the mouse still and moving.
static void situ_y_flick(double at, int mode) {
    aim_from_level(at);
    ev("situ yflickafter mode=%d at=%g", mode, at);
    input_inject("Y 6");
    wait_reads(120);
    situ_motion(mode, 2);
    wait_reads(10);
    move(0, -1000, 0, 1000);
    wait_reads(3);
    move(0, 1000, 0, 1000);
    move_wait();
    wait_reads(40);
    ev("end");
}

static void t_situ(int part) {
    static const char *names[] = {"jump", "squidjump", "shotsurface", "sub", "shoot", "special", "recentre", "menu",
                                  "map", "focus", "hitch", "yflick", "xflick"};
    typedef struct { int mode; double at; } Place;
    static const Place p1[] = {{0, 15}, {1, 15}, {2, 15}}, p2[] = {{1, -15}, {1, 35}, {2, -30}};
    const Place *p = part == 1 ? p1 : p2;
    for (size_t k = 0; k < 3; k++)
        for (size_t i = 0; i < sizeof names / sizeof *names; i++) situ(names[i], p[k].mode, p[k].at);
    for (int mode = 0; mode < 2; mode++) situ_y_flick(part == 1 ? -30 : 30, mode);
    if (part == 1) {
        aim_from_level(15);
        ev("situ idle mode=0 at=15");
        wait_reads(60 * 20);
        ev("end");
    }
}

// Large vertical turns in one frame, from a camera aimed `at` degrees up, toward level first: a flick of 1000 counts
// and back (31.8 degrees of controller pitch at 0.0318 degrees per count, 40 at 0.04), without a Y (neither leg reaches
// a limit) and after a Y with the view there; and a swipe of 1050 counts at 10000 counts/s made while the game is held
// in a 200 ms hitch, so the read after it carries them all, and back without a hitch. With mgb_native_hold off
// (release 0.3.4) then as built. Events "flick hold=<0|1> at=<deg> kind=flick|yflick|hitch", "back" and "end".
static void t_flick(void) {
    static const double at[] = {-30, 30, 20};
    static const char *const kinds[] = {"flick", "yflick", "hitch"};
    for (int hold = 0; hold < 2; hold++) {
        __atomic_store_n(&hold_resync_set, hold ? -1 : 0, __ATOMIC_RELAXED);
        for (size_t i = 0; i < sizeof at / sizeof *at; i++)
            for (int k = 0; k < 3; k++) {
                int d = at[i] < 0 ? -1 : 1;   // counts y > 0 turn down
                aim_from_level(at[i]);
                ev("flick hold=%d at=%g kind=%s", hold, at[i], kinds[k]);
                if (k == 1) input_inject("Y 6"), wait_reads(120);
                if (k < 2) {
                    move(0, 1000 * d, 0, 1000);
                } else {
                    int64_t r0 = nreads();
                    __atomic_store_n(&stall_ns, (int64_t)200000000, __ATOMIC_RELAXED);
                    while (nreads() == r0) plat_sleep_ns(200000);   // that read has begun its stall
                    move(0, 1050 * d, 10000, 1000);
                }
                wait_reads(20);
                ev("back");
                move(0, -(k < 2 ? 1000 : 1050) * d, k < 2 ? 0 : 10000, 1000);
                wait_reads(40);
                ev("end");
            }
    }
    __atomic_store_n(&hold_resync_set, -1, __ATOMIC_RELAXED);
}

// Circles of 250 counts, 1 s a turn, three turns, from a level camera, standing and walking forward (the left stick
// held), twice each. Events "walk stick=<0|1>" and "end".
static void t_walk(void) {
    for (int k = 0; k < 4; k++) {
        int stick = k & 1;
        level_camera();
        ev("walk stick=%d", stick);
        if (stick) input_inject("LSTICK 0 32767 200");
        wait_reads(10);
        circle_start(250, 1, 3, 1000, 1);
        move_wait();
        wait_reads(60);
        ev("end");
    }
}

// A match: the mouse moves for 330 s, round circles of two sizes, up and down, still, flicks and a slow drift, while
// the game goes through what it shows (the intro, the battle, the end); no finder. The view is read from the N line.
// old: with mgb_native_ref and mgb_native_hold off (release 0.3.4).
static void t_match(int old) {
    const double secs = 330;
    __atomic_store_n(&ref_set, old ? 0 : -1, __ATOMIC_RELAXED);
    __atomic_store_n(&hold_resync_set, old ? 0 : -1, __ATOMIC_RELAXED);
    ev("match old=%d", old);
    uint64_t t0 = kern_now_ns();
    int k = 0;
    while ((kern_now_ns() - t0) * 1e-9 < secs) {
        switch (k++ % 6) {
        case 0: ev("match circle r=60"); circle_start(60, 0.5, 6, 1000, 1); break;
        case 1: ev("match circle r=250"); circle_start(250, 1, 3, 1000, 1); break;
        case 2: ev("match vsine r=200"); circle_start(200, 1, 3, 1000, 0); break;
        case 3: ev("match still"); break;
        case 4: ev("match flick"); move(400, -200, 0, 1000), wait_reads(5), move(-400, 200, 0, 1000); break;
        case 5: ev("match drift up"); move(0, -150, 100, 1000), move(0, 150, 100, 1000); break;
        }
        move_wait();
        wait_reads(60);
    }
    ev("match done");
}

// A match's launches from the spawn point and super jumps (no finder, as the match test; SPLAT3_MATCH): the mouse
// still (respawn) or up and down (respawnmove: 120 counts each way, 1 s a cycle) until 15 s into the battle, through
// its start; then, until 165 s into it, rounds of a super jump back to the spawn point (X the map, DOWN the spawn
// point, A the jump) with the mouse still, up and down, or round a small circle (60 counts, 0.5 s a turn) for 8 s from
// the jump, each followed by a walk forward with ZR held while the view turns slowly (a fall from the stage respawns
// the player too). Pictures of the screen around each jump. Events "respawn ..." and "end".
static void t_respawn(int moving) {
    shots_setup();
    ev("respawn start moving=%d", moving);
    uint64_t t0 = kern_now_ns();
    while (!automatch_battle_on() && kern_now_ns() - t0 < 400000000000ull) {
        if (moving) {
            circle_start(120, 1.0, 2, 1000, 0);
            move_wait();
        } else {
            wait_reads(30);
        }
    }
    if (!automatch_battle_on()) { ev("respawn no battle"); return; }
    uint64_t go = kern_now_ns();
    ev("respawn go");
    while (kern_now_ns() - go < 15000000000ull) {
        if (moving) {
            circle_start(120, 1.0, 2, 1000, 0);
            move_wait();
        } else {
            wait_reads(30);
        }
    }
    ev("end");
    static const char *const kinds[] = {"still", "vsine", "circle"};
    for (int k = 0; (kern_now_ns() - go) * 1e-9 < 165; k++) {
        int kind = k % 3;
        ev("respawn jump=%s round=%d", kinds[kind], k);
        input_inject("X 6");
        wait_reads(40);
        __atomic_store_n(&shots_left, 1, __ATOMIC_RELAXED);
        input_inject("DOWN 6");
        wait_reads(20);
        __atomic_store_n(&shots_left, 1, __ATOMIC_RELAXED);
        wait_reads(5);
        if (kind == 1) circle_start(120, 1.0, 8, 1000, 0);
        else if (kind == 2) circle_start(60, 0.5, 16, 1000, 1);
        input_inject("A 6");
        for (int i = 0; i < 4; i++) {
            wait_reads(i ? 60 : 30);
            __atomic_store_n(&shots_left, 1, __ATOMIC_RELAXED);
        }
        move_wait();
        wait_reads(120);
        ev("end");
        ev("respawn walk round=%d", k);
        input_inject("ZR 360");
        input_inject("LSTICK 0 32767 360");
        int n = view_counts(120) * (k & 1 ? -1 : 1);
        move(n, 0, abs(n) / 6.0, 1000);
        wait_reads(30);
        ev("end");
    }
    ev("respawn done");
}

// A match where the player walks forward through the battle, so it falls from the stage or meets the other player's
// fire and respawns (on a stage whose middle is water for the first minute, picked with the match's rule stage=, a
// straight walk from the spawn point falls into it): with ZR held and the view turning 60 degrees one way then the
// other every 10 s (respawnwalk), or straight on without shooting (respawnrun; respawnrunold with release 0.3.7's
// handling of the frames in which the game holds the camera's turns, mgb_native_held). The game writes the camera's
// pitch from the frame the player is splatted (mouse_native.c: the N line's flag MOUSE_NATIVE_WRITTEN); from then until
// the respawn the mouse is still, turns up by 250 counts over 2 s, or goes up and down (120 counts each way, 1 s a
// cycle, four cycles), and in a fourth case it turns up by 250 counts over the second from the respawn, while the game
// holds the camera's turns (MOUSE_NATIVE_OFF or MOUSE_NATIVE_HELD). At the spawn point, whose camera is narrow, ZR
// launches the player. A picture every 2 s. Events "respawnwalk death=<n> mouse=<kind>", "respawnwalk hold",
// "respawnwalk launch", "end" (2 s after the camera is back to the human form's after the landing).
static void t_respawn_walk(int run) {
    shots_setup();
    ev("respawnwalk start run=%d", run);
    uint64_t t0 = kern_now_ns();
    while (!automatch_battle_on() && kern_now_ns() - t0 < 400000000000ull) wait_reads(30);
    if (!automatch_battle_on()) { ev("respawnwalk no battle"); return; }
    uint64_t go = kern_now_ns();
    ev("respawnwalk go");
    wait_reads(120);
    static const char *const kinds[] = {"still", "updead", "vsinedead", "uphold"};
    int deaths = 0, phase = 0, kind = 0, leg = 0;   // phase: 0 alive, 1 dead, 2 held, 3 at the spawn, 4 launched
    // polls in a row with the camera narrow, and not: the flags are read while the camera may be writing them, so the
    // camera's state counts once several polls agree
    int narrow = 0, wide = 0;
    int64_t phase_reads = 0, leg_reads = 0;
    for (int64_t i = 0; (kern_now_ns() - go) * 1e-9 < 170; i++) {
        wait_reads(10);
        if (i % 12 == 0) __atomic_store_n(&shots_left, 1, __ATOMIC_RELAXED);
        int flags = mouse_native_pitch_debug().flags;
        phase_reads += 10, leg_reads += 10;
        if (phase == 0 && leg_reads >= 600) {   // a leg of walking
            leg_reads = 0;
            if (!run) {
                int n = view_counts(60) * (leg++ & 1 ? -1 : 1);
                move(n, 0, abs(n) / 0.5, 1000);
                input_inject("ZR 600");
            }
            input_inject("LSTICK 0 32767 600");
        }
        int hold = (flags & (MOUSE_NATIVE_OFF | MOUSE_NATIVE_HELD)) != 0;
        narrow = flags & MOUSE_NATIVE_NARROW ? narrow + 1 : 0;
        wide = flags & MOUSE_NATIVE_NARROW ? 0 : wide + 1;
        if (phase == 0 && (flags & MOUSE_NATIVE_WRITTEN || hold)) {   // some deaths write no pitch: the hold first
            kind = deaths % 4, phase = 1, phase_reads = 0;
            ev("respawnwalk death=%d mouse=%s%s", deaths++, kinds[kind], hold ? " (seen at the hold)" : "");
            if (kind == 1) move_start(0, -250, 125, 1000, 0);
            else if (kind == 2) circle_start(120, 1.0, 4, 1000, 0);
        }
        if (phase == 1 && hold) {
            phase = 2, phase_reads = 0;
            ev("respawnwalk hold");
            if (kind == 3) move_start(0, -250, 250, 1000, 0);
        } else if (phase == 2 && !hold) {
            phase = 3, phase_reads = 0;
        } else if (phase == 3 && narrow >= 9) {   // at the spawn point
            ev("respawnwalk launch");
            input_inject("ZR 6");
            narrow = 0;
        } else if (phase == 3 && phase_reads >= 30 && wide >= 3) {
            phase = 4, phase_reads = 0;
        } else if (phase == 4 && narrow >= 12) {
            phase = 3, phase_reads = 0;
        } else if (phase == 4 && (flags & (MOUSE_NATIVE_CHANGING | MOUSE_NATIVE_NARROW))) {
            phase_reads = 0;
        } else if (phase == 4 && phase_reads >= 120) {
            ev("end");
            phase = 0, leg_reads = 600;
        }
    }
    move_wait();
    ev("respawnwalk done");
}

// The objects of TOTK_MGYRO_BENCH_OBJ: the function-entry hook for 30 reads, then off.
static void watch_find(void) {
    if (!nwfn) return;
    fnprof_on(FNPROF_BENCH);
    wait_reads(30);
    fnprof_off(FNPROF_BENCH);
    uint64_t heap = mem_layout()->heap.base;
    for (int k = 0; k < nwfn; k++)
        for (int i = 0; i < WSEEN && wseen[k][i]; i++)
            out("H,obj%d,fn=main+%#llx,%s%#llx%s", k, (unsigned long long)(wfn[k] - main_base()),
                wseen[k][i] >= heap ? "heap+" : "", (unsigned long long)(wseen[k][i] - (wseen[k][i] >= heap ? heap : 0)),
                i ? " (not watched)" : "");
    out_flush();
}

static int wants(const char *list, const char *name) {
    if (!strcmp(list, "all") || !strcmp(list, "1")) return 1;
    size_t n = strlen(name);
    for (const char *p = list; (p = strstr(p, name)); p += n)
        if ((p == list || p[-1] == ',' || p[-1] == ' ') && (p[n] == 0 || p[n] == ',' || p[n] == ' ')) return 1;
    return 0;
}

static void bench_main(void *arg) {
    (void)arg;
    const char *list = cfg_get("TOTK_MGYRO_BENCH"), *o = cfg_get("TOTK_MGYRO_BENCH_OUT"), *d = cfg_get("TOTK_MGYRO_BENCH_DELAY");
    const char *path = o && *o ? o : "mgyro-bench.csv";
    ofile = fopen(path, "w");
    if (!ofile) rt_log("mgyro bench: cannot write %s", path);
    double delay = d && *d ? atof(d) : 40;
    rt_log("mgyro bench: tests '%s', starting %.0f s after the first live six-axis read, CSV %s", list, delay, path);
    watch_parse();
    poke_parse();
    // the first read with samples, then the delay
    while (!last_s0) plat_sleep_ns(10000000);
    uint64_t t0 = kern_now_ns();
    while (kern_now_ns() - t0 < (uint64_t)(delay * 1e9)) {
        plat_sleep_ns(50000000);
        out_flush();
    }
    input_human(0);   // the player's gamepad, keys and mouse stay out of the measurement
    out("H,deg_per_count,%.8f", input_mouse_gyro_deg_per_count());   // mouse_gyro_sens, or what cm360 makes
    out("H,mouse_gyro_cm360,%s", cfg_value("TOTK_MOUSE_GYRO_CM360"));
    out("H,mouse_gyro_dpi,%s", cfg_value("TOTK_MOUSE_GYRO_DPI"));
    out("H,mouse_gyro_x,%s", cfg_value("TOTK_MOUSE_GYRO_X"));
    out("H,mouse_gyro_y,%s", cfg_value("TOTK_MOUSE_GYRO_Y"));
    out("H,mouse_gyro_pitch,%s", cfg_value("TOTK_MOUSE_GYRO_PITCH"));
    out("H,heap,%#llx+%#llx", (unsigned long long)mem_layout()->heap.base, (unsigned long long)mem_heap_size());
    int found = 0;
    if (!strcmp(list, "match") || !strcmp(list, "matchold")) {   // a match: no Y (menus may be open), no finder (the
        t_match(list[5] != 0);                                      // view is the N line's), the motion only
        found = -1;
    } else if (!strcmp(list, "respawn") || !strcmp(list, "respawnmove")) {   // a match too
        t_respawn(list[7] != 0);
        found = -1;
    } else if (!strcmp(list, "respawnwalk") || !strcmp(list, "respawnrun") || !strcmp(list, "respawnrunold")) {
        if (!strcmp(list, "respawnrunold")) __atomic_store_n(&native_held_set, 0, __ATOMIC_RELAXED);   // 0.3.7's
        t_respawn_walk(list[7] == 'r');
        found = -1;
    } else {
        // Y: the game recentres its camera behind the player, level (and the virtual controller levels with it)
        shots_setup();
        __atomic_store_n(&shots_left, 1, __ATOMIC_RELAXED);
        wait_reads(5);
        ev("start Y");
        input_inject("Y 6");
        wait_reads(60);
        __atomic_store_n(&shots_left, 1, __ATOMIC_RELAXED);
        wait_reads(5);
    }
    for (int tries = 0; tries < 6 && !found; tries++) {
        found = find_aims();
        if (!found) {
            rt_log("mgyro bench: no aim vector found (try %d); again in 10 s", tries + 1);
            plat_sleep_ns(10000000000ull);
        }
    }
    if (!found) rt_log("mgyro bench: no aim vector: the tests log the controller only");
    watch_find();
    if (wants(list, "look")) t_look();
    if (wants(list, "idle")) t_idle(10);
    if (wants(list, "lin")) t_lin(0), t_lin(1);
    if (wants(list, "step")) t_step();
    if (wants(list, "phase")) t_phase();
    if (wants(list, "rate")) t_rate();
    if (wants(list, "rt")) t_rt();
    if (wants(list, "fields")) t_fields();
    if (wants(list, "pitchyaw")) t_pitchyaw();
    if (wants(list, "yawaxis")) t_yawaxis();
    if (wants(list, "reverse")) t_reverse();
    if (wants(list, "cap")) t_cap();
    if (wants(list, "recenter")) t_recenter();
    if (wants(list, "limits")) t_limits();
    if (wants(list, "hitch")) t_hitch();
    if (wants(list, "fps")) t_fps();
    if (wants(list, "image")) t_image();
    if (wants(list, "circle")) t_circle();
    if (wants(list, "vsine")) t_vsine();
    if (wants(list, "squid")) t_squid();
    if (wants(list, "drift")) t_drift();
    if (wants(list, "ink")) t_ink();
    if (wants(list, "wall")) t_wall();
    if (wants(list, "pokecmp")) t_pokecmp();
    if (wants(list, "native")) t_native();
    if (wants(list, "sweep")) t_sweep();
    if (wants(list, "surface")) t_surface("21");
    if (wants(list, "surfacenew")) t_surface("2");
    if (wants(list, "surfacegame")) t_surface("0");
    if (wants(list, "swim")) t_swim();
    if (wants(list, "situ1")) t_situ(1);
    if (wants(list, "situ2")) t_situ(2);
    if (wants(list, "flick")) t_flick();
    if (wants(list, "walk")) t_walk();
    if (wants(list, "idle")) t_idle(10);
    ev("done");
    wait_reads(10);
    out_flush();
    rt_log("mgyro bench: done, %lld reads", (long long)nreads());
    plat_xchg32(&closed, 1);
    out_flush();
    if (ofile) fclose(ofile), ofile = NULL;
    const char *ex = cfg_get("TOTK_MGYRO_BENCH_EXIT");
    if (ex && atoi(ex)) {
        prof_write();
        plat_exit(0);
    }
}
