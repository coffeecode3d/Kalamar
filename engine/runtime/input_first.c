// Input first for Splatoon 3 11.3.0 (input_first.h). Measured with TOTK_MGYRO_BENCH's image test and timeline marks
// (CSV M and A lines, fn_trace) at 60 fps, the game's system task runs a frame as: at the end of the previous frame,
// after the frame's wait for the display (main+0x4da1360, or main+0x4da11c0 for the task's other base), it starts the
// logic jobs on the worker threads (the scene, the actors, the player camera); the framework then draws, handing the
// recordings of the previous frame to its present thread; in the calc the task's main step (main+0x4da1500) waits for
// the logic jobs, updates the controllers (sead's controller manager: every nn::hid state read of the frame) and runs
// the late jobs: the gyro processor (the component that turns the new six-axis states into the angular velocity and
// attitude the cameras use, main+0x2afaf6c) and the jobs that prepare and record the draw, which the next frame's draw
// submits. So the camera of frame N used the sensor states read and processed in frame N - 1's calc: a turn of the
// mouse read in a calc was on screen in the second present after the read (21 ms with late input), when the draw that
// follows the read, about 4 ms later, could have shown it had the logic of the read's frame run after the read.
//
// The patch set input_first (recomp/patches-splatoon3-v1130.txt) moves the controller update and the gyro processor
// before the logic jobs of the same frame. While the setting is on, the early start does not start the logic jobs
// (its flags load hides the bit that asks for it), so the main step starts them itself; just before, a host function
// updates the controllers and runs the gyro processor, as the main step and the late job would have. The main step's
// own call of the update then goes to a host function that does nothing, and the late job's run of the gyro processor
// skips the processor's work. Each frame still updates the controllers once, processes the gyro once and runs its
// logic once; only their order changes, and the logic jobs no longer overlap the start of the next frame's calc. The
// manager, its update and the gyro processor's object are the ones the game called last: a frame before those calls
// keeps the game's order, and so does a frame after one in which no late job ran the gyro processor (a pause), so a
// paused processor stays paused.
//
// A last hook counts the frame's draw where the frame function calls it (nvn_frame_drawn): the late-input gate
// (nvn_api_obj.c), which waits at the frame's first state read, now comes just after the draw that hands the previous
// frame to the present thread and usually before that frame's present, and needs to know which to aim the read at.
#include <stdint.h>
#include <string.h>

#include "cfg.h"
#include "host.h"
#include "input_first.h"
#include "nvn_api.h"
#include "platform/platform.h"
#include "splat3_addr.h"

#if __has_include("patches.h")
#include "patches.h"
#endif

static int set_index = -2;

int input_first_active(void) {
    static unsigned seen = ~0u;
    static int on;
    unsigned v = cfg_version();
    if (v != seen) {
        if (set_index == -2) set_index = rt_patch_set("input_first");
        seen = v, on = rt_patch_enabled(set_index) && cfg_bool("TOTK_INPUT_FIRST");
    }
    return on;
}

#ifdef RT_PATCH_PAGE
#define HOST_INPUT_FIRST_READ (RT_PATCH_PAGE + 0xee0)
#define HOST_INPUT_FIRST_SKIP (RT_PATCH_PAGE + 0xed0)
#define HOST_INPUT_FIRST_DRAW (RT_PATCH_PAGE + 0xf20)
#define HOST_INPUT_FIRST_NONE (RT_PATCH_PAGE + 0xf10)
#define HOST_INPUT_FIRST_EARLY (RT_PATCH_PAGE + 0xf00)
#define HOST_INPUT_FIRST_GYRO (RT_PATCH_PAGE + 0xef0)
#define MGR_UPDATE_SLOT 0xb0   // the controller manager's update in its table, as the main step calls it
#define FRAME_DRAW_SLOT 0xe8   // the framework's draw in its table, as the frame function calls it
#define EARLY_JOBS_BIT 2u      // in the early start's flags: start the logic jobs there

// The guest registers a call into the game may change, put back after it: the hook sits where the code around it keeps
// values in registers the called functions do not preserve.
typedef struct {
    uint64_t x[32], sp, nzcv, fpcr, fpsr;
    __uint128_t v[32];
} Regs;

static void regs_save(const Cpu *c, Regs *r) {
    memcpy(r->x, c->x, sizeof r->x);
    memcpy(r->v, c->v, sizeof r->v);
    r->sp = c->sp, r->nzcv = c->nzcv, r->fpcr = c->fpcr, r->fpsr = c->fpsr;
}

static void regs_restore(Cpu *c, const Regs *r) {
    memcpy(c->x, r->x, sizeof r->x);
    memcpy(c->v, r->v, sizeof r->v);
    c->sp = r->sp, c->nzcv = r->nzcv, c->fpcr = r->fpcr, c->fpsr = r->fpsr;
}

static uint64_t read64(uint64_t a) { return mem_guest_readable(a, 8) ? *(const uint64_t *)(uintptr_t)a : 0; }

static uint64_t gyro_entry(void) {
    static uint64_t e = 1;
    if (e == 1) {
        const Splat3Addr *s3 = splat3_addr();
        e = s3 && s3->gyro_processor && ldr_count() ? ldr_module(0)->base + s3->gyro_processor : 0;
    }
    return e;
}

static volatile uint64_t mgr, update;   // the manager and its update, as the main step last called them
static volatile uint64_t gyro_obj;      // the gyro processor's object, as a late job last ran it
static volatile int32_t ahead;          // the update ran before the logic jobs: the main step's call is skipped
static volatile int32_t gyro_ahead;     // the gyro processor's work ran there: a late job's run skips it
static volatile int32_t gyro_late;      // a late job ran the gyro processor since the last early update
static volatile uint64_t gyro_early;    // the guest thread (TPIDR) running the gyro processor from read_ahead, 0: none

// Before the main step starts the logic jobs (`ldr x23, [x19, #0x4c0]`, kept in the patch).
static void read_ahead(Cpu *c) {
    int gyro = __atomic_exchange_n(&gyro_late, 0, __ATOMIC_ACQ_REL);
    __atomic_store_n(&gyro_ahead, 0, __ATOMIC_RELEASE);
    if (!input_first_active()) return;
    uint64_t m = __atomic_load_n(&mgr, __ATOMIC_ACQUIRE), f = __atomic_load_n(&update, __ATOMIC_ACQUIRE);
    if (!m || !f) return;
    uint64_t g = gyro_entry(), obj = __atomic_load_n(&gyro_obj, __ATOMIC_ACQUIRE);
    Regs r;
    regs_save(c, &r);
    rt_call(c, f, 1, m);
    __atomic_store_n(&ahead, 1, __ATOMIC_RELEASE);
    if (gyro && g && obj && c->tpidr) {   // only while the late job runs it: a paused processor stays paused
        __atomic_store_n(&gyro_early, c->tpidr, __ATOMIC_RELEASE);
        rt_call(c, g, 1, obj);
        __atomic_store_n(&gyro_early, 0, __ATOMIC_RELEASE);
        __atomic_store_n(&gyro_ahead, 1, __ATOMIC_RELEASE);
    }
    regs_restore(c, &r);
}

// In place of `ldr x8, [x8, #0xb0]` before the main step's call of the update: x0 the manager, x8 its table.
static void skip_update(Cpu *c) {
    uint64_t f = read64(c->x[8] + MGR_UPDATE_SLOT);
    __atomic_store_n(&update, f, __ATOMIC_RELEASE);
    __atomic_store_n(&mgr, c->x[0], __ATOMIC_RELEASE);
    c->x[8] = __atomic_exchange_n(&ahead, 0, __ATOMIC_ACQ_REL) ? HOST_INPUT_FIRST_NONE : f;
}

// In place of the gyro processor's `bl <work>` (x0 = x19 the processor, x1 the controller it reads): the call is made
// as the game makes it (its target read back from the game's code at the return address minus 4), except in a late
// job's run after read_ahead already ran the processor this frame. The processor then goes on as usual: it zeroed its
// outputs before the work and fills them again from what the work left, the same values as after read_ahead's run.
static void gyro_work(Cpu *c) {
    if (__atomic_load_n(&gyro_early, __ATOMIC_ACQUIRE) != c->tpidr || !c->tpidr) {
        __atomic_store_n(&gyro_obj, c->x[19], __ATOMIC_RELEASE);
        __atomic_store_n(&gyro_late, 1, __ATOMIC_RELEASE);
        if (__atomic_exchange_n(&gyro_ahead, 0, __ATOMIC_ACQ_REL)) return;
    }
    uint64_t at = c->x[30] - 4;
    uint32_t w = mem_guest_readable(at, 4) ? *(const uint32_t *)(uintptr_t)at : 0;
    if ((w & 0xfc000000u) != 0x94000000u)   // bl
        rt_fatal("input first: main+%#llx is not the expected call (%08x)", (unsigned long long)(at - ldr_module(0)->base), w);
    int64_t off = (int64_t)((uint64_t)(w & 0x3ffffffu) << 38) >> 36;
    rt_call(c, at + (uint64_t)off, 0);
}

// In place of the early start's `ldr w8, [x19, #<flags>]`, the load whose bit EARLY_JOBS_BIT decides whether the logic
// jobs start there: the instruction is read back from the game's code (the return address minus 4) and done here,
// without that bit while the setting is on.
static void early_flags(Cpu *c) {
    uint64_t at = c->x[30] - 4;
    uint32_t w = mem_guest_readable(at, 4) ? *(const uint32_t *)(uintptr_t)at : 0;
    if ((w & 0xffc00000u) != 0xb9400000u)   // ldr wt, [xn, #imm]
        rt_fatal("input first: main+%#llx is not the expected load (%08x)", (unsigned long long)(at - ldr_module(0)->base), w);
    unsigned rt = w & 31, rn = (w >> 5) & 31;
    uint64_t a = (rn == 31 ? c->sp : c->x[rn]) + ((w >> 10) & 0xfff) * 4ull;
    uint32_t v = mem_guest_readable(a, 4) ? *(const volatile uint32_t *)(uintptr_t)a : 0;
    if (input_first_active()) v &= ~EARLY_JOBS_BIT;
    if (rt != 31) c->x[rt] = v;
}

// In place of `ldr x8, [x8, #0xe8]` before the frame function's call of the draw: x8 the framework's table.
static void count_draw(Cpu *c) {
    c->x[8] = read64(c->x[8] + FRAME_DRAW_SLOT);
    nvn_frame_drawn();
}

static void nothing(Cpu *c) { (void)c; }

__attribute__((constructor)) static void input_first_register(void) {
    rt_register_host(HOST_INPUT_FIRST_READ, read_ahead);
    rt_register_host(HOST_INPUT_FIRST_SKIP, skip_update);
    rt_register_host(HOST_INPUT_FIRST_DRAW, count_draw);
    rt_register_host(HOST_INPUT_FIRST_NONE, nothing);
    rt_register_host(HOST_INPUT_FIRST_EARLY, early_flags);
    rt_register_host(HOST_INPUT_FIRST_GYRO, gyro_work);
}
#endif
