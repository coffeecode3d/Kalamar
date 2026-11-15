// Splatoon 3: the start scene (start_scene / SPLAT3_START) and a trace of chosen guest function entries
// (TOTK_FN_TRACE), both through the function-entry hook every lifted function has (cpu.h FN_ENTER, main.c fnprof_hit).
//
// The main offsets below are 11.2.0's; the running build's are a row of splat3_addr.c.
//
// How the game picks its first scene. When the Boot scene is done, main+0x375338c (reached through a function
// pointer) reads the first-boot flag of the save's game data (Plaza/IsDoneFirstBootSeq): clear, it requests
// "PlayerMake" (character creation, call at main+0x3753478); set, it stores a flag of the save into the plaza's state
// and requests "Plaza" (call at main+0x37534c0). A request is main+0x3d2f440(scene manager, const char *const *name,
// flags): the manager copies the name, and the scene's pack loads afterwards on the resource thread.
//
// A start scene replaces the name of that one Plaza request: on the entry of main+0x3d2f440 whose return address is
// main+0x37534c4, x1 is pointed at the wanted name, before the function reads it. Everything else the boot choice does
// stays the game's. The hook is on from the start until that request (as resolution.c's), then off: no cost after.
// For the plaza itself the request keeps its name and the plaza's entry kind is changed, so it starts in free play.
// Without a save that finished its first boot the game goes to PlayerMake as always (save_profile's
// skip_first_launch writes that flag).
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "cfg.h"
#include "host.h"
#include "platform/platform.h"
#include "scene_start.h"
#include "splat3_addr.h"

static const Splat3Addr *s3;                // the running build's main offsets; NULL: nothing here runs
#define REQUEST_SCENE (s3->request_scene)
#define BOOT_PLAZA_CALL (s3->boot_plaza_call)             // bl REQUEST_SCENE ("Plaza")
#define BOOT_PLAYERMAKE_CALL (s3->boot_playermake_call)   // bl REQUEST_SCENE ("PlayerMake")

static uint64_t main_base;
static int start_on;                 // a start scene waits for the boot choice's request
static char start_value[64], start_scene[64];
static uint64_t *name_holder;        // guest-visible {char *name} + the name, for x1

// ---- the trace: TOTK_FN_TRACE=<main offset>[,<main offset>...] (hex), TOTK_FN_TRACE_DEPTH=<frames> (default 8)
// An entry may name guest memory to dump: <offset>@x<r>+<hex>:<hex length> (at most 64 bytes), x<r> as on entry;
// <offset>@x<r>+<hex>*<hex>:<hex length> reads a pointer at x<r>+<hex> first and dumps from that pointer plus the
// second offset (a field of an object the argument points to).
#define TRACE_MAX 32
static uint64_t trace_addr[TRACE_MAX];
static struct { int reg, deref; uint32_t off, off2, len; } trace_dump[TRACE_MAX];
static int trace_n, trace_depth = 8;
static volatile int32_t trace_count[TRACE_MAX];
#define TRACE_PER_FN 400             // lines per function, then it is counted only

static const char *describe(uint64_t a, char *buf, int n) {
    if (main_base && a - main_base < 0x10000000ull) snprintf(buf, n, "main+%#llx", (unsigned long long)(a - main_base));
    else ldr_describe(a, buf, n);
    return buf;
}

// The NUL-terminated string at a guest address, printable ASCII only, else "".
static const char *guest_str(uint64_t a, char *buf, int n) {
    buf[0] = 0;
    if (!a || (a & 0xffff000000000000ull) || !mem_guest_readable(a, (uint64_t)n)) return buf;
    int i = 0;
    for (; i < n - 1; i++) {
        char ch = *(const char *)(uintptr_t)(a + i);
        if (!ch) break;
        if (ch < 32 || ch > 126) { buf[0] = 0; return buf; }
        buf[i] = ch;
    }
    buf[i] = 0;
    return buf;
}

static void trace_hit(int k, uint64_t addr, uint64_t lr) {
    int32_t seen = __atomic_add_fetch(&trace_count[k], 1, __ATOMIC_RELAXED);
    if (seen > TRACE_PER_FN) {
        if (seen == TRACE_PER_FN + 1) rt_log("fntrace: %#llx: more calls are not logged", (unsigned long long)addr);
        return;
    }
    Cpu *c = kern_cpu();
    if (!c) return;
    char at[96], from[96], line[1400];
    int n = snprintf(line, sizeof line, "fntrace: %s #%d from %s x0=%#llx x1=%#llx x2=%#llx x3=%#llx x4=%#llx",
                     describe(addr, at, sizeof at), seen, describe(lr, from, sizeof from),
                     (unsigned long long)c->x[0], (unsigned long long)c->x[1], (unsigned long long)c->x[2],
                     (unsigned long long)c->x[3], (unsigned long long)c->x[4]);
    // Strings an argument points to, directly or through one pointer (a const char *const * or a SafeString's
    // string): scene names, job names
    for (int r = 0; r < 4 && n < (int)sizeof line - 200; r++) {
        char s[80];
        uint64_t v = c->x[r];
        if (guest_str(v, s, sizeof s)[0] && strlen(s) >= 3) n += snprintf(line + n, sizeof line - n, " x%d->\"%s\"", r, s);
        else if (v && mem_guest_readable(v, 16)) {
            uint64_t p0 = *(const uint64_t *)(uintptr_t)v, p1 = *(const uint64_t *)(uintptr_t)(v + 8);
            if (guest_str(p0, s, sizeof s)[0] && strlen(s) >= 3) n += snprintf(line + n, sizeof line - n, " *x%d->\"%s\"", r, s);
            else if (guest_str(p1, s, sizeof s)[0] && strlen(s) >= 3)
                n += snprintf(line + n, sizeof line - n, " *(x%d+8)->\"%s\"", r, s);
        }
    }
    if (trace_dump[k].len) {
        uint64_t a = c->x[trace_dump[k].reg] + trace_dump[k].off;
        if (trace_dump[k].deref)
            a = a && !(a >> 48) && mem_guest_readable(a, 8) ? *(const uint64_t *)(uintptr_t)a + trace_dump[k].off2 : 0;
        n += snprintf(line + n, sizeof line - n, trace_dump[k].deref ? " [[x%d+%#x]+%#x]=" : " [x%d+%#x]=",
                      trace_dump[k].reg, trace_dump[k].off, trace_dump[k].off2);
        if (a && !(a >> 48) && mem_guest_readable(a, trace_dump[k].len))
            for (uint32_t i = 0; i < trace_dump[k].len && n < (int)sizeof line - 300; i++)
                n += snprintf(line + n, sizeof line - n, "%02x%s", *(const uint8_t *)(uintptr_t)(a + i),
                              (i & 3) == 3 ? " " : "");
        else n += snprintf(line + n, sizeof line - n, "unreadable");
    }
    // The guest frame-pointer chain: each frame is {caller's x29, return address}
    uint64_t fp = c->x[29];
    n += snprintf(line + n, sizeof line - n, " stack");
    for (int d = 0; d < trace_depth && fp && !(fp & 7) && mem_guest_readable(fp, 16) && n < (int)sizeof line - 40; d++) {
        uint64_t ret = *(const uint64_t *)(uintptr_t)(fp + 8);
        n += snprintf(line + n, sizeof line - n, " %s", describe(ret, from, sizeof from));
        uint64_t next = *(const uint64_t *)(uintptr_t)fp;
        if (next <= fp) break;
        fp = next;
    }
    rt_log("%s", line);
}

// The local lobby (scene LobbyLocal) plays over local wireless (nn::ldn, network start mode 2, job "InitLocal") or,
// with its state's IsLan byte set, over LAN (mode 3, job "InitLan"). The game sets that byte itself when the player
// holds ZL+ZR and the left stick for 181 frames in the local lobby (main+0x3955360: IsLan = 1, then the scene is
// entered again, "ReenterScene"). The scene states hang off main+0x60f24e0: object +0xd8 -> the next scene's states,
// +0x6678 the local lobby's ({+0x30 IsLan, +0x34 Mode, +0x38 MyUserId}, the names its serializer main+0x34f8ee4
// writes). The boot choice holds that object's mutex (+0x100) around its request, so the byte is written under it.
#define SCENE_STATES (s3->scene_states)
static int start_lan;     // lobby-lan
static int start_plaza;   // plaza: in free play, without the title screen

// A scene's state object for the next scene: *(*(*(main+0x60f24e0) + 0xd8) + off), 0 when unreadable.
static uint64_t scene_state(uint64_t off) {
    if (!s3) return 0;
    uint64_t p = main_base + SCENE_STATES;
    for (int i = 0; i < 2; i++) {   // *(main+0x60f24e0), then +0xd8
        if (!mem_guest_readable(p, 8)) return 0;
        p = *(const uint64_t *)(uintptr_t)p + (i ? off : 0xd8);
    }
    if (!mem_guest_readable(p, 8)) return 0;
    uint64_t s = *(const uint64_t *)(uintptr_t)p;
    return s && mem_guest_readable(s + 0x30, 16) ? s : 0;
}

static int set_lobby_local_lan(void) {
    uint64_t ll = scene_state(0x6678);
    if (!ll) return 0;
    *(volatile uint8_t *)(uintptr_t)(ll + 0x30) = 1;
    return 1;
}

// ---- the census: TOTK_FN_CENSUS=<file> counts the entries of every function of main in named windows, to find the
// functions an action runs (a window around a press against windows without it). fn_census_mark("<name>") (or the
// live input line "CENSUS <name>", input.c) ends the current window, appending "<window> <main offset> <count>" per
// function entered to the file, and starts the next one; "CENSUS off" ends the last. The hook runs on every function
// entry only while a window is open.
static uint32_t *census;            // entries per 4-byte slot of main's text
static uint64_t census_slots;
static char census_path[512], census_win[64];
static PlatMutex census_lock = PLAT_MUTEX_INIT;

void fn_census_mark(const char *name) {
    if (!census) return;
    plat_mutex_lock(&census_lock);
    fnprof_off(FNPROF_TRACE * !trace_n);   // the trace keeps the hook on if it is set
    if (census_win[0]) {
        FILE *f = fopen(census_path, "ab");
        uint64_t fns = 0;
        for (uint64_t i = 0; i < census_slots; i++)
            if (census[i]) {
                if (f) fprintf(f, "%s %llx %u\n", census_win, (unsigned long long)i * 4, census[i]);
                census[i] = 0, fns++;
            }
        if (f) fclose(f);
        rt_log("fncensus: window %s: %llu functions entered", census_win, (unsigned long long)fns);
    }
    snprintf(census_win, sizeof census_win, "%s", strcmp(name, "off") ? name : "");
    for (char *p = census_win; *p; p++)
        if (*p == ' ' || *p == '\r' || *p == '\n') *p = '_';
    if (census_win[0]) fnprof_on(FNPROF_TRACE);
    plat_mutex_unlock(&census_lock);
}

void scene_fn_hit(uint64_t addr, uint64_t lr, uint64_t x0) {
    (void)x0;
    if (census && census_win[0]) {
        uint64_t o = (addr - main_base) >> 2;
        if (o < census_slots) census[o]++;   // racy by design: a count, not an exact one
    }
    if (__builtin_expect(trace_n, 0))
        for (int k = 0; k < trace_n; k++)
            if (trace_addr[k] == addr) { trace_hit(k, addr, lr); break; }
    if (!start_on || addr != main_base + REQUEST_SCENE) return;
    Cpu *c = kern_cpu();
    if (!c) return;
    if (lr == main_base + BOOT_PLAYERMAKE_CALL + 4) {
        start_on = 0;
        fnprof_off(FNPROF_SCENE);
        rt_log("start scene: the save has not finished its first boot; the game creates the character as usual "
               "(start_scene=%s waits for a save with Plaza/IsDoneFirstBootSeq, save_profile's skip_first_launch)",
               start_value);
        return;
    }
    if (lr != main_base + BOOT_PLAZA_CALL + 4) return;
    start_on = 0;
    fnprof_off(FNPROF_SCENE);
    if (start_plaza) {
        // The plaza's state (+0x6658) holds at +0x30 how the plaza is entered; the boot choice has just stored 1 there
        // (from Boot: the title screen, then the news). 6 is the value the game stores when it sends the player back to
        // the plaza after a network error (main+0x3b06890): the plaza then starts in free play. Measured: 0 and 1 show
        // the title, 6 does not.
        uint64_t pz = scene_state(0x6658);
        if (!pz) {
            rt_log("start scene: the plaza's state was not found; the title screen stays");
        } else {
            rt_log("start scene: the plaza starts in free play (entry %u -> 6: no title screen, no news)",
                   *(const uint32_t *)(uintptr_t)(pz + 0x30));
            *(volatile uint32_t *)(uintptr_t)(pz + 0x30) = 6;
        }
    }
    if (start_lan && !set_lobby_local_lan()) {
        rt_log("start scene: the local lobby's state was not found; start_scene=%s ignored", start_value);
        return;
    }
    c->x[1] = (uint64_t)(uintptr_t)name_holder;
    rt_log("start scene: the boot choice requests \"%s\"%s (start_scene=%s%s)", start_scene,
           strcmp(start_scene, "Plaza") ? " instead of \"Plaza\"" : "", start_value,
           start_lan ? ", the local lobby in LAN mode" : "");
}

// ---- settings
static const struct { const char *value, *scene; int lan; } starts[] = {
    {"plaza", "Plaza", 0}, {"lobby", "LobbyVersus", 0}, {"lobby-lan", "LobbyLocal", 1},
    {"range", "ShootingRange", 0}, {"story", "SmallWorld", 0},
};

static int code_matches(void) {
    // The two call sites must be bl REQUEST_SCENE, so another build of main is never patched blind
    uint64_t sites[2] = {BOOT_PLAZA_CALL, BOOT_PLAYERMAKE_CALL};
    for (int i = 0; i < 2; i++) {
        uint64_t a = main_base + sites[i];
        if (!mem_guest_readable(a, 4)) return 0;
        uint32_t want = 0x94000000u | (uint32_t)(((REQUEST_SCENE - sites[i]) >> 2) & 0x3ffffffu);
        if (*(const uint32_t *)(uintptr_t)a != want) return 0;
    }
    return 1;
}

const char *scene_start_value(void) { return start_value[0] ? start_value : NULL; }

void scene_start_init(void) {
    if (rt_target.family != RT_GAME_SPLATOON3 || !ldr_count()) return;
    main_base = ldr_module(0)->base;
    const char *tr = cfg_get("TOTK_FN_TRACE");
    if (tr && *tr) {
        for (const char *p = tr; *p && trace_n < TRACE_MAX;) {
            char *end;
            unsigned long long off = strtoull(p, &end, 16);
            if (end == p) break;
            int r;
            unsigned doff, doff2, dlen, k = 0;
            if (sscanf(end, "@x%d+%x*%x:%x%n", &r, &doff, &doff2, &dlen, &k) == 4 && k && r >= 0 && r < 31) {
                trace_dump[trace_n].reg = r, trace_dump[trace_n].off = doff;
                trace_dump[trace_n].deref = 1, trace_dump[trace_n].off2 = doff2;
                trace_dump[trace_n].len = dlen > 64 ? 64 : dlen;
                end += k;
            } else if (k = 0, sscanf(end, "@x%d+%x:%x%n", &r, &doff, &dlen, &k) == 3 && k && r >= 0 && r < 31) {
                trace_dump[trace_n].reg = r, trace_dump[trace_n].off = doff;
                trace_dump[trace_n].len = dlen > 64 ? 64 : dlen;
                end += k;
            }
            trace_addr[trace_n++] = main_base + off;
            while (*end && *end != ',') end++;
            p = end + (*end == ',');
        }
        const char *d = cfg_get("TOTK_FN_TRACE_DEPTH");
        if (d && atoi(d) >= 0) trace_depth = atoi(d) > 32 ? 32 : atoi(d);
        if (trace_n) {
            fnprof_on(FNPROF_TRACE);
            rt_log("fntrace: %d functions of main, %d stack frames (every function entry checks them: slower)", trace_n,
                   trace_depth);
        }
    }
    const char *cs = cfg_get("TOTK_FN_CENSUS");
    if (cs && *cs) {
        census_slots = ldr_module(0)->text_end / 4;
        census = calloc(census_slots, sizeof *census);
        snprintf(census_path, sizeof census_path, "%s", cs);
        rt_log("fncensus: %s, %llu slots (windows: CENSUS <name> in TOTK_INPUT_LIVE)", census ? cs : "no memory",
               (unsigned long long)census_slots);
    }
    const char *v = cfg_get("SPLAT3_START");
    if (!v || !*v) v = cfg_get("TOTK_START_SCENE");
    if (!v || !*v || !strcmp(v, "off") || !strcmp(v, "title")) return;
    const char *scene = NULL;
    for (size_t i = 0; i < sizeof starts / sizeof *starts; i++)
        if (!strcmp(v, starts[i].value)) scene = starts[i].scene, start_lan = starts[i].lan;
    start_plaza = !strcmp(v, "plaza");
    if (!scene && !strncmp(v, "scene:", 6) && v[6]) scene = v + 6;   // any scene pack, for experiments
    if (!scene) {
        rt_log("start scene: unknown value \"%s\" (plaza, lobby, lobby-lan, range, story, scene:<Name>); ignored", v);
        return;
    }
    s3 = splat3_addr();
    if (!s3) {
        rt_log("start scene: no main offsets for target %s; start_scene=%s ignored", rt_target.name, v);
        return;
    }
    if (!code_matches()) {
        rt_log("start scene: main does not have the expected boot choice; start_scene=%s ignored", v);
        return;
    }
    snprintf(start_value, sizeof start_value, "%s", v);
    snprintf(start_scene, sizeof start_scene, "%s", scene);
    name_holder = mem_kernel_alloc(128, 16);
    memcpy(name_holder + 2, start_scene, strlen(start_scene) + 1);
    name_holder[0] = (uint64_t)(uintptr_t)(name_holder + 2);
    start_on = 1;
    fnprof_on(FNPROF_SCENE);
    rt_log("start scene: %s (scene %s)", start_value, start_scene);
}
