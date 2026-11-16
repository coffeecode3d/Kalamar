// Splatoon 3: automatic LAN match entry and the match result, for a launcher that is the menu and starts the
// game only for one match (docs/auto-match.md).
//
// SPLAT3_MATCH=host|join, with LAN play on (SPLAT3_LAN) and SPLAT3_START=lobby-lan: after boot the runtime enters the
// game's LAN private battle by itself with the presses a player makes, each one gated on an effect the game shows:
//   - the RomFS files it opens (romfs.c calls automatch_file): Pack/Scene/LobbyLocal (the local lobby),
//     Pack/Scene/Vss_<stage> (a match's stage), and the stage intro's files (INTRO_FILE);
//   - the session library's LAN sockets (lan_net.c's observer): the session list starts its network (UDP 35000 bound) and sends a
//     search (a broadcast starting with byte 0); a host answers it (a datagram from another address starting with byte
//     1); creating or joining a session restarts the network (35000 closed and bound again); a member's session
//     traffic arrives on UDP 49152;
//   - the nifm local-network request (the lobby's LAN network start, job InitLan), and in relay mode the relay room's
//     member events (their names: SPLAT3_MATCH_RULES teams by name).
// A step whose effect does not follow is retried until the step's time is up (SPLAT3_MATCH_TIMEOUTS). Measured flow
// (docs/auto-match.md): host L, A (session list), Y, A (session created), then with every player in: Down, Down, A
// (mode and stage ready), on the team screen (the teams already in the save) Down per member and once more, A (teams
// ready), A (OK at the pre-match screen); guest L, A (list), A, A (join), A (OK). On the room screen each press waits
// for the screen's own state and is spaced in the game's frames (host_setup), so a slow machine takes the same steps.
//
// Out of sight: the window opens hidden (automatch_window_hidden), the sound is muted and the player's own input is not
// read until the match is presented (SPLAT3_MATCH_REVEAL); a failed flow never shows the half-driven lobby: it writes
// the error and closes the game (SPLAT3_MATCH_ON_ERROR). SPLAT3_MATCH_STATUS gets the flow's state at every change.
//
// The result: when the match ends the game stores it for the local lobby in that scene's state, LastVersusRecord
// (the state's serializer main+0x34f894c reads it at +0x40 with the record reader main+0x3d80050: Rule +0x30,
// StageId +0x94, DateTimeUtc +0x98, WinTeam +0xa0, BattleFrame +0xa4, TeamRecordArray {buffer +0xa8, capacity +0xb0,
// start +0xb4, count +0xb8; 0x1b8 bytes each}, PersonalRecordArray {+0x5e8, +0x5f0, +0x5f4, +0x5f8; 0xe60 bytes each}).
// It is read when it appears and written as JSON to SPLAT3_MATCH_RESULT; SPLAT3_MATCH_EXIT=1 then hides the window and
// closes the game as its window's close button does.
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "auto_match.h"
#include "brand.h"
#include "cfg.h"
#include "fs.h"
#include "gfx.h"
#include "host.h"
#include "input.h"
#include "json_esc.h"
#include "lan.h"
#include "platform/platform.h"
#include "save_profile.h"
#include "scene_start.h"
#include "splat3_addr.h"
#include "utf16.h"
#include "vk.h"

void gfx_window_request_close(void);   // gfx_window_win.c / gfx_window_sdl.c: as the window's close button

volatile int automatch_on;

// The main offsets cited in this file's comments are 11.2.0's; the running build's are a row of splat3_addr.c.
static const Splat3Addr *s3;        // set by automatch_init before any hook is on
#define SCENE_STATES (s3->scene_states)   // main: the scene states' holder (scene_start.c)
#define LOBBY_LOCAL_STATE 0x6678ull // the local lobby's state in a states object
#define REC_OFF 0x40                // LastVersusRecord in the local lobby's state
#define REC_SIZE 0x7904             // up to MyPersonalIndex
#define TEAM_SIZE 0x1b8
#define PERSON_SIZE 0xe60
#define PORT_DISCOVERY 35000
#define PORT_SESSION 49152
#define PEER_SILENT_MS 20000        // a room member whose session traffic stopped that long has left
#define GUEST_RESULTS_MS 20000      // a guest presses A that long on its result screens
// In a match: the flow stops (connection-lost) once the session has been silent this long and the game has not ended
// the match. The game shows its own "communication error" 2 to 5 s after the other game vanished and never ends the
// match by itself (measured, docs/auto-match.md "A player who leaves"): a match of two players cannot go on without the
// other one (15 s); with more players the others may play on (45 s, not measured). SPLAT3_MATCH_SILENT sets it.
#define MATCH_SILENT_MS 45000
#define MATCH_SILENT_DUEL_MS 15000
#define LEAVE_HEARD_MS 2000         // a departure is "in play" when every other player was heard this recently

enum { HOST = 1, JOIN = 2 };
enum { REVEAL_INTRO, REVEAL_LOADING, REVEAL_NEVER };
static int round_no = 1;   // the round (SPLAT3_MATCH_CMD next): 1 for the first match
static int role, players = 2, want_exit, play_s, reveal_mode = REVEAL_INTRO, error_show, diag, bench_teams;
static char match_id[65] = "none", result_path[512], status_path[512], shots_dir[512], host_name[64], teams[17],
            self_name[64];
static uint64_t main_base, t0_ms;
static double t0_unix;   // wall clock (UTC seconds) at t0_ms, so a launcher can relate the timeline to its own clock

// Teams by name (SPLAT3_MATCH_RULES teams=<name>:<A|B|->,...): relay mode, where the relay names each member
static struct { char name[33]; char team; } by_name[10];   // the room has 10 places (8 players, 2 spectators)
static int nby_name;

// The room's settings (SPLAT3_MATCH_RULES mode=, stage=, the other settings): what the host's room is set to
static const char *const mode_keys[] = {"turf", "zones", "tower", "rainmaker", "clams"};   // the mode selector's order
#define MODES 5               // the room's mode selector offers the game's rules 0-4 (measured: Right cycles 0..4, 0)
// The room's "other settings" screen, top to bottom: gear's sub abilities off, the gear a figure (the game's NFC figures)
// brings, the names figures bring, camera and player settings reset at each match. Neutral keys; opt_alias: the keys
// of earlier builds, still taken as input (never listed nor written)
static const char *const opt_keys[] = {"main_abilities_only", "figure_gear", "figure_names", "reset_settings"};
static const char *const opt_alias[] = {"", "amiibo_gear", "amiibo_names", ""};
#define OPTS 4
static int want_rule, want_stage = -1, want_opt[OPTS];   // rule id; VersusSceneInfo Id or -1 random; 0/1 each
static char want_stage_scene[64] = "random";
static char rule_names[8][8];  // the game's rule names, its VersusRule enum text read at start (rule_names_read)
static int nrule_names;

// Each step's time limit in seconds (SPLAT3_MATCH_TIMEOUTS=<step>=<s>,...)
enum { T_BOOT, T_LIST, T_CREATE, T_PLAYERS, T_JOIN, T_START, T_STAGE, T_MATCH, T_RESULTS, T_N };
static const char *const t_name[T_N] = {"boot", "list", "create", "players", "join", "start", "stage", "match",
                                        "results"};
static int t_s[T_N] = {300, 90, 60, 300, 300, 360, 90, 900, 120};

static const char *role_name(void) { return role == HOST ? "host" : role == JOIN ? "join" : "none"; }

// This player's seat in the match, apart from its role in the session (host or guest): a player of a team, or a
// spectator (the room's stands, team letter S). seat_rules: what the rules say (the host's own row; a guest named in
// teams by name, contract 2 giving every participant the rules); seat_game: what the game showed (record_seat, from the
// match's record), which wins. "unknown": a guest the rules do not name, before its match's record.
enum { SEAT_UNKNOWN, SEAT_PLAYER, SEAT_SPECTATOR };
static int seat_rules, seat_game;
static int seat_now(void) { return seat_game ? seat_game : seat_rules; }
static const char *seat_word(int s) { return s == SEAT_PLAYER ? "player" : s == SEAT_SPECTATOR ? "spectator" : "unknown"; }

static double wall_now(void) {
    struct timespec ts;
    return timespec_get(&ts, TIME_UTC) ? (double)ts.tv_sec + ts.tv_nsec / 1e9 : 0;
}

// ---- what the game shows (counters only grow; the flow compares them with a value read before a press)
static volatile int32_t n_nifm, n_nifm_failed, n_bind, n_close, n_reply, n_peers, seq_lobby, seq_vss, seq_intro;
static PlatMutex lock = PLAT_MUTEX_INIT;
static uint32_t peers[16], host_ip;   // members in the order their session traffic started; the session's host
static uint64_t peer_seen[16];        // plat_now_ms of each member's last session datagram
static char vss_name[64];

static double now_s(void) { return (double)(plat_now_ms() - t0_ms) / 1000.0; }
static int readable(uint64_t a, uint64_t n);
static uint64_t rd64(uint64_t a);
static uint32_t rd32(uint64_t a);
#define LOAD(x) __atomic_load_n(&(x), __ATOMIC_ACQUIRE)
#define BUMP(x) __atomic_add_fetch(&(x), 1, __ATOMIC_ACQ_REL)

static struct { char what[48]; double t; } timeline[40];
static int ntimeline;
static void mark(const char *what) {
    if (ntimeline < 40) {
        snprintf(timeline[ntimeline].what, sizeof timeline[0].what, "%s", what);
        timeline[ntimeline++].t = now_s();
    }
    rt_log("automatch: +%.1f s %s", now_s(), what);
}

static void hexdump(const char *what, const uint8_t *p, long n) {
    rt_log("automatch: %s, %ld bytes:", what, n);
    for (long i = 0; i < n && i < 704; i += 32) {
        char line[120];
        int k = 0;
        for (long j = i; j < n && j < i + 32; j++) k += snprintf(line + k, sizeof line - k, "%02x", p[j]);
        rt_log("automatch:   %04lx %s", i, line);
    }
}

// Every string of the status and result files goes through json_esc.h (JE, json_esc): some come from other players
// (the room's stage name, the players' names in the match's record) and must stay inside their JSON string.
//
// A JSON member list built in a buffer (the room, the stage list): appended with jcat, which leaves *k at -1 once a
// piece does not fit, and jdone drops the whole list then, so a cut list never makes the file invalid.
static void jcat(char *out, size_t cap, int *k, const char *fmt, ...) {
    if (*k < 0) return;
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(out + *k, cap - (size_t)*k, fmt, ap);
    va_end(ap);
    *k = n < 0 || (size_t)*k + (size_t)n >= cap ? -1 : *k + n;
}

static void jdone(char *out, int k, const char *what) {
    if (k >= 0) return;
    out[0] = 0;
    rt_log("automatch: %s does not fit its buffer: left out", what);
}

// ---- the status file (SPLAT3_MATCH_STATUS): rewritten whole (a temporary file renamed over it) at every change
static PlatMutex status_lock = PLAT_MUTEX_INIT;
static const char *cur_state = "";
static double cur_since;
static char status_room[4096];   // JSON members about the room, added to every status once known (room_status)
static char status_save[200];   // JSON members about the match's save (match_save_ok), once its copy is ready
static char status_leave[120];  // JSON members of a departure's error status (depart): in_play, session_silent_ms
// SPLAT3_MATCH_LOADOUT's weapon and gear (-1: the save's own), checked against this player's record entry (write_result)
static int lo_on;
static int64_t lo_weapon = -1, lo_gear[3] = {-1, -1, -1};
static volatile int32_t shown;   // the window is shown (reveal)

static void status_write(const char *state, const char *code, int present, int expected, const char *detail) {
    plat_mutex_lock(&status_lock);
    if (strcmp(cur_state, state)) cur_state = state, cur_since = wall_now();
    double since = cur_since;
    plat_mutex_unlock(&status_lock);
    rt_log("automatch: +%.1f s status %s%s%s: %s", now_s(), state, code ? " " : "", code ? code : "", detail);
    if (!status_path[0]) return;
    char tmp[600], d[1200];
    snprintf(tmp, sizeof tmp, "%s.tmp", status_path);
    json_esc(d, sizeof d, detail);
    plat_mutex_lock(&status_lock);
    FILE *f = fopen(tmp, "wb");
    if (f) {
        fprintf(f, "{\"version\": 1, \"match_id\": \"%s\", \"role\": \"%s\", \"seat\": \"%s\", \"state\": \"%s\", "
                   "\"since_unix\": %.3f, \"detail\": \"%s\"",
                JE(match_id), JE(role_name()), JE(seat_word(seat_now())), JE(state), since, d);
        if (expected) fprintf(f, ", \"present\": %d, \"expected\": %d", present, expected);
        if (code) fprintf(f, ", \"error\": \"%s\", \"reason\": \"%s\"", JE(code), d);
        if (code && status_leave[0]) fprintf(f, ", %s", status_leave);
        if (status_save[0]) fprintf(f, ", %s", status_save);
        if (status_room[0]) fprintf(f, ", %s", status_room);
        fprintf(f, ", \"visible\": %s, \"pid\": %u}\n", LOAD(shown) ? "true" : "false", plat_process_id());
        fclose(f);
        if (!plat_file_replace(tmp, status_path)) rt_log("automatch: cannot move %s to %s", tmp, status_path);
    } else {
        rt_log("automatch: cannot write %s", tmp);
    }
    plat_mutex_unlock(&status_lock);
}

static void status(const char *state, const char *fmt, ...) {
    char detail[400];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(detail, sizeof detail, fmt, ap);
    va_end(ap);
    status_write(state, NULL, 0, 0, detail);
}

// ---- out of sight until the match: the window, the sound and the player's input
int automatch_window_hidden(void) { return automatch_on && !LOAD(shown); }

static int nshot;
static void shot(const char *step);

static void reveal(const char *why) {
    if (plat_xchg32(&shown, 1)) return;
    if (shots_dir[0]) {   // the next presented frame: the first one the player sees
        char l[700];
        snprintf(l, sizeof l, "SHOT %s/reveal-%s-%s.png", shots_dir, role == HOST ? "host" : "join", match_id);
        input_inject(l);
    }
    input_human(1);
    audio_mute(0);
    gfx_window_show(1);
    char m[64];
    snprintf(m, sizeof m, "revealed (%s)", why);
    mark(m);
}

static void hide(void) {
    plat_xchg32(&shown, 0);
    input_human(0);
    audio_mute(1);
    gfx_window_show(0);
}

// ---- presentation pace and the intro's first frames: the presents (and the game's NVN calls) counted from this
// thread, in windows of 10 s, or of 250 ms around the stage's load (frames_fine), as hidden and shown. A hidden window
// that throttled the presents (and the game, which waits for them) shows here.
// The intro on screen: the stage's loading screen presents in fits (the stage's first frames build their pipelines:
// measured 0 presents for 4 s on one stage, 1-12 per 250 ms on another) and the intro's music opens during
// or before them; the first two windows in a row at 13 presents or more (52/s) that begin after the intro's music are
// the intro running (measured: 1.3 s after the music, the rule's title over the stage). NVN calls
// per present do not tell the loading screen from the intro (4000-8000 on both).
#define STEADY_PRESENTS 13
static volatile int32_t frames_fine, seq_intro_frames, seq_vss_go;
static void frames_main(void *arg) {
    (void)arg;
    GfxStats g = gfx_stats();
    uint64_t last = g.frames, t_last = plat_now_ns(), w0 = t_last, n = 0, worst = 0, c0 = g.nvn_calls;
    int was_shown = LOAD(shown), after_music = 0, steady = 0;
    for (;;) {
        plat_sleep_ms(1);
        g = gfx_stats();
        uint64_t t = plat_now_ns();
        int s = LOAD(shown), fine = LOAD(frames_fine);
        if (g.frames != last) {
            if (t - t_last > worst && n) worst = t - t_last;
            n += g.frames - last, last = g.frames, t_last = t;
        }
        uint64_t span = fine ? 250000000ull : 10000000000ull;
        if (t - w0 >= span || s != was_shown) {
            uint64_t cpp = n ? (g.nvn_calls - c0) / n : 0;
            if (t > w0 && (n || t - w0 > 500000000ull || fine))
                rt_log("automatch: +%.2f s frames %s: %.2f s, %llu presents, mean %.2f ms, worst %.2f ms, %llu NVN calls "
                       "per present", now_s(), was_shown ? "shown" : "hidden", (t - w0) / 1e9, (unsigned long long)n,
                       n ? (t - w0) / 1e6 / (double)n : 0.0, worst / 1e6, (unsigned long long)cpp);
            int whole = t - w0 >= span;
            steady = fine && whole && after_music && n >= STEADY_PRESENTS ? steady + 1 : 0;
            if (steady >= 2 && !LOAD(seq_intro_frames)) {
                rt_log("automatch: +%.2f s the intro is on screen: presents steady after its music", now_s());
                BUMP(seq_intro_frames);
            }
            w0 = t, n = 0, worst = 0, was_shown = s, c0 = g.nvn_calls, after_music = LOAD(seq_intro) != 0;
        }
    }
}

// ---- the network
// The host name filter (SPLAT3_MATCH_HOST). A host's answer carries its player name twice (measured on 11.2.0, the
// answers logged in docs/auto-match.md's runs): in UTF-8 after its byte count (32-bit big endian) and a byte 01, with
// the next field's bytes right after it, and in UTF-16LE between NUL units. Only the whole name counts, in either
// form: "Alpha" is not in an answer from "Alpha2", "TheAlpha" or "Alph" (a substring match took them all).
static int has_name(const uint8_t *p, long n) {
    size_t k = strlen(host_name);
    uint16_t w16[64];
    size_t wk = utf8_to_utf16(host_name, w16, 64);
    uint8_t w[128];
    for (size_t i = 0; i < wk; i++) w[2 * i] = (uint8_t)w16[i], w[2 * i + 1] = (uint8_t)(w16[i] >> 8);
    if (!k || !wk) return 0;
    for (long i = 0; i < n; i++) {
        if (i >= 5 && i + (long)k <= n && !memcmp(p + i, host_name, k) &&
            ((uint32_t)p[i - 5] << 24 | (uint32_t)p[i - 4] << 16 | (uint32_t)p[i - 3] << 8 | p[i - 2]) == k)
            return 1;
        long e = i + 2 * (long)wk;   // the unit after the name
        if (e <= n && !memcmp(p + i, w, 2 * wk) && (i < 2 || (!p[i - 2] && !p[i - 1])) &&
            (e + 2 > n || (!p[e] && !p[e + 1])))
            return 1;
    }
    return 0;
}

static int on_net(int ev, uint16_t lport, uint32_t ip, const void *data, long n) {
    const uint8_t *p = data;
    uint32_t self = lan_cfg()->ip;
    static volatile int32_t told_search, told_reply, told_drop;
    char a[16];
    switch (ev) {
    case LAN_OBS_NIFM: BUMP(n_nifm); break;
    case LAN_OBS_NIFM_FAILED: BUMP(n_nifm_failed); break;
    case LAN_OBS_BIND: if (lport == PORT_DISCOVERY) BUMP(n_bind); break;
    case LAN_OBS_CLOSE: if (lport == PORT_DISCOVERY) BUMP(n_close); break;
    case LAN_OBS_RECV:
        if (lport == PORT_DISCOVERY && n > 0) {
            if (ip == self) {   // our own search, as the stack hands a broadcast back
                if (!plat_xchg32(&told_search, 1)) hexdump("the session list's search (own broadcast)", p, n);
            } else if (p[0] != 0) {   // a host's answer
                if (role == JOIN && host_name[0] && !has_name(p, n)) {
                    if (!plat_xchg32(&told_drop, 1))
                        rt_log("automatch: a session answer from %s without the host name \"%s\" dropped", lan_ip_str(ip, a),
                               host_name);
                    return 0;
                }
                if (!plat_xchg32(&told_reply, 1)) hexdump("a host's answer to the search", p, n);
                plat_mutex_lock(&lock);
                if (!host_ip) {
                    host_ip = ip;
                    rt_log("automatch: +%.1f s a session answer from %s", now_s(), lan_ip_str(ip, a));
                } else if (host_ip != ip) {
                    rt_log("automatch: +%.1f s another session answer, from %s (the list's first: %s)", now_s(),
                           lan_ip_str(ip, a), lan_ip_str(host_ip, (char[16]){0}));
                }
                plat_mutex_unlock(&lock);
                BUMP(n_reply);
            }
        } else if (lport == PORT_SESSION && ip != self) {
            plat_mutex_lock(&lock);
            int k = 0;
            while (k < 16 && peers[k] && peers[k] != ip) k++;
            if (k < 16) {
                if (!peers[k]) {
                    peers[k] = ip;
                    rt_log("automatch: +%.1f s session traffic from a new member %s", now_s(), lan_ip_str(ip, a));
                    BUMP(n_peers);
                }
                peer_seen[k] = plat_now_ms();
            }
            plat_mutex_unlock(&lock);
        }
        break;
    }
    return 1;
}

// Members whose session traffic is recent; *silent: the longest silence of a known member, ms
static int members_present(uint64_t *silent) {
    uint64_t now = plat_now_ms(), worst = 0;
    int k = 0;
    plat_mutex_lock(&lock);
    for (int i = 0; i < 16 && peers[i]; i++) {
        uint64_t q = now - peer_seen[i];
        if (q < PEER_SILENT_MS) k++;
        if (q > worst) worst = q;
    }
    plat_mutex_unlock(&lock);
    if (silent) *silent = worst;
    return k;
}

static uint64_t host_silent_ms(void) {
    uint64_t q = 0;
    plat_mutex_lock(&lock);
    for (int i = 0; i < 16 && peers[i]; i++)
        if (peers[i] == host_ip) q = plat_now_ms() - peer_seen[i];
    plat_mutex_unlock(&lock);
    return q;
}

// The session's silence: the time since the most recent traffic of any member (0 with no member yet)
static uint64_t session_silent_ms(void) {
    uint64_t now = plat_now_ms(), q = UINT64_MAX;
    plat_mutex_lock(&lock);
    for (int i = 0; i < 16 && peers[i]; i++)
        if (now - peer_seen[i] < q) q = now - peer_seen[i];
    plat_mutex_unlock(&lock);
    return q == UINT64_MAX ? 0 : q;
}

// The members that were in the match when it began (session traffic within PEER_SILENT_MS at the in-match status), a
// bit per peers[] slot: a member who left the room before the match never makes a later departure look partial.
static volatile int32_t match_members;

static void note_match_members(void) {
    uint64_t now = plat_now_ms();
    uint32_t m = 0;
    plat_mutex_lock(&lock);
    for (int i = 0; i < 16 && peers[i]; i++)
        if (now - peer_seen[i] < PEER_SILENT_MS) m |= 1u << i;
    plat_mutex_unlock(&lock);
    plat_xchg32(&match_members, (int32_t)m);
}

// ---- the files the game opens. The game streams its music from Sound/Resource/Stream. After the stage pack, the first
// versus music is the intro's (the first INTRO_STREAM file), opened while the black loading screen may still be
// up (measured: 2.4 s after the pack on one stage with the intro on screen 0.6 s later, 3.8 s after it on
// another with the loading screen still presented for seconds); the match's own music, BGM_Versus_<song>.bwav,
// comes with "Go!" about 15 s later. The intro counts as shown once its music is open and its frames are presented
// (frames_main); the match's music shows the game at the latest.
#define INTRO_STREAM "Sound/Resource/Stream/BGM_Versus_"
static volatile int32_t diag_files, in_room, stage_open;
static uint64_t stage_open_ms;

void automatch_file(const char *path) {
    if (!strncmp(path, "Pack/Scene/", 11)) {
        const char *pack = path + 11;
        char name[64];
        size_t k = strcspn(pack, ".");
        snprintf(name, sizeof name, "%.*s", (int)(k < 63 ? k : 63), pack);
        rt_log("automatch: +%.1f s scene pack %s", now_s(), name);
        if (!strcmp(name, "LobbyLocal")) BUMP(seq_lobby);
        else if (!strncmp(name, "Vss_", 4)) {
            plat_mutex_lock(&lock);
            snprintf(vss_name, sizeof vss_name, "%s", name);
            plat_mutex_unlock(&lock);
            if (LOAD(in_room) && !LOAD(stage_open)) stage_open_ms = plat_now_ms(), plat_xchg32(&stage_open, 1);
            BUMP(seq_vss);
        }
        return;
    }
    if (!LOAD(stage_open) || LOAD(seq_vss_go)) return;
    if (diag && plat_now_ms() - stage_open_ms < 30000 && plat_inc32(&diag_files) <= 600)
        rt_log("automatch: +%.2f s file %s", now_s(), path);
    if (!strncmp(path, INTRO_STREAM, sizeof INTRO_STREAM - 1)) {
        const char *b = strrchr(path, '/') + 1;
        rt_log("automatch: +%.2f s %s music %.*s", now_s(), LOAD(seq_intro) ? "the match's" : "the intro's",
               (int)strcspn(b, "."), b);
        if (LOAD(seq_intro)) BUMP(seq_vss_go);   // the next music: the match's own, with "Go!"
        else BUMP(seq_intro);
    }
}

// ---- presses and shots
#define PRESS_TICKS 6   // a press holds its button this many ticks of game time (input.c: frames at 60 a second)
static void press(const char *button) {
    char l[48];
    snprintf(l, sizeof l, "%s %d", button, PRESS_TICKS);
    input_inject(l);
    rt_log("automatch: +%.1f s press %s", now_s(), button);
}

// A press, then the next step once `ticks` ticks of game time (frame_step_ticks: the frames the game stepped, one per
// frame at 60 a second) have passed since it, and at least the hold and as long again: each press and the release
// between two reach the game's menu at any frame rate. A gap in wall time only merges two presses into one held button
// on a machine that shows the hold's frames in that time (12 frames a second for 500 ms), and on a slower one gives the
// menu fewer frames between presses than it was measured with. At most 10 s.
static void press_paced(const char *button, uint32_t ticks) {
    if (ticks < 2 * PRESS_TICKS) ticks = 2 * PRESS_TICKS;
    uint64_t t0 = frame_step_ticks(), end = plat_now_ms() + 10000;
    press(button);
    while (frame_step_ticks() < t0 + ticks + 1 && plat_now_ms() < end) plat_sleep_ms(5);
}

static void shot(const char *step) {
    if (!shots_dir[0]) return;
    char l[700];
    snprintf(l, sizeof l, "SHOT %s/auto-%s-%02d-%s.png", shots_dir, role == HOST ? "host" : "join", ++nshot, step);
    input_inject(l);
}

static void sleep_ms(uint32_t ms) { plat_sleep_ms(ms); }

static uint64_t until(int t) { return plat_now_ms() + (uint64_t)t_s[t] * 1000; }
static int before(uint64_t end) { return plat_now_ms() < end; }

static int wait_above(volatile int32_t *c, int32_t v, uint64_t end) {
    while (before(end)) {
        if (LOAD(*c) > v) return 1;
        sleep_ms(50);
    }
    return LOAD(*c) > v;
}

// ---- the session list's search: the game's browse request and the step the session library's browse job ends with (in
// main; the job stores it with its name string, "session::BrowseSessionJob::CompleteProcess"). A press on the list
// before its search ended is lost (measured: Y 2 s after the list opened did nothing). Hooked through the
// function-entry hook (FNPROF_AUTO) only from the lobby to the stage, so the rest of the game runs without it.
#define PIA_BROWSE_DONE (s3->pia_browse_done)   // BrowseSessionJob::CompleteProcess
#define NET_BROWSE (s3->net_browse)   // the game's network system, vtable +0x70: browse (-> the session library's BrowseSessionAsync)
static volatile int32_t n_browse_req, n_browse_done;

// ---- the room's settings. Measured with function-entry traces and memory reads of the host's room screen (docs):
// - the save data object (*(main+0x6097e78)) holds the private battle's settings the room screen shows: at +0xb8 a
//   pointer to {+0 stage: VersusSceneInfo Id, -1 random; +8 the stage list's count, +0x10 a pointer to its entries
//   {u32 stage id, u32 in the random draw}; +0x454 the four "other settings" bytes, in the screen's order}, and at
//   +0x219070 a pointer to {+4 rule: the game's VersusRule id, 0-5};
// - the room screen's commit, main+0x38f9930 (x0 the screen), runs every frame while the screen is up: it stores the
//   screen's rule (+0xcf0) into the save's, its Recon flag (+0xcef) into the room as the match type (3 Recon, 1 a
//   battle), and copies the save's stage (the scene name looked up by Id, or "random" with the draw's count) and other
//   settings into the room; the room's update (main+0x388fad0, main+0x3890300) then sends them to the members as
//   spl::VersusPrivateSettingNetState {+0x20 random draw count, +0x24 match type, +0x28 rule, +0x2c..+0x2f the other
//   settings} and spl::CommonPrivateSettingNetState {+0x28 -> the stage's scene name, "" for random; +0x7c random};
//   a guest applies them (main+0x3890d20) to its own room and save.
// The host writes the wanted values at the entry of the commit, every time it runs: the game itself then sets the room
// and sends the settings, as when the player picks them. Its own screen keeps showing its cursor's choices (the window
// is hidden). The members' copies (the net states' serialize and deserialize, below) are read back to check.
#define SAVE_HOLDER (s3->save_holder)   // main: the save data object's pointer
#define SAVE_PRIVATE 0xb8ull         // save data object: -> the private battle's settings
#define SAVE_RULE 0x219070ull        // save data object: -> {+4 rule}
#define ROOM_COMMIT (s3->room_commit)   // the room screen's commit (x0 the screen)
#define ROOM_SCREEN (s3->room_screen)   // the room screen's update, every frame it is up, host and guests (it calls the commit)
#define UI_RECON 0xcefull            // room screen: Recon on (u8)
#define UI_RULE 0xcf0ull             // room screen: rule (u32)
#define PRIV_STAGE 0x0ull
#define PRIV_POOL_N 0x8ull
#define PRIV_POOL 0x10ull
#define PRIV_OPTS 0x454ull
#define NET_VERSUS_OUT (s3->net_versus_out)  // VersusPrivateSettingNetState serialize (host) / deserialize (members), x0 the state
#define NET_VERSUS_IN (s3->net_versus_in)
#define NET_COMMON_OUT (s3->net_common_out)  // CommonPrivateSettingNetState serialize / deserialize
#define NET_COMMON_IN (s3->net_common_in)
#define NET_COMMON_IN_B (s3->net_common_in_b) // the same through its second base: x0 = the state + 0x10 (what a member runs)
// main: the VersusRule enum's text ("<name> , <name> , ..."): splat3_addr.c rule_names
static char rules_error[200];   // parse_rules: why the rules cannot work
static void parse_rules(const char *v);
static int stage_scene(int id, char *out, size_t cap);
static int results_hold = 10;   // s without the player's input before the result screens advance
static int spectators_wait = 60; // host, teams by name: s it waits for the spectators once every player is in
static uint64_t command_ms;      // when the last next command came (round 2 on)
static volatile int32_t rules_on, n_commit, stage_check;   // stage_check: 1 in the room's list, -1 not
static volatile int32_t n_room_frames;   // the room screen's updates (ROOM_SCREEN)
static volatile uint64_t room_model;     // the room the screen commits to: +0xdc0 the 10 places' teams (u32)
// The room screen (x0 of its commit) is a state machine (its update main+0x38f7ff0; on 11.3.0 the update main+0x38f8930
// and the change of state main+0x390067c): +0x14 the state, +0xc the one before, +0x18 the frames spent in the state
// (zeroed when the state changes, counted by each update). The host's states on the way to the match, measured on 11.3.0
// (docs/auto-match.md, "Slow machines"): 4 the room (mode, stage, "ready"), 5 the check of the player list that "ready"
// starts (a message, 115 to 125 of the screen's frames: 2.1 s at 60 frames a second, 3 to 4.5 s at 26 to 39), 7 the
// team screen, 8 once its "Teams OK" is taken (the room's team array is set in that frame). The teams asked
// for per row (0 Alpha, 1 Bravo, -1 as the game has it), written into the save's at the commit while teams_on: the team
// screen takes the save's teams when it opens.
#define UI_STATE 0x14ull
#define UI_FRAMES 0x18ull
#define UI_TEAM_SCREEN 7
#define PRIV_TEAMS 0x42cull
#define PLACE_SPECTATOR 3   // a place's value for a spectator (measured: Right on a row cycles 4 any, 0, 1, 3, 4)
static volatile uint64_t room_ui;
static volatile int32_t teams_on;
static int team_row[10] = {-1, -1, -1, -1, -1, -1, -1, -1, -1, -1};
static uint64_t save_part(uint64_t off);

// The teams of the room's 10 places and of the save's (what the team screen sets), for the log
static void teams_log(const char *when) {
    uint64_t rm = LOAD(room_model), ps = save_part(SAVE_PRIVATE);
    char a[80] = "", b[80] = "";
    for (int i = 0; i < 10; i++) {
        snprintf(a + strlen(a), sizeof a - strlen(a), " %d", rm && readable(rm + 0xdc0 + 4 * i, 4) ? (int)rd32(rm + 0xdc0 + 4 * i) : -9);
        snprintf(b + strlen(b), sizeof b - strlen(b), " %d", ps && readable(ps + 0x42c + 4 * i, 4) ? (int)rd32(ps + 0x42c + 4 * i) : -9);
    }
    rt_log("automatch: +%.1f s teams %s: room%s; save%s", now_s(), when, a, b);
}
static int stage_list(int32_t *ids, int cap);
static volatile uint64_t net_versus, net_common;   // the room's net states, as their (de)serialize saw them

static uint64_t save_part(uint64_t off) {
    uint64_t s = s3 ? rd64(main_base + SAVE_HOLDER) : 0;
    return s ? rd64(s + off) : 0;
}

// Host, at the room screen's commit (a guest thread, with the screen in x0)
static void room_commit(void) {
    Cpu *c = kern_cpu();
    if (role != HOST || !LOAD(rules_on) || !c) return;
    uint64_t ui = c->x[0], rs = save_part(SAVE_RULE), ps = save_part(SAVE_PRIVATE);
    if (!readable(ui + UI_RECON, 8) || !rs || !readable(rs, 8) || !ps || !readable(ps, PRIV_OPTS + OPTS)) return;
    if (want_stage >= 0 && LOAD(stage_check) <= 0) {   // only a stage of the room's own list (filled with the room)
        int32_t ids[64];
        int n = LOAD(stage_check) ? 0 : stage_list(ids, 64), in = 0;
        for (int i = 0; i < n; i++) in |= ids[i] == want_stage;
        if (n) plat_xchg32(&stage_check, in ? 1 : -1);
        if (!in) return;
    }
    *(volatile uint8_t *)(uintptr_t)(ui + UI_RECON) = 0;   // a battle, not Recon
    *(volatile uint32_t *)(uintptr_t)(ui + UI_RULE) = (uint32_t)want_rule;
    *(volatile uint32_t *)(uintptr_t)(rs + 4) = (uint32_t)want_rule;
    *(volatile int32_t *)(uintptr_t)(ps + PRIV_STAGE) = want_stage;
    for (int i = 0; i < OPTS; i++) *(volatile uint8_t *)(uintptr_t)(ps + PRIV_OPTS + i) = (uint8_t)want_opt[i];
    __atomic_store_n(&room_ui, ui, __ATOMIC_RELEASE);
    __atomic_store_n(&room_model, rd64(ui + 0xcd0), __ATOMIC_RELEASE);   // the room the screen commits to
    if (LOAD(teams_on))   // the teams, before the team screen opens: it shows and confirms the save's
        for (int r = 0; r < 10; r++)
            if (team_row[r] >= 0) *(volatile uint32_t *)(uintptr_t)(ps + PRIV_TEAMS + 4 * r) = (uint32_t)team_row[r];
    BUMP(n_commit);
}

// The host's room screen state, as its update leaves it each frame (ROOM_SCREEN, x0 the screen; UI_STATE and UI_FRAMES
// below): each change is logged with the frames the state before lasted, so a log shows which screen the presses met
static int32_t ui_seen = -1;
static uint32_t ui_seen_frames;
static void room_screen_seen(void) {
    Cpu *c = kern_cpu();
    if (role != HOST || !LOAD(rules_on) || !c || !readable(c->x[0] + UI_STATE, 8)) return;
    int32_t st = (int32_t)rd32(c->x[0] + UI_STATE);
    uint32_t fr = rd32(c->x[0] + UI_FRAMES);
    if (st != ui_seen && ui_seen >= 0)
        rt_log("automatch: +%.2f s room screen state %d -> %d after %u frames", now_s(), ui_seen, st, ui_seen_frames);
    ui_seen = st, ui_seen_frames = fr;
}

void automatch_fn_hit(uint64_t addr) {
    if (!s3) return;
    uint64_t o = addr - main_base;
    if (o == PIA_BROWSE_DONE) BUMP(n_browse_done);
    else if (o == NET_BROWSE) BUMP(n_browse_req);
    else if (o == ROOM_COMMIT) room_commit();
    else if (o == ROOM_SCREEN) BUMP(n_room_frames), room_screen_seen();
    else if (o == NET_VERSUS_OUT || o == NET_VERSUS_IN || o == NET_COMMON_OUT || o == NET_COMMON_IN ||
             o == NET_COMMON_IN_B) {
        Cpu *c = kern_cpu();
        int out = o == NET_VERSUS_OUT || o == NET_COMMON_OUT;   // the host sends its own, a member receives the host's
        if (c && out == (role == HOST)) __atomic_store_n(o == NET_VERSUS_OUT || o == NET_VERSUS_IN ? &net_versus : &net_common,
                                c->x[0] - (o == NET_COMMON_IN_B ? 0x10 : 0), __ATOMIC_RELEASE);
    }
}

// The room's settings as its net states hold them now: rule, match type, other settings, stage scene ("" random).
// 0 when the states were not seen yet.
typedef struct { int rule, type, random; int opt[OPTS]; char scene[64]; } RoomNow;

// A stage's scene name as the game's VersusSceneInfo rows name them: 1 to 63 letters, digits and '_'.
// A guest reads the room's scene name from what the host sent: anything else is not a stage's.
static int scene_name_ok(const char *s) {
    size_t n = strspn(s, "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_");
    return n && n < 64 && !s[n];
}

static int room_now(RoomNow *r) {
    uint64_t v = LOAD(net_versus), cm = LOAD(net_common);
    memset(r, 0, sizeof *r);
    if (!v || !readable(v + 0x20, 0x14)) return 0;   // a member: the stage state may be unseen (scene "")
    r->rule = (int)rd32(v + 0x28), r->type = (int)rd32(v + 0x24);
    for (int i = 0; i < OPTS; i++) r->opt[i] = *(volatile const uint8_t *)(uintptr_t)(v + 0x2c + i);
    if (!cm || !readable(cm + 0x28, 0x58)) return 1;
    r->random = *(volatile const uint8_t *)(uintptr_t)(cm + 0x7c) != 0;
    uint64_t sp = rd64(cm + 0x28);
    for (int k = 0; sp && k < 63 && readable(sp + k, 1); k++)
        if (!(r->scene[k] = *(volatile const char *)(uintptr_t)(sp + k))) break;
    if (r->scene[0] && !scene_name_ok(r->scene)) {   // kept as unseen: a guest then names the stage pack it opened
        static volatile int32_t told;
        if (!plat_xchg32(&told, 1)) rt_log("automatch: the room's stage name \"%s\" is not a scene name; ignored", JE(r->scene));
        r->scene[0] = 0;
    }
    return 1;
}

// The save's private-battle stage list (what the room's stage screen offers): ids into ids[], how many
static int stage_list(int32_t *ids, int cap) {
    uint64_t ps = save_part(SAVE_PRIVATE);
    if (!ps || !readable(ps, PRIV_POOL + 8)) return 0;
    uint32_t n = rd32(ps + PRIV_POOL_N);
    uint64_t arr = rd64(ps + PRIV_POOL);
    int k = 0;
    for (uint32_t i = 0; i < n && i < 64 && k < cap && arr; i++) {
        uint64_t e = rd64(arr + 8 * i);
        if (e && readable(e, 8)) ids[k++] = (int32_t)rd32(e);
    }
    return k;
}

static const char *rule_name(int rule) { return rule >= 0 && rule < nrule_names ? rule_names[rule] : "?"; }
static const char *mode_key(int rule) { return rule >= 0 && rule < MODES ? mode_keys[rule] : "other"; }

const char *automatch_mode_key(int rule) { return rule >= 0 && rule < MODES ? mode_keys[rule] : NULL; }
const char *automatch_option_key(int i) { return i >= 0 && i < OPTS ? opt_keys[i] : NULL; }

// The game's rule names from its VersusRule enum text in main (a comma-separated list), checked for shape: more names
// than the room's modes, each 1-7 letters or digits
int automatch_rule_names(uint64_t base, char (*names)[8], int cap) {
    char buf[80];
    int n = 0, k = 0;
    const Splat3Addr *sa = splat3_addr();
    if (!sa) return 0;
    const char *text = (const char *)(uintptr_t)(base + sa->rule_names);
    for (; n < 79 && mem_guest_readable(base + sa->rule_names + n, 1) && text[n]; n++) buf[n] = text[n];
    buf[n] = 0;
    for (char *p = buf; *p && k < cap;) {
        p += strspn(p, " ,");
        size_t len = strcspn(p, " ,");
        if (!len || len > 7) break;
        for (size_t i = 0; i < len; i++)
            if (!((p[i] >= 'A' && p[i] <= 'Z') || (p[i] >= 'a' && p[i] <= 'z') || (p[i] >= '0' && p[i] <= '9'))) return 0;
        snprintf(names[k++], 8, "%.*s", (int)len, p);
        p += len;
    }
    return k > MODES ? k : 0;
}

static void rule_names_read(void) { nrule_names = automatch_rule_names(main_base, rule_names, 8); }

// ---- the room's settings in the status: the stage list the room offers (both roles, from the lobby on) and the
// settings the room's net states hold (host: once applied; guest: when the stage loads)
static char stages_json[1600];
static RoomNow room_last;   // the room's settings when the stage started loading
static int room_known;

// The room's settings as the JSON member "room". r->scene came from the room's host (room_now): escaped like every
// string here (an unescaped stage name that closed the string would add members such as "state").
#define ROOM_JSON_CAP 1024
static void room_json(char *out, size_t cap, const RoomNow *r) {
    int k = 0;
    jcat(out, cap, &k, "\"room\": {\"mode\": \"%s\", \"rule\": %d, \"rule_name\": \"%s\", \"stage\": \"%s\"",
         JE(mode_key(r->rule)), r->rule, JE(rule_name(r->rule)), JE(r->random || !r->scene[0] ? "random" : r->scene));
    for (int i = 0; i < OPTS; i++) jcat(out, cap, &k, ", \"%s\": %d", JE(opt_keys[i]), r->opt[i]);
    jcat(out, cap, &k, "}");
    jdone(out, k, "the room's settings");
}

static void room_status(const RoomNow *r) {
    char room[ROOM_JSON_CAP] = "";
    if (r) room_json(room, sizeof room, r);
    plat_mutex_lock(&status_lock);
    snprintf(status_room, sizeof status_room, "%s%s%s", stages_json, stages_json[0] && room[0] ? ", " : "", room);
    plat_mutex_unlock(&status_lock);
}

// Host: the room's net states carry the wanted mode, stage and other settings (the commit hook ran and the game sent
// them). 1 when they do within ms.
// 1 when the room's settings r are the ones asked for (want_rule, want_stage / want_stage_scene, want_opt), a battle.
// Random: the room's random flag alone. Its scene is "" until a match drew a stage and keeps the drawn stage's scene
// after it (measured: after a random round on stage 16 the host's net state held random 1 and that stage's scene, and
// round 2 asked random stopped with rules-not-applied when "" was required).
static int room_matches(const RoomNow *r) {
    if (r->rule != want_rule || r->type != 1) return 0;
    if (want_stage < 0 ? !r->random : r->random || strcmp(r->scene, want_stage_scene)) return 0;
    for (int i = 0; i < OPTS; i++)
        if (r->opt[i] != want_opt[i]) return 0;
    return 1;
}

static int room_applied(uint32_t ms, RoomNow *r) {
    uint64_t end = plat_now_ms() + ms;
    do {
        if (LOAD(n_commit) && room_now(r) && room_matches(r)) return 1;
        sleep_ms(100);
    } while (before(end));
    rt_log("automatch: the room's net states now: rule %d, type %d, random %d, scene \"%s\", other settings %d %d %d %d "
           "(asked: rule %d, stage %s, %d %d %d %d)", r->rule, r->type, r->random, r->scene, r->opt[0], r->opt[1],
           r->opt[2], r->opt[3], want_rule, want_stage_scene, want_opt[0], want_opt[1], want_opt[2], want_opt[3]);
    return 0;
}

static void jobs_log(const char *when) {
    rt_log("automatch: +%.1f s %s: searches requested %d, ended %d; 35000 bound %d, closed %d; answers %d, members %d",
           now_s(), when, LOAD(n_browse_req), LOAD(n_browse_done), LOAD(n_bind), LOAD(n_close), LOAD(n_reply),
           LOAD(n_peers));
}

// The session list is open while the session library's network is up: 35000 bound more often than closed
static int list_up(void) { return LOAD(n_bind) > LOAD(n_close); }

// Waits for the search a browse request started to end, then for the list to be drawn
static void search_ended(int32_t d0) {
    if (wait_above(&n_browse_done, d0, plat_now_ms() + 20000)) {
        sleep_ms(1000);
        mark("search ended");
    } else {
        rt_log("automatch: the search did not end in 20 s");
    }
}

// The session list: L opens the lobby's menu, A picks the LAN private battle; the session library binds 35000 then and searches.
static int open_list(uint64_t end) {
    for (int attempt = 1; before(end); attempt++) {
        int32_t b0 = LOAD(n_bind), d0 = LOAD(n_browse_done);
        press("L");
        sleep_ms(1500);
        press("A");
        if (wait_above(&n_bind, b0, plat_now_ms() + 4000)) {
            mark("session list open");
            search_ended(d0);
            return 1;
        }
        rt_log("automatch: the session list did not open (attempt %d): back out and again", attempt);
        press("B");
        sleep_ms(1000);
        press("B");
        sleep_ms(2500);
    }
    return 0;
}

// The network restarts when a session is created or joined: 35000 closed, then bound again
static int restarted(int32_t c0, int32_t b0, uint32_t ms) {
    uint64_t end = plat_now_ms() + ms;
    while (before(end)) {
        if (LOAD(n_close) > c0 && LOAD(n_bind) > b0) return 1;
        sleep_ms(50);
    }
    return 0;
}

// After presses that did not create or join: back to a listed, searched session list
static int back_to_list(int32_t q0, int32_t d0, uint64_t end) {
    if (LOAD(n_browse_req) > q0) {   // an A on the list started a new search
        search_ended(d0);
    } else if (list_up()) {          // a dialog may be open: B closes it (B on the list would close the list)
        press("B");
        sleep_ms(2000);
    }
    if (!list_up()) return open_list(end);
    return 1;
}

static int host_create(uint64_t end) {
    for (int attempt = 1; before(end); attempt++) {
        int32_t c0 = LOAD(n_close), b0 = LOAD(n_bind), q0 = LOAD(n_browse_req), d0 = LOAD(n_browse_done);
        press("Y");   // "Create a session"
        sleep_ms(1500);
        press("A");   // confirm
        if (restarted(c0, b0, 8000)) {
            mark("session created");
            jobs_log("created");
            return 1;
        }
        rt_log("automatch: no session after Y, A (attempt %d)", attempt);
        jobs_log("create failed");
        if (!back_to_list(q0, d0, end)) return 0;
    }
    return 0;
}

// The team of the team screen's row (the players in the order they came in: the host first): from the arrival-order
// list (teams=AB), or by name (teams=Alpha:A,Bravo:B) with the host's own name and the relay's names of the members
// The team of a team screen row (the players in the order they came in, the host first) whose player is `name` (""
// when unknown): by name when teams by name were given, else the arrival-order string; '-' keeps the game's "any team"
static char team_for(int row, const char *name) {
    if (!nby_name) return row < (int)strlen(teams) ? teams[row] : '-';
    for (int i = 0; i < nby_name && name[0]; i++)
        if (!strcmp(by_name[i].name, name)) return by_name[i].team;
    return '-';
}

// This player's seat as the rules give it: the host's own row (row 0, its own name); a guest by its name in teams by
// name (the arrival-order form names no guest's row: unknown). A row left to the game ('-') is a player's.
static int rules_seat(void) {
    if (role == HOST) return (teams[0] || nby_name ? team_for(0, self_name) : '-') == 'S' ? SEAT_SPECTATOR : SEAT_PLAYER;
    for (int i = 0; i < nby_name && self_name[0]; i++)
        if (!strcmp(by_name[i].name, self_name)) return by_name[i].team == 'S' ? SEAT_SPECTATOR : SEAT_PLAYER;
    return SEAT_UNKNOWN;
}

// Teams by name: the spectators the rules list
static int spectators_asked(void) {
    int sp = 0;
    for (int i = 0; i < nby_name; i++) sp += by_name[i].team == 'S';
    return sp;
}

// The match's players by the rules: players= less the spectators (S) its teams name, by name or in arrival order
static int rules_players(void) {
    int sp = spectators_asked();
    if (!nby_name)
        for (int i = 0; teams[i] && i < players; i++) sp += teams[i] == 'S';
    return players - sp;
}

// The session's silence that ends a match (connection-lost): SPLAT3_MATCH_SILENT, else 15 s for two players (the
// spectators not counted), 45 s for more
static uint32_t silent_set_ms;   // SPLAT3_MATCH_SILENT in ms, 0 when unset
static uint32_t match_silent_ms(void) {
    return silent_set_ms ? silent_set_ms : rules_players() == 2 ? MATCH_SILENT_DUEL_MS : MATCH_SILENT_MS;
}

// A member the rules name as a spectator (teams by name, relay mode: the relay names the members)
static int member_is_spectator(uint32_t ip) {
    char name[33] = "";
    if (!nby_name || !lan_relay_member_name(ip, name, sizeof name) || !name[0]) return 0;
    for (int j = 0; j < nby_name; j++)
        if (!strcmp(by_name[j].name, name)) return by_name[j].team == 'S';
    return 0;
}

// Teams by name, host: the members in the room now (session traffic within PEER_SILENT_MS), the host included, that the
// rules name as players (returned) and as spectators (*sp). A member whose name the relay has not given, or whom the
// rules do not list, counts as neither: the host then waits for everyone, as without spectators.
static int named_present(int *sp) {
    uint64_t now = plat_now_ms();
    uint32_t ips[16];
    int k = 0, pl = 0, s = 0;
    plat_mutex_lock(&lock);
    for (int i = 0; i < 16 && peers[i]; i++)
        if (now - peer_seen[i] < PEER_SILENT_MS) ips[k++] = peers[i];
    plat_mutex_unlock(&lock);
    for (int i = -1; i < k; i++) {
        char name[33] = "";
        if (i < 0) snprintf(name, sizeof name, "%s", self_name);
        else lan_relay_member_name(ips[i], name, sizeof name);
        for (int j = 0; j < nby_name && name[0]; j++)
            if (!strcmp(by_name[j].name, name)) {
                if (by_name[j].team == 'S') s++;
                else pl++;
                break;
            }
    }
    *sp = s;
    return pl;
}

// The row's player name: the host's own for row 0, else the relay's name of the member whose session traffic came
// (row)-th
static char row_team(int row) {
    char name[33] = "";
    if (row == 0) {
        snprintf(name, sizeof name, "%s", self_name);
    } else {
        plat_mutex_lock(&lock);
        uint32_t ip = row - 1 < 16 ? peers[row - 1] : 0;
        plat_mutex_unlock(&lock);
        lan_relay_member_name(ip, name, sizeof name);
    }
    char t = team_for(row, name);
    rt_log("automatch: row %d (%s): team %c", row, name[0] ? name : "name unknown", t);
    return t;
}

// The teams asked for, checked: at most 4 in Alpha and 4 in Bravo (a team of the room has 4 places), at most 2
// spectators (S), and the game's own condition for "Teams OK": two players at least and each team with a player or a
// row left to the game (measured on the team screen with 2 in the room: a spectator and a player in Alpha, a spectator
// and "any team", or both in Alpha leave it disabled; Alpha and "any team" enable it); 0 and why
static int teams_check(char *why, size_t cap) {
    int a = 0, b = 0, sp = 0, set = 0;
    if (nby_name)
        for (int i = 0; i < nby_name; i++) a += by_name[i].team == 'A', b += by_name[i].team == 'B', sp += by_name[i].team == 'S';
    else
        for (int i = 0; teams[i] && i < players; i++) a += teams[i] == 'A', b += teams[i] == 'B', sp += teams[i] == 'S';
    for (int i = 0; i < nby_name; i++)
        if (!strchr("ABS-", by_name[i].team)) return snprintf(why, cap, "team %c of %s is not A, B, S or -", by_name[i].team, by_name[i].name), 0;
    for (const char *p = teams; *p; p++)
        if (!strchr("ABS-", *p)) return snprintf(why, cap, "team %c is not A, B, S or -", *p), 0;
    if (a > 4 || b > 4) return snprintf(why, cap, "%d in Alpha and %d in Bravo: a team has 4 places", a, b), 0;
    if (sp > 2) return snprintf(why, cap, "%d spectators: the room has 2 places for them", sp), 0;
    set = players - sp;              // the players, spectators left out
    if (set > 8)
        return snprintf(why, cap, "players=%d with %d spectators: a room has 8 places for players and 2 for spectators",
                        players, sp), 0;
    int any = set - a - b;           // their rows left to the game
    if (set < 2 || a + any < 1 || b + any < 1)
        return snprintf(why, cap, "%d in Alpha, %d in Bravo, %d spectators of %d in the room: the game confirms the teams "
                        "only with two players at least and a player for each team", a, b, sp, players), 0;
    return 1;
}

// ---- the probe's JSON reader: 1 when text[0..n) is one JSON value (RFC 8259) whose strings hold valid UTF-8 (its own
// check, apart from json_esc.h: the probe judges the writer with code the writer does not share); the top-level
// object's keys, unescaped and cut at 23 bytes, into keys[0..*nkeys)
typedef struct { const unsigned char *p, *e; char (*keys)[24]; int nkeys, cap, depth; } JsonRd;

static void jr_ws(JsonRd *j) {
    while (j->p < j->e && (*j->p == ' ' || *j->p == '\t' || *j->p == '\r' || *j->p == '\n')) j->p++;
}

static int jr_utf8(JsonRd *j) {   // one well-formed UTF-8 sequence of 2 to 4 bytes (Unicode's table 3-7)
    unsigned c = j->p[0];
    int k = c >= 0xc2 && c <= 0xdf ? 2 : c >= 0xe0 && c <= 0xef ? 3 : c >= 0xf0 && c <= 0xf4 ? 4 : 0;
    if (!k || j->e - j->p < k) return 0;
    unsigned b = j->p[1];
    if ((c == 0xe0 && b < 0xa0) || (c == 0xed && b > 0x9f) || (c == 0xf0 && b < 0x90) || (c == 0xf4 && b > 0x8f)) return 0;
    for (int i = 1; i < k; i++)
        if ((j->p[i] & 0xc0) != 0x80) return 0;
    j->p += k;
    return 1;
}

static int jr_str(JsonRd *j, char *out, size_t cap) {
    size_t o = 0;
    if (j->p >= j->e || *j->p != '"') return 0;
    for (j->p++; j->p < j->e && *j->p != '"';) {
        unsigned c = *j->p;
        if (c < 0x20) return 0;
        if (c == '\\') {
            if (j->e - j->p < 2) return 0;
            unsigned x = j->p[1];
            if (x == 'u') {
                if (j->e - j->p < 6) return 0;
                for (int i = 2; i < 6; i++)
                    if (!strchr("0123456789abcdefABCDEF", j->p[i]) || !j->p[i]) return 0;
                j->p += 6, c = '?';
            } else {
                if (!x || !strchr("\"\\/bfnrt", (int)x)) return 0;
                j->p += 2, c = x;
            }
        } else if (c >= 0x80) {
            if (!jr_utf8(j)) return 0;
            c = '?';
        } else {
            j->p++;
        }
        if (out && o + 1 < cap) out[o++] = (char)c;
    }
    if (j->p >= j->e) return 0;
    j->p++;
    if (out) out[o] = 0;
    return 1;
}

static int jr_word(JsonRd *j, const char *w) {
    size_t n = strlen(w);
    if ((size_t)(j->e - j->p) < n || memcmp(j->p, w, n)) return 0;
    j->p += n;
    return 1;
}

static int jr_digits(JsonRd *j) {
    const unsigned char *s = j->p;
    while (j->p < j->e && *j->p >= '0' && *j->p <= '9') j->p++;
    return j->p > s;
}

static int jr_value(JsonRd *j) {
    jr_ws(j);
    if (j->p >= j->e) return 0;
    unsigned c = *j->p;
    if (c == '"') return jr_str(j, NULL, 0);
    if (c == 't') return jr_word(j, "true");
    if (c == 'f') return jr_word(j, "false");
    if (c == 'n') return jr_word(j, "null");
    if (c == '{' || c == '[') {
        int top = ++j->depth == 1 && c == '{';
        unsigned close = c == '{' ? '}' : ']';
        if (j->depth > 32) return 0;
        j->p++;
        jr_ws(j);
        if (j->p < j->e && *j->p == close) return j->p++, j->depth--, 1;
        for (;;) {
            if (c == '{') {
                char key[24];
                jr_ws(j);
                if (!jr_str(j, key, sizeof key)) return 0;
                if (top && j->nkeys < j->cap) snprintf(j->keys[j->nkeys++], sizeof key, "%s", key);
                jr_ws(j);
                if (j->p >= j->e || *j->p++ != ':') return 0;
            }
            if (!jr_value(j)) return 0;
            jr_ws(j);
            if (j->p >= j->e) return 0;
            unsigned d = *j->p++;
            if (d == ',') continue;
            if (d != close) return 0;
            j->depth--;
            return 1;
        }
    }
    // a number: -?(0|[1-9][0-9]*)(.[0-9]+)?([eE][+-]?[0-9]+)?
    if (c == '-') j->p++;
    if (j->p < j->e && *j->p == '0') j->p++;
    else if (j->p >= j->e || *j->p < '1' || *j->p > '9' || !jr_digits(j)) return 0;
    if (j->p < j->e && *j->p == '.' && (j->p++, !jr_digits(j))) return 0;
    if (j->p < j->e && (*j->p == 'e' || *j->p == 'E')) {
        j->p++;
        if (j->p < j->e && (*j->p == '+' || *j->p == '-')) j->p++;
        if (!jr_digits(j)) return 0;
    }
    return 1;
}

// The JSON file at path, read whole into text (cut at cap - 1): 1 when it is one JSON object whose top-level keys are
// all different, with *count the times key is one of them
static int json_file_ok(const char *path, const char *key, int *count, char *text, size_t cap) {
    uint64_t n = 0;
    uint8_t *b = plat_file_read(path, &n);
    char keys[64][24];
    JsonRd j = {b, b + n, keys, 0, 64, 0};
    int ok = b && n && b[0] == '{' && jr_value(&j);
    jr_ws(&j);
    ok = ok && j.p == j.e;
    *count = 0;
    for (int i = 0; i < j.nkeys; i++) {
        *count += !strcmp(keys[i], key);
        for (int k = 0; k < i; k++) ok = ok && strcmp(keys[i], keys[k]);
    }
    snprintf(text, cap, "%.*s", b ? (int)(n < cap ? n : cap - 1) : 0, b ? (const char *)b : "");
    free(b);
    return ok;
}

// The error a relay's refusal (lan_relay_refusal) stops the match with (docs/auto-match.md, "Status file"): the precise
// code when the relay said why, else network-unavailable (unreachable, or an older relay's ROOM_FULL that may be either).
// *why: the end of the English sentence, NULL for network-unavailable.
static const char *refusal_code(int reason, int detail, const char **why) {
    *why = NULL;
    if (detail == LR_NAK_DETAIL_SOURCE && reason == LR_NAK_ROOM_FULL)
        return *why = "too many members of this match come from this player's Internet address", "relay-source-limit";
    if (detail == LR_NAK_DETAIL_SOURCE && reason == LR_NAK_RELAY_FULL)
        return *why = "too many players on the relay come from this player's Internet address", "relay-source-full";
    if (reason == LR_NAK_RELAY_FULL) return *why = "the relay is full", "relay-full";
    if (reason == LR_NAK_ROOM_FULL) return *why = "the match's room on the relay is full", "relay-room-full";
    return "network-unavailable";
}

// --probe-automatch: the room logic without the game. The rules parser and the team rows against two recorded rooms
// (the proof runs of docs/auto-match.md: M42 in lan mode, teams=AB; R3 in relay mode, teams by name, the relay's
// names) and synthetic rooms of 4 and 8 players in the order a host sees them come in; the status and result files
// with hostile text in them; the host name filter.
static int place_ok(int asked, int room);
static int seat_of_index(uint32_t me, uint32_t np);
static void write_result(uint64_t s, const char *outcome, const char *reason);
int automatch_selftest(void) {
    int fails = 0;
#define T(c, what) do { if (!(c)) { fails++; printf("FAIL automatch: %s\n", what); } } while (0)
    rules_error[0] = 0, teams[0] = 0, nby_name = 0;
    {   // the relay's refusals (JOIN_NAK reason and detail) and the errors they stop the match with
        const char *why;
        T(!strcmp(refusal_code(LR_NAK_RELAY_FULL, 0, &why), "relay-full") && why, "RELAY_FULL: relay-full");
        T(!strcmp(refusal_code(LR_NAK_ROOM_FULL, 0, &why), "relay-room-full") && why, "ROOM_FULL: relay-room-full");
        T(!strcmp(refusal_code(LR_NAK_ROOM_FULL, LR_NAK_DETAIL_SOURCE, &why), "relay-source-limit"),
          "ROOM_FULL for this address: relay-source-limit");
        T(!strcmp(refusal_code(LR_NAK_RELAY_FULL, LR_NAK_DETAIL_SOURCE, &why), "relay-source-full"),
          "RELAY_FULL for this address: relay-source-full");
        T(!strcmp(refusal_code(0, 0, &why), "network-unavailable") && !why, "no refusal: network-unavailable");
        T(!strcmp(refusal_code(LR_NAK_ROOM, 0, &why), "network-unavailable"), "another refusal: network-unavailable");
    }
    parse_rules("players=2,teams=AB");   // M42
    T(team_for(0, "Alpha") == 'A' && team_for(1, "") == 'B' && team_for(2, "") == '-', "M42: arrival order AB");
    parse_rules("players=2,teams=Alpha:B,Bravo:A,mode=turf");   // R3: relay names Alpha (host) and Bravo
    T(team_for(0, "Alpha") == 'B' && team_for(1, "Bravo") == 'A', "R3: by name");
    T(team_for(1, "") == '-' && team_for(1, "Charlie") == '-', "an unknown name keeps any team");
    parse_rules("players=8,teams=P1:A,P2:B,P3:A,P4:B,P5:A,P6:B,P7:A,P8:B,mode=clams,stage=random");
    const char *arrival[8] = {"P1", "P5", "P2", "P8", "P3", "P4", "P7", "P6"};   // the order they came in
    char got[9] = "";
    for (int r = 0; r < 8; r++) got[r] = team_for(r, arrival[r]);
    T(!strcmp(got, "AABBABAB") && players == 8 && want_rule == 4 && want_stage == -1, "8 players by name, any order");
    char why[120];
    T(teams_check(why, sizeof why), "4 v 4 is allowed");
    parse_rules("players=4,teams=Host:A,G1:A,G2:A,G3:B");   // 3 v 1
    T(nby_name == 4 && team_for(3, "G3") == 'B' && teams_check(why, sizeof why), "teams of different sizes");
    parse_rules("teams=P1:A,P2:A,P3:A,P4:A,P5:A");
    T(!teams_check(why, sizeof why), "5 in Alpha is refused");
    parse_rules("players=4,teams=Host:S,G1:A,G2:B,G3:A");   // a spectator: the team screen's place value 3
    T(team_for(0, "Host") == 'S' && teams_check(why, sizeof why), "a spectator and players in both teams");
    parse_rules("players=2,teams=SA");
    T(!teams_check(why, sizeof why), "a spectator and one player is refused (each team needs a player)");
    // The silence that ends a match (connection-lost): 15 s for two players, the spectators not counted; 45 s for more
    silent_set_ms = 0;
    parse_rules("players=2,teams=AB");
    T(rules_players() == 2 && match_silent_ms() == MATCH_SILENT_DUEL_MS, "1v1: 15 s of silence");
    parse_rules("players=3,teams=SAB");
    T(rules_players() == 2 && match_silent_ms() == MATCH_SILENT_DUEL_MS, "1v1 and a spectator in arrival order: 15 s");
    parse_rules("players=4,teams=Host:A,G1:B,S1:S,S2:S");
    T(rules_players() == 2 && match_silent_ms() == MATCH_SILENT_DUEL_MS, "1v1 and two spectators by name: 15 s");
    parse_rules("players=4,teams=Host:A,G1:B,G2:A,G3:B");
    T(rules_players() == 4 && match_silent_ms() == MATCH_SILENT_MS, "2v2: 45 s");
    parse_rules("players=2");
    T(match_silent_ms() == MATCH_SILENT_DUEL_MS, "no teams, two players: 15 s");
    silent_set_ms = 30000;
    T(match_silent_ms() == 30000, "SPLAT3_MATCH_SILENT wins");
    silent_set_ms = 0;
    parse_rules("players=3,teams=SAB");
    T(teams_check(why, sizeof why), "a spectator and one player per team");
    parse_rules("players=5,teams=SSSAB");
    T(!teams_check(why, sizeof why), "three spectators are refused");
    parse_rules("players=10,teams=P1:A,P2:B,P3:A,P4:B,P5:A,P6:B,P7:A,P8:B,S1:S,S2:S");
    T(players == 10 && teams_check(why, sizeof why), "8 players and 2 spectators: players counts everyone in the room");
    parse_rules("players=10,teams=P1:A,P2:B,P3:A,P4:B,P5:A,P6:B,P7:A,P8:B,P9:-,S1:S");
    T(!teams_check(why, sizeof why), "9 players besides a spectator are refused");
    parse_rules("players=9,teams=AABBAABB-");
    T(!teams_check(why, sizeof why), "9 players without spectators are refused");
    // the seat (status and result "seat"): the host's own row, a guest by its name in teams by name (match SP2's rules)
    parse_rules("players=3,teams=Alpha:A,Bravo:B,Charlie:S");
    int role0 = role;
    char self0[64];
    snprintf(self0, sizeof self0, "%s", self_name);
    role = HOST, snprintf(self_name, sizeof self_name, "Alpha");
    T(teams_check(why, sizeof why) && rules_seat() == SEAT_PLAYER, "SP2: the host plays");
    role = JOIN, snprintf(self_name, sizeof self_name, "Charlie");
    T(rules_seat() == SEAT_SPECTATOR, "SP2: Charlie's game knows it is a spectator from the rules");
    snprintf(self_name, sizeof self_name, "Bravo");
    T(rules_seat() == SEAT_PLAYER, "SP2: Bravo plays");
    snprintf(self_name, sizeof self_name, "Delta");
    T(rules_seat() == SEAT_UNKNOWN, "a guest the rules do not name: unknown");
    parse_rules("players=3,teams=SAB");
    role = HOST;
    T(rules_seat() == SEAT_SPECTATOR, "a host in the stands (arrival order: its row is the first)");
    role = JOIN;
    T(rules_seat() == SEAT_UNKNOWN, "a guest in arrival order: unknown");
    role = role0, snprintf(self_name, sizeof self_name, "%s", self0);
    T(seat_of_index(0, 2) == SEAT_PLAYER && seat_of_index(1, 2) == SEAT_PLAYER && seat_of_index(0xffffffffu, 2) == SEAT_SPECTATOR,
      "SP2's records: MyPersonalIndex 0 and 1 on the players' games, -1 on the spectator's");
    // the room's places after "Teams OK" (SP1: room 0 1 -1 for save places 0 1 3)
    T(place_ok(0, 0) && place_ok(1, 1) && place_ok(PLACE_SPECTATOR, -1), "SP1's room: Alpha, Bravo, a spectator");
    T(!place_ok(PLACE_SPECTATOR, 0) && !place_ok(PLACE_SPECTATOR, 1) && !place_ok(PLACE_SPECTATOR, -9) && !place_ok(1, -1),
      "a spectator put in a team, an unreadable room, a player without a team");
    parse_rules("players=2,teams=A-");
    T(teams_check(why, sizeof why), "a row left to the game can fill the other team");
    parse_rules("players=2,teams=S-");
    T(!teams_check(why, sizeof why), "a spectator and one row left to the game is refused (measured: not confirmed)");
    parse_rules("players=2,teams=AA");
    T(!teams_check(why, sizeof why), "both players in one team is refused");
    parse_rules("teams=AAB-");
    T(!nby_name && team_for(3, "x") == '-' && team_for(2, "x") == 'B', "a new teams list replaces the names");
    rules_error[0] = 0;
    parse_rules("mode=rainmaker,stage=12,main_abilities_only=1,results_hold=7");
    T(!rules_error[0] && want_rule == 3 && want_stage == 12 && want_opt[0] == 1 && results_hold == 7,
      "mode, stage id, other settings, results_hold");
    parse_rules("figure_gear=1,amiibo_names=1,reset_settings=1");   // an earlier build's key is still taken
    T(!rules_error[0] && want_opt[1] == 1 && want_opt[2] == 1 && want_opt[3] == 1 && !strcmp(opt_keys[1], "figure_gear"),
      "the other settings' neutral keys, and an earlier key as input");
    // the room check: random after a random round (the net state keeps the drawn scene), a fixed stage, other settings
    parse_rules("mode=turf,stage=random,main_abilities_only=0,figure_gear=0,figure_names=0,reset_settings=0");
    RoomNow rn = {.rule = 0, .type = 1, .random = 1};
    T(room_matches(&rn), "random, before any draw");
    snprintf(rn.scene, sizeof rn.scene, "Vss_Drawn");
    T(room_matches(&rn), "random after a random round: the drawn stage's scene stays in the room");
    rn.random = 0;
    T(!room_matches(&rn), "a fixed stage is not random");
    want_stage = 7, snprintf(want_stage_scene, sizeof want_stage_scene, "Vss_Drawn");
    T(room_matches(&rn), "the fixed stage asked for");
    rn.random = 1;
    T(!room_matches(&rn), "random is not the fixed stage asked for");
    rn.random = 0, rn.type = 3;
    T(!room_matches(&rn), "Recon is not a battle");
    rn.type = 1, rn.opt[2] = 1;
    T(!room_matches(&rn), "another other setting");
    want_stage = -1, snprintf(want_stage_scene, sizeof want_stage_scene, "random");
    strcpy(rule_names[1], "Rul1"), nrule_names = 6;   // as the game's names read from main in a game run
    parse_rules("mode=Rul1");
    T(!rules_error[0] && want_rule == 1, "a mode by the game's own rule name");
    nrule_names = 0;
    rules_error[0] = 0;
    parse_rules("mode=tricolor");
    T(rules_error[0] != 0, "an unknown mode is an error");
    rules_error[0] = 0;
    parse_rules("results_hold=-1");
    T(rules_error[0] != 0, "a bad results_hold is an error");
    rules_error[0] = 0;
    parse_rules("players=4,teams=Alpha:A,Bravo:B,Charlie:S,Delta:S,spectators_wait=45");
    T(!rules_error[0] && spectators_wait == 45 && spectators_asked() == 2 && teams_check(why, sizeof why),
      "two spectators by name, spectators_wait");
    parse_rules("spectators_wait=601");
    T(rules_error[0] != 0, "a bad spectators_wait is an error");
    rules_error[0] = 0, spectators_wait = 60;
    // A name without its team letter ("Bravo:") is no team by name. The check looked at the byte after the item's NUL
    // (one byte past the item's buffer for an item of 47 bytes); here that byte is the previous item's NUL, so the
    // old check took "Bravo" with the team letter NUL, which teams_check let through.
    nby_name = 0, teams[0] = 0;
    parse_rules("players=00002,teams=Bravo:");
    T(nby_name == 0, "a team-by-name item without its team letter is not a team by name");
    parse_rules("players=2,teams=NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN:");   // 47 bytes: the item's buffer full
    T(nby_name == 0, "a 47-byte team-by-name item without its team letter");
    rules_error[0] = 0, nby_name = 0, teams[0] = 0, players = 2;

    // JSON string contents (json_esc): quotes, backslashes and control characters escaped, valid UTF-8 kept, every
    // byte of a malformed sequence U+FFFD (as \u escape), a cut only between whole characters and escapes
    {
        static const struct { const char *in, *out; } esc[] = {
            {"a\"b\\c\nd}\x7f", "a\\\"b\\\\c\\u000ad}\\u007f"},
            {"\xff", "\\ufffd"},                                   // never a UTF-8 byte
            {"\xc0\xaf", "\\ufffd\\ufffd"},                        // an overlong '/'
            {"\xed\xa0\x80", "\\ufffd\\ufffd\\ufffd"},             // a surrogate
            {"\xf4\x90\x80\x80", "\\ufffd\\ufffd\\ufffd\\ufffd"},  // past U+10FFFF
            {"\xe2\x82\"", "\\ufffd\\ufffd\\\""},                  // cut short by a quote, which stays escaped
            {"\x80x", "\\ufffdx"},                                 // a stray continuation byte
            {"\xc3\xa9 \xf0\x9d\x84\x9e", "\xc3\xa9 \xf0\x9d\x84\x9e"},   // e acute and U+1D11E, kept
        };
        for (size_t i = 0; i < sizeof esc / sizeof *esc; i++) {
            char o[96], what[160];
            json_esc(o, sizeof o, esc[i].in);
            snprintf(what, sizeof what, "json_esc case %zu: got \"%s\"", i, o);
            T(!strcmp(o, esc[i].out), what);
        }
    }

    // The stage name a room's host sends (the scene name of its
    // CommonPrivateSettingNetState, which a guest's room_now copies, 63 bytes at most) reaches the guest's status and
    // result files. A name that closes the JSON string must stay inside it: no member of its own ("state", "error"),
    // no broken file. The same with a stage pack name, a reason and a timeline entry holding a backslash, a newline and
    // bytes that are no UTF-8.
    {
        static const char forged[] = "X\"}, \"state\": \"error\", \"error\": \"host-left\", \"z\": {\"a\": \"";
        static const char nasty[] = "Vss_\\\"\n}\xff\xc0\xaf\"error\": 1, \"state\": \"x";
        T(strlen(forged) <= 63 && strlen(nasty) <= 63, "the hostile names fit what room_now copies");
        char dir[PLAT_PATH_MAX], sp[512], rp[512];
        if (!plat_temp_dir(dir, sizeof dir)) snprintf(dir, sizeof dir, ".");
        snprintf(sp, sizeof sp, "%s/klm-probe-automatch-%u-status.json", dir, plat_process_id());
        snprintf(rp, sizeof rp, "%s/klm-probe-automatch-%u-result.json", dir, plat_process_id());
        char sp0[512], rp0[512], vss0[64], stages0[sizeof stages_json];
        snprintf(sp0, sizeof sp0, "%s", status_path), snprintf(rp0, sizeof rp0, "%s", result_path);
        snprintf(vss0, sizeof vss0, "%s", vss_name), snprintf(stages0, sizeof stages0, "%s", stages_json);
        RoomNow last0 = room_last;
        int known0 = room_known, role1 = role, nt0 = ntimeline;
        snprintf(status_path, sizeof status_path, "%s", sp), snprintf(result_path, sizeof result_path, "%s", rp);
        role = JOIN, stages_json[0] = 0;
        for (int pass = 0; pass < 2; pass++) {
            const char *scene = pass ? nasty : forged;
            char text[4096], what[200];
            int n;
            memset(&room_last, 0, sizeof room_last);
            room_last.type = 1;
            snprintf(room_last.scene, sizeof room_last.scene, "%s", scene);   // as room_now copies it
            room_known = 1;
            room_status(&room_last);
            status_write("loading-stage", NULL, 0, 0, "loading the stage");
            int ok = json_file_ok(sp, "state", &n, text, sizeof text);
            snprintf(what, sizeof what, "%s stage name: the status file is one JSON object, \"state\" once, no \"error\" "
                     "(%s)", pass ? "a malformed" : "a forged", text);
            int errors = 0;
            json_file_ok(sp, "error", &errors, text, sizeof text);
            T(ok && n == 1 && !errors, what);
            if (!pass)
                T(strstr(text, "\"stage\": \"X\\\"}, \\\"state\\\": \\\"error\\\", \\\"error\\\": \\\"host-left\\\", "
                               "\\\"z\\\": {\\\"a\\\": \\\"\"") != NULL,
                  "the forged stage name is the room's \"stage\" string, escaped");
            snprintf(vss_name, sizeof vss_name, "%s", scene);
            ntimeline = 0;
            mark(scene);
            write_result(0, "aborted", scene);
            ok = json_file_ok(rp, "outcome", &n, text, sizeof text);
            json_file_ok(rp, "error", &errors, text, sizeof text);
            int states = 0, zs = 0;
            json_file_ok(rp, "state", &states, text, sizeof text);
            json_file_ok(rp, "z", &zs, text, sizeof text);
            snprintf(what, sizeof what, "%s stage name: the result file is one JSON object, \"outcome\" once, no "
                     "\"state\", \"error\" or \"z\" (%s)", pass ? "a malformed" : "a forged", text);
            T(ok && n == 1 && !errors && !states && !zs, what);
        }
        plat_file_delete(sp), plat_file_delete(rp);
        snprintf(status_path, sizeof status_path, "%s", sp0), snprintf(result_path, sizeof result_path, "%s", rp0);
        snprintf(vss_name, sizeof vss_name, "%s", vss0), snprintf(stages_json, sizeof stages_json, "%s", stages0);
        room_last = last0, room_known = known0, role = role1, ntimeline = nt0;
        status_room[0] = 0;
    }

    // The host name filter (SPLAT3_MATCH_HOST) takes a host's answer only when it carries the name whole. An
    // answer holds the host's player name twice (measured: docs/auto-match.md's runs on 11.2.0): in UTF-8 after its
    // byte count (32-bit big endian) and a byte 01, then the next field's bytes; in UTF-16LE between NUL units.
    {
        uint8_t ans[192];
        char host0[64];
        snprintf(host0, sizeof host0, "%s", host_name);
        memset(ans, 0, sizeof ans);
        ans[0] = 1, ans[3] = 2;   // an answer: its first byte is not 0
        static const uint8_t u8[] = {0, 0, 0, 5, 1, 'A', 'l', 'p', 'h', 'a', 'm', ']', 'e', 'v'};
        static const uint8_t u16[] = {0, 0, 'A', 0, 'l', 0, 'p', 0, 'h', 0, 'a', 0, 0, 0};
        memcpy(ans + 0x24, u8, sizeof u8);
        memcpy(ans + 0x8b, u16, sizeof u16);
        static const struct { const char *host; int found; } hn[] = {
            {"Alpha", 1}, {"Alph", 0}, {"lpha", 0}, {"Alpham", 0}, {"Bravo", 0}, {"pha", 0},
        };
        for (size_t i = 0; i < sizeof hn / sizeof *hn; i++) {
            char what[96];
            snprintf(host_name, sizeof host_name, "%s", hn[i].host);
            snprintf(what, sizeof what, "the host name filter: \"%s\" in an answer from Alpha", hn[i].host);
            T(has_name(ans, sizeof ans) == hn[i].found, what);
        }
        // an answer from Alpha2 is not one from Alpha
        static const uint8_t u8b[] = {0, 0, 0, 6, 1, 'A', 'l', 'p', 'h', 'a', '2', 0x13, 0x37};
        static const uint8_t u16b[] = {0, 0, 'A', 0, 'l', 0, 'p', 0, 'h', 0, 'a', 0, '2', 0, 0, 0};
        memset(ans + 0x20, 0, sizeof ans - 0x20);
        memcpy(ans + 0x24, u8b, sizeof u8b);
        memcpy(ans + 0x8b, u16b, sizeof u16b);
        snprintf(host_name, sizeof host_name, "Alpha");
        T(!has_name(ans, sizeof ans), "the host name filter: Alpha in an answer from Alpha2");
        snprintf(host_name, sizeof host_name, "Alpha2");
        T(has_name(ans, sizeof ans), "the host name filter: Alpha2 in an answer from Alpha2");
        // a name past ASCII, in its UTF-16 copy alone (SPLAT3_MATCH_HOST is UTF-8: each character one UTF-16 unit)
        static const uint8_t u16c[] = {0, 0, 0xc9, 0, 'v', 0, 'a', 0, 0, 0};   // E acute (U+00C9), v, a
        memset(ans + 0x20, 0, sizeof ans - 0x20);
        memcpy(ans + 0x8b, u16c, sizeof u16c);
        snprintf(host_name, sizeof host_name, "\xc3\x89va");
        T(has_name(ans, sizeof ans), "the host name filter: a name past ASCII in UTF-16");
        snprintf(host_name, sizeof host_name, "%s", host0);
    }
    // the room's stage name a guest takes (room_now): a scene name, else none
    T(scene_name_ok("Vss_Pivot03") && !scene_name_ok("") && !scene_name_ok("X\"}, \"state\": \"error") &&
      !scene_name_ok("Vss_\xc3\xa9") && !scene_name_ok("Vss Pivot"), "scene names");
    {   // the shader translator's form: every character past ASCII escaped, as UTF-16 units
        char o[64];
        json_esc_n(o, sizeof o, "\xc3\xa9\xf0\x9d\x84\x9e\xff/", (size_t)-1, JSON_ESC_ASCII);
        T(!strcmp(o, "\\u00e9\\ud834\\udd1e\\ufffd/"), "json_esc_n, ASCII form");
        // a cut only between whole characters and escapes: 2 bytes of room left after "ab" never take half of one
        json_esc(o, 5, "ab\n");
        T(!strcmp(o, "ab"), "json_esc cut before an escape");
        json_esc(o, 4, "ab\xc3\xa9");
        T(!strcmp(o, "ab"), "json_esc cut before a UTF-8 sequence");
        json_esc_n(o, sizeof o, "abc\"def", 4, JSON_ESC_UTF8);
        T(!strcmp(o, "abc\\\""), "json_esc_n stops at n bytes");
    }
    printf("probe-automatch: %s (%d failures)\n", fails ? "FAILED" : "passed", fails);
#undef T
    return fails != 0;
}

// The team screen sets the save's team of each place (private settings +0x42c, u32 per place: measured 4 before any
// choice, Right -> 0 Alpha -> 1 Bravo); the room takes them when the screen is confirmed (room +0xdc0, -1 before).
static int save_team(int row) {
    uint64_t ps = save_part(SAVE_PRIVATE);
    return ps && readable(ps + PRIV_TEAMS + 4 * row, 4) ? (int)rd32(ps + PRIV_TEAMS + 4 * row) : -9;
}

static int room_team(int row) {
    uint64_t rm = LOAD(room_model);
    return rm && readable(rm + 0xdc0 + 4 * row, 4) ? (int)rd32(rm + 0xdc0 + 4 * row) : -9;
}

// A row's team in the room is the one asked for. A spectator's place keeps no team in the room's team array (measured,
// match SP1: save places 0 1 3, room 0 1 -1 after "Teams OK", and the pre-match screen listed the third player under
// the spectators' stands), so a spectator row is right when the room put it in no team (Alpha, Bravo or the third)
static int place_ok(int asked, int room) {
    if (asked == PLACE_SPECTATOR) return room != -9 && (room < 0 || room > 2);
    return room == asked;
}
static int room_row_ok(int row) { return place_ok(team_row[row], room_team(row)); }

// Diagnostics for the spectators' places: the room model around its team array and the room's net state, as bytes
static void room_dump(const char *when) {
    int sp = 0;
    for (int r = 0; r < 10; r++) sp += team_row[r] == PLACE_SPECTATOR;
    if (!sp) return;
    uint64_t rm = LOAD(room_model), nv = LOAD(net_versus);
    char what[96];
    if (rm && readable(rm + 0xd00, 0x200)) {
        snprintf(what, sizeof what, "room model +0xd00 (%s)", when);
        hexdump(what, (const uint8_t *)(uintptr_t)(rm + 0xd00), 0x200);
    }
    if (nv && readable(nv, 0x80)) {
        snprintf(what, sizeof what, "the room's versus net state (%s)", when);
        hexdump(what, (const uint8_t *)(uintptr_t)nv, 0x80);
    }
}

static int ui_state(void) {
    uint64_t ui = LOAD(room_ui);
    return ui && readable(ui + UI_STATE, 4) ? (int)rd32(ui + UI_STATE) : -1;
}

// The frames the room screen has spent in its current state (UI_FRAMES), -1 while the screen is unknown
static int64_t ui_frames(void) {
    uint64_t ui = LOAD(room_ui);
    return ui && readable(ui + UI_FRAMES, 4) ? (int64_t)rd32(ui + UI_FRAMES) : -1;
}

// Waits until the room screen is in state st (-1: any) and has been for `frames` of its frames. 1 then; 0 after max_ms of
// wall time (the frames are the bound on a machine that runs; the seconds only on one that hardly does), or as soon as
// it has spent `away` frames in the state `other` (a step that went elsewhere; -2: none).
static int ui_wait(int st, int64_t frames, int other, int64_t away, uint32_t max_ms) {
    uint64_t end = plat_now_ms() + max_ms;
    for (;;) {
        int s = ui_state();
        int64_t f = ui_frames();
        if ((st < 0 || s == st) && f >= frames) return 1;
        if (s == other && f >= away) return 0;
        if (!before(end)) return 0;
        sleep_ms(10);
    }
}

// Waits until the room screen leaves state s, or has spent `frames` more of its frames in it (max_ms of wall time at
// most). 1 when it left.
static int ui_leaves(int s, int64_t frames, uint32_t max_ms) {
    int64_t f0 = ui_frames();
    uint64_t end = plat_now_ms() + max_ms;
    while (ui_state() == s && ui_frames() < f0 + frames && before(end)) sleep_ms(10);
    return ui_state() != s;
}

// "Ready" and the team screen. Each step is gated on the room screen's state machine (UI_STATE, UI_FRAMES) and its
// presses are spaced in the game's frames (press_paced), never in seconds alone: the game's menus count frames, so a
// machine that shows fewer of them a second takes the same steps, only later (docs/auto-match.md, "Slow machines").
// - The teams asked for per row (the players in the order they came in) go into the save's at the commit and are read
//   back; the team screen opens on them.
// - "Ready": the room screen idle READY_IDLE frames (a screen back after a match ignores presses during its entry), then
//   Down, Down, A until its state changes (again after RETRY_FRAMES, 3 times at most).
// - "Ready" starts the game's check of the player list (a message, about 120 frames), then the team screen comes (its
//   state UI_TEAM_SCREEN); the flow waits for that state and TEAMS_IDLE frames in it. It used to wait 3 s from the
//   check's start instead, which the check outlasts on a host below about 40 frames a second (or with long frames in
//   those seconds): the presses then met the message, the change from the check to the team screen was taken for
//   "Teams OK", and the room's teams were still unset 3 s later (teams-not-set, every row -1 in the room).
// - "Teams OK": Down once per row and once more (the cursor starts on the first row and stops on "Teams OK", the last
//   item: measured, n + 1 presses confirm), then A, until the screen leaves the team screen (again after RETRY_FRAMES,
//   3 times at most);
// - every asked row's team is then checked in the room (set in the frame "Teams OK" is taken).
// 0 and why when a step does not happen.
#define READY_IDLE 60      // frames the room screen shows its state before "ready" is pressed
#define TEAMS_IDLE 30      // frames the team screen shows before its first press
#define RETRY_FRAMES 180   // frames without the step's effect before its presses are made again
#define STEP_MAX_MS 60000  // wall-clock bound of each wait (a machine at 5 frames a second still counts 300 frames)
static int in_session = 2;   // the host's room when it starts the match: players, or fewer when spectators did not come
static int host_setup(char *why, size_t cap) {
    for (int r = 0; r < 10; r++) team_row[r] = -1;
    for (int r = 0; r < in_session && r < 10; r++) {
        char t = teams[0] || nby_name ? row_team(r) : '-';
        team_row[r] = t == 'A' ? 0 : t == 'B' ? 1 : t == 'S' ? PLACE_SPECTATOR : -1;
    }
    plat_xchg32(&teams_on, 1);
    int32_t c0 = LOAD(n_commit);
    uint64_t end = plat_now_ms() + 3000;
    while (LOAD(n_commit) < c0 + 3 && before(end)) sleep_ms(50);
    for (int r = 0; r < 10; r++)
        if (team_row[r] >= 0 && save_team(r) != team_row[r]) {
            plat_xchg32(&teams_on, 0);
            return snprintf(why, cap, "row %d: the save's team is %d, %d asked", r, save_team(r), team_row[r]), 0;
        }
    if (!ui_wait(-1, READY_IDLE, -2, 0, STEP_MAX_MS)) {
        plat_xchg32(&teams_on, 0);
        return snprintf(why, cap, "the room screen did not settle (state %d, %lld frames in it)", ui_state(),
                        (long long)ui_frames()), 0;
    }
    int s0 = ui_state();
    for (int attempt = 0; attempt < 3 && ui_state() == s0; attempt++) {
        if (attempt) ui_leaves(s0, RETRY_FRAMES, STEP_MAX_MS);
        if (ui_state() != s0) break;
        press_paced("DOWN", 48);
        press_paced("DOWN", 48);   // "ready" under the mode and the stage
        shot("room-ready");
        press("A");
        ui_leaves(s0, RETRY_FRAMES, STEP_MAX_MS);
    }
    if (ui_state() == s0) {
        plat_xchg32(&teams_on, 0);
        return snprintf(why, cap, "\"ready\" was not taken (room screen state %d)", s0), 0;
    }
    // the check of the player list, then the team screen; back on the room's state for 60 frames: the check refused
    if (!ui_wait(UI_TEAM_SCREEN, TEAMS_IDLE, s0, 60, STEP_MAX_MS)) {
        plat_xchg32(&teams_on, 0);
        return snprintf(why, cap, "the team screen did not open (room screen state %d, %lld frames in it)", ui_state(),
                        (long long)ui_frames()), 0;
    }
    mark("teams screen");
    shot("teams");
    teams_log("on the team screen");
    room_dump("on the team screen");
    int left = 0;
    for (int attempt = 0; attempt < 3 && !left; attempt++) {
        if (attempt) rt_log("automatch: +%.1f s \"Teams OK\" not taken after %d frames: again", now_s(), RETRY_FRAMES);
        for (int r = 0; r <= in_session && r < 11; r++) press_paced("DOWN", 30);
        shot("teams-set");
        press("A");   // "Teams OK"
        left = ui_leaves(UI_TEAM_SCREEN, RETRY_FRAMES, STEP_MAX_MS);
    }
    plat_xchg32(&teams_on, 0);
    if (!left) return snprintf(why, cap, "the team screen did not take \"Teams OK\" (room screen state %d)", ui_state()), 0;
    mark("teams confirmed");
    end = plat_now_ms() + 3000;   // the room has the teams from the frame "Teams OK" is taken
    int bad = -1;
    do {
        bad = -1;
        for (int r = 0; r < in_session && r < 10 && bad < 0; r++)
            if (team_row[r] >= 0 && !room_row_ok(r)) bad = r;
        if (bad >= 0) sleep_ms(100);
    } while (bad >= 0 && before(end));
    teams_log("confirmed");
    room_dump("confirmed");
    if (bad >= 0)
        return snprintf(why, cap, "row %d: team %d in the room, %d asked (room screen state %d)", bad, room_team(bad),
                        team_row[bad], ui_state()), 0;
    return 1;
}

// 1 joined, 0 no session answered, -1 a session answered but the join did not happen
static int join_session(uint64_t end) {
    // No host answered the search yet: search again (X) until one does
    while (!LOAD(n_reply) && before(end)) {
        int32_t d0 = LOAD(n_browse_done);
        sleep_ms(2000);
        press("X");
        search_ended(d0);
    }
    if (!LOAD(n_reply)) return 0;
    mark("a session is listed");
    for (int attempt = 1; before(end); attempt++) {
        int32_t c0 = LOAD(n_close), b0 = LOAD(n_bind), q0 = LOAD(n_browse_req), d0 = LOAD(n_browse_done);
        press("A");   // the first session of the list
        sleep_ms(1500);
        press("A");   // join
        if (restarted(c0, b0, 8000)) {
            mark("session joined");
            jobs_log("joined");
            return 1;
        }
        rt_log("automatch: not joined after A, A (attempt %d)", attempt);
        jobs_log("join failed");
        if (!back_to_list(q0, d0, end)) return -1;
    }
    return -1;
}

// The pre-match screen: every player confirms with A; the stage then loads. 1 loading, 0 time up, -1 a member left
// (*who: the host for a guest)
static int to_stage(uint32_t period_ms, uint64_t end) {
    int32_t v0 = LOAD(seq_vss);
    uint64_t next = 0, worst = 0, told = 0;
    while (before(end)) {
        if (LOAD(seq_vss) > v0) {
            rt_log("automatch: longest silence of a member in the room: %.1f s", worst / 1000.0);
            return 1;
        }
        if (plat_now_ms() >= next) press("A"), next = plat_now_ms() + period_ms;
        uint64_t silent = 0;
        members_present(&silent);
        if (role == JOIN) silent = host_silent_ms();
        if (silent > worst) worst = silent;
        if (silent >= 5000 && silent / 5000 != told) {
            told = silent / 5000;
            rt_log("automatch: +%.1f s a member silent for %.1f s", now_s(), silent / 1000.0);
        }
        if (silent >= PEER_SILENT_MS) return -1;
        sleep_ms(50);
    }
    return 0;
}

// ---- the record
// This thread is not a guest thread: a page of the guest heap the game has not committed would fault here, so the OS is
// asked too (plat_mem_readable), not only the layout.
static int readable(uint64_t a, uint64_t n) { return mem_guest_readable(a, n) && plat_mem_readable(a, n); }

static uint64_t rd64(uint64_t a) {
    return a && !(a >> 48) && readable(a, 8) ? *(volatile const uint64_t *)(uintptr_t)a : 0;
}
static uint32_t rd32(uint64_t a) {
    return a && !(a >> 48) && readable(a, 4) ? *(volatile const uint32_t *)(uintptr_t)a : 0;
}

static uint64_t base_utc;   // the record's DateTimeUtc before the match (the lobby's state is not saved: 0 at boot)

// The local lobby's state as scene_start.c finds it: *(*(*(main+0x60f24e0) + 0xd8) + 0x6678); 0 when unreadable.
// Measured: the match writes LastVersusRecord there when its results start (the scan of holder slots +0xc0..+0xf8 in
// the first runs found it at +0xd8 only, on both players).
static uint64_t lobby_state(void) {
    uint64_t g = s3 ? rd64(main_base + SCENE_STATES) : 0;
    uint64_t x = g ? rd64(g + 0xd8) : 0;
    uint64_t s = x ? rd64(x + LOBBY_LOCAL_STATE) : 0;
    return s && readable(s, REC_OFF + REC_SIZE) ? s : 0;
}

// A finished match's record: a new DateTimeUtc, frames, 2 to 4 teams and 1 to 8 players
static uint64_t find_record(void) {
    uint64_t s = lobby_state();
    if (!s) return 0;
    uint64_t r = s + REC_OFF, utc = rd64(r + 0x98);
    uint32_t frames = rd32(r + 0xa4), nt = rd32(r + 0xb8), np = rd32(r + 0x5f8);
    return utc && utc != base_utc && frames && nt >= 2 && nt <= 4 && np >= 1 && np <= 8 ? s : 0;
}

// The seat the match's record gives this player. Every member's game gets the same record, the players' entries only;
// MyPersonalIndex (+0x7900) is the player's own entry, -1 on a spectator's game (measured, match SP2: two players and a
// spectator, MyPersonalIndex 0 on the host's game, 1 on the other player's, -1 on the spectator's)
static int seat_of_index(uint32_t me, uint32_t np) { return me < np ? SEAT_PLAYER : SEAT_SPECTATOR; }
static int record_seat(uint64_t s) { return seat_of_index(rd32(s + REC_OFF + 0x7900), rd32(s + REC_OFF + 0x5f8)); }

// The raw record for analysis: u32 magic "S3AM", state bytes, team bytes, player bytes; u64 guest addresses of the state,
// the team buffer and the player buffer; then those bytes
static void dump_state(uint64_t s, const char *path) {
    FILE *f = fopen(path, "wb");
    if (!f) return;
    uint64_t r = s + REC_OFF;
    uint64_t tb = rd64(r + 0xa8), pb = rd64(r + 0x5e8);
    uint32_t tc = rd32(r + 0xb0), pc = rd32(r + 0x5f0);
    if (tc > 4) tc = 4;
    if (pc > 8) pc = 8;
    uint32_t hdr[4] = {0x4d413353u, REC_OFF + 0x600, tb && readable(tb, tc * TEAM_SIZE) ? tc * TEAM_SIZE : 0,
                       pb && readable(pb, pc * PERSON_SIZE) ? pc * PERSON_SIZE : 0};
    uint64_t addrs[3] = {s, tb, pb};
    fwrite(hdr, sizeof hdr, 1, f);
    fwrite(addrs, sizeof addrs, 1, f);
    fwrite((const void *)(uintptr_t)s, 1, hdr[1], f);
    if (hdr[2]) fwrite((const void *)(uintptr_t)tb, 1, hdr[2], f);
    if (hdr[3]) fwrite((const void *)(uintptr_t)pb, 1, hdr[3], f);
    fclose(f);
}

// A guest C string (a player's name in the match's record: another player chose it) as JSON string contents
static void json_str(char *out, size_t cap, uint64_t a) {
    char raw[65];
    int k = 0;
    for (; a && k < 64 && readable(a + k, 1); k++)
        if (!(raw[k] = *(const char *)(uintptr_t)(a + k))) break;
    raw[k] = 0;
    json_esc(out, cap, raw);
}

static const char *team_name(uint32_t t) { return t == 0 ? "alpha" : t == 1 ? "bravo" : t == 2 ? "charlie" : "none"; }

// The record as JSON. Layout (readers main+0x3d80050 for the record, 0x3d7e724 for a player, 0x3567a00 for its
// UserSetting, 0x3d89df8 for its Result; team words measured against the players' points and the result screen):
//   record: Rule +0x30 (0 turf war), DeemedResult +0x90, StageId +0x94, DateTimeUtc +0x98, WinTeam +0xa0 (0 Alpha,
//     1 Bravo), BattleFrame +0xa4 (60 per second), teams {buffer +0xa8, capacity +0xb0, start +0xb4, count +0xb8},
//     players {+0x5e8, +0x5f0, +0x5f4, +0x5f8}, MyPersonalIndex +0x7900;
//   team (0x1b8 bytes, index = team): paint points +0xa0, turf share in tenths of a percent +0xa4;
//   player (0xe60 bytes): UserSetting +0x30 (UserName: chars at [+0xe0], Team +0x150), Result +0xa58 (KillNum +0xac8,
//     KillAssistNum +0xacc, DeathNum +0xad0, PaintPoint +0xad4, SpecialUsageNum +0xad8, IsDisconnectedPlayer +0xadc),
//     PlayerIndex +0xe50.
static void write_result(uint64_t s, const char *outcome, const char *reason) {
    if (!result_path[0]) return;
    char tmp[600];
    snprintf(tmp, sizeof tmp, "%s.tmp", result_path);
    FILE *f = fopen(tmp, "wb");
    if (!f) {
        rt_log("automatch: cannot write %s", tmp);
        return;
    }
    char vss[64];
    plat_mutex_lock(&lock);
    snprintf(vss, sizeof vss, "%s", vss_name);
    plat_mutex_unlock(&lock);
    // A spectator's file is never a player's report: its own seat, and "spectated" where a player's says "completed"
    int spectator = seat_now() == SEAT_SPECTATOR;
    if (spectator && !strcmp(outcome, "completed")) outcome = "spectated";
    fprintf(f, "{\n  \"version\": 2,\n  \"match_id\": \"%s\",\n  \"role\": \"%s\",\n  \"seat\": \"%s\",\n"
               "  \"seat_asked\": \"%s\",\n  \"round\": %d,\n  \"outcome\": \"%s\",\n",
            JE(match_id), JE(role_name()), JE(seat_word(seat_now())), JE(seat_word(seat_rules)), round_no, JE(outcome));
    if (reason) {
        char r[1200];
        json_esc(r, sizeof r, reason);
        fprintf(f, "  \"reason\": \"%s\",\n", r);
    }
    if (!s) {   // no record: what the room was set to
        fprintf(f, "  \"stage_scene\": \"%s\",\n", JE(vss));
        if (room_known) {
            char room[ROOM_JSON_CAP];
            room_json(room, sizeof room, &room_last);
            if (room[0]) fprintf(f, "  %s,\n", room);
        }
    }
    if (s) {
        uint64_t r = s + REC_OFF;
        uint32_t frames = rd32(r + 0xa4), win = rd32(r + 0xa0), rule = rd32(r + 0x30);
        uint32_t nt = rd32(r + 0xb8), np = rd32(r + 0x5f8), me = rd32(r + 0x7900);
        uint32_t stage = rd32(r + 0x94);
        fprintf(f, "  \"rule\": %u,\n  \"rule_name\": \"%s\",\n  \"mode\": \"%s\",\n  \"stage_id\": %u,\n  \"stage_scene\": \"%s\",\n",
                rule, JE(rule_name((int)rule)), JE(mode_key((int)rule)), stage, JE(vss));
        if (role == HOST) {
            fprintf(f, "  \"requested\": {\"mode\": \"%s\", \"stage\": ", JE(mode_keys[want_rule]));
            if (want_stage < 0) fprintf(f, "\"random\"");
            else fprintf(f, "%d", want_stage);
            for (int i = 0; i < OPTS; i++) fprintf(f, ", \"%s\": %d", JE(opt_keys[i]), want_opt[i]);
            fprintf(f, "},\n  \"settings_ok\": %s,\n",
                    (int)rule == want_rule && (want_stage < 0 || (int)stage == want_stage) ? "true" : "false");
        }
        if (room_known) {
            char room[ROOM_JSON_CAP];
            room_json(room, sizeof room, &room_last);
            if (room[0]) fprintf(f, "  %s,\n", room);
        }
        fprintf(f, "  \"end_utc\": %llu,\n  \"battle_frames\": %u,\n  \"duration_s\": %.2f,\n",
                (unsigned long long)rd64(r + 0x98), frames, frames / 60.0);
        fprintf(f, "  \"win_team\": %u,\n  \"winner\": \"%s\",\n  \"deemed_result\": %d,\n", win, JE(team_name(win)),
                (int32_t)rd32(r + 0x90));
        uint64_t tb = rd64(r + 0xa8);
        uint32_t tcap = rd32(r + 0xb0), tstart = rd32(r + 0xb4);
        fprintf(f, "  \"teams\": [");
        for (uint32_t i = 0; i < nt && tcap && i < 4; i++) {
            uint64_t t = tb + (uint64_t)((tstart + i) % tcap) * TEAM_SIZE;
            // +0xa8: the count of the modes other than turf (100 at the start, measured 99 for the winner and 100 for
            // the loser of a rainmaker and a clams match, 100 for both in turf)
            fprintf(f, "%s\n    {\"team\": \"%s\", \"paint_points\": %u, \"turf_permille\": %u, \"count\": %u}",
                    i ? "," : "", JE(team_name(i)), rd32(t + 0xa0), rd32(t + 0xa4), rd32(t + 0xa8));
        }
        fprintf(f, "\n  ],\n  \"players\": [");
        uint64_t pb = rd64(r + 0x5e8);
        uint32_t pcap = rd32(r + 0x5f0), pstart = rd32(r + 0x5f4);
        int lo_ok = -1;   // SPLAT3_MATCH_LOADOUT: this player's entry has the weapon and gear asked for
        for (uint32_t i = 0; i < np && pcap && i < 8; i++) {
            uint64_t p = pb + (uint64_t)((pstart + i) % pcap) * PERSON_SIZE;
            char name[400];
            json_str(name, sizeof name, rd64(p + 0xe0));
            // weapon +0x240 and gear +0x2c8/+0x4d0/+0x6d8 (head, clothes, shoes): the game's ids, measured against the
            // profiles' weapon (two players, three matches) and gear (one player)
            fprintf(f, "%s\n    {\"name\": \"%s\", \"self\": %s, \"team\": \"%s\", \"paint\": %u, \"kills\": %u, "
                       "\"assists\": %u, \"deaths\": %u, \"specials\": %u, \"disconnected\": %s, \"weapon\": %d, "
                       "\"gear\": [%d, %d, %d], \"abilities\": [",
                    i ? "," : "", name, i == me && !spectator ? "true" : "false", JE(team_name(rd32(p + 0x150))), rd32(p + 0xad4),
                    rd32(p + 0xac8), rd32(p + 0xacc), rd32(p + 0xad0), rd32(p + 0xad8),
                    rd32(p + 0xadc) & 0xff ? "true" : "false", (int32_t)rd32(p + 0x240), (int32_t)rd32(p + 0x2c8),
                    (int32_t)rd32(p + 0x4d0), (int32_t)rd32(p + 0x6d8));
            // Each gear's abilities, [main, sub, sub, sub] for head, clothes, shoes: the gear record's +0x14 and
            // +0xa0/+0x118/+0x190 hold the ability's position in main's ability enum (0: none), written here as the
            // enum's values, the save's (-1: an empty slot). Measured on two players' gear in three matches, against the
            // sub abilities their saves held; all sub slots read empty with the room's main_abilities_only.
            static const uint32_t gear_off[] = {0x2c8, 0x4d0, 0x6d8}, abil_off[] = {0x14, 0xa0, 0x118, 0x190};
            for (int g = 0; g < 3; g++)
                for (int k = 0; k < 4; k++)
                    fprintf(f, "%s%lld%s", k ? ", " : g ? ", [" : "[",
                            (long long)save_profile_ability_at((int32_t)rd32(p + gear_off[g] + abil_off[k])), k == 3 ? "]" : "");
            fprintf(f, "]}");
            if (lo_on && i == me && !spectator) {
                int32_t got[4] = {(int32_t)rd32(p + 0x240), (int32_t)rd32(p + 0x2c8), (int32_t)rd32(p + 0x4d0),
                                  (int32_t)rd32(p + 0x6d8)};
                int64_t want[4] = {lo_weapon, lo_gear[0], lo_gear[1], lo_gear[2]};
                lo_ok = 1;
                for (int k = 0; k < 4; k++) lo_ok &= want[k] < 0 || want[k] == got[k];
            }
        }
        fprintf(f, "\n  ],\n");
        if (lo_on && !spectator)   // a spectator plays no loadout: none to check
            fprintf(f, "  \"loadout\": {\"weapon\": %lld, \"gear\": [%lld, %lld, %lld], \"ok\": %s},\n",
                    (long long)lo_weapon, (long long)lo_gear[0], (long long)lo_gear[1], (long long)lo_gear[2],
                    lo_ok == 1 ? "true" : "false");
    }
    fprintf(f, "  \"t0_unix\": %.3f,\n  \"timeline_s\": [", t0_unix);
    for (int i = 0; i < ntimeline; i++)
        fprintf(f, "%s\n    [\"%s\", %.1f]", i ? "," : "", JE(timeline[i].what), timeline[i].t);
    fprintf(f, "\n  ]\n}\n");
    fclose(f);
    if (!plat_file_replace(tmp, result_path)) rt_log("automatch: cannot move %s to %s", tmp, result_path);
    else rt_log("automatch: result written to %s", result_path);
}

// Ends the process as the window's close does (gfx_window_closed: the save flushed, the pipeline cache and the play
// time written), with exit code 3: the flow failed
static _Noreturn void exit_failed(void) {
    rt_log("automatch: closing the game after the error (exit code 3)");
    fs_save_flush();
    fs_match_end();
    vk_pipeline_save();
    playtime_save();
    prof_write();
    plat_exit(3);
}

// The flow stops: the error in the status and the result files; then the game closes out of sight, or with
// SPLAT3_MATCH_ON_ERROR=show the window is shown on what the game shows now
static void fail(const char *code, const char *fmt, ...) {
    char why[400];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(why, sizeof why, fmt, ap);
    va_end(ap);
    why[rt_brand_vars(why, strlen(why))] = 0;   // the reason names the variables as the documentation does
    fnprof_off(FNPROF_AUTO);
    rt_log("automatch: +%.1f s stopped: %s (%s)", now_s(), why, code);
    shot("failed");
    if (error_show) reveal("error, SPLAT3_MATCH_ON_ERROR=show");
    else hide();
    status_write("error", code, 0, 0, why);
    if (error_show) return;
    sleep_ms(500);   // the failure shot
    exit_failed();
}

// ---- a player who leaves (docs/auto-match.md "Leave match"). What a departure means depends on the session at that
// moment. Before the match (the stage loading, or earlier): LEAVE_BEFORE. In the match: LEAVE_LOST when the session had
// already stopped for LEAVE_HEARD_MS (a guest: its host silent; the host: every member silent): the other game vanished
// first and the game's own "communication error" (2 to 5 s later) is on screen, so the player who leaves is not the
// one who broke the match; LEAVE_IN_PLAY when every other player of the match was heard within LEAVE_HEARD_MS: the
// match was being played; else LEAVE_PARTLY (another player gone, the session alive for the others). A server's policy
// may make a departure in play lose the match (protocol 1.1.11, `leave`): only LEAVE_IN_PLAY says `in_play: true`.
enum { LEAVE_BEFORE, LEAVE_IN_PLAY, LEAVE_PARTLY, LEAVE_LOST };

// *silent: the longest silence (ms) of another player of the match, or of the session for LEAVE_LOST
static int leave_state(uint64_t *silent) {
    plat_mutex_lock(&status_lock);
    int in_match = !strcmp(cur_state, "in-match");
    plat_mutex_unlock(&status_lock);
    *silent = 0;
    if (!in_match) return LEAVE_BEFORE;
    uint64_t now = plat_now_ms(), heard = UINT64_MAX, q[16];
    uint32_t ips[16], members = (uint32_t)LOAD(match_members);
    int k = 0;
    plat_mutex_lock(&lock);
    for (int i = 0; i < 16 && peers[i]; i++) {
        uint64_t t = now - peer_seen[i];
        if (t < heard) heard = t;
        if (members & (1u << i)) ips[k] = peers[i], q[k++] = t;
    }
    plat_mutex_unlock(&lock);
    uint64_t gone = role == JOIN ? host_silent_ms() : heard;
    if (gone >= LEAVE_HEARD_MS) {   // the host's traffic (guest), or every member's (host), stopped: the match was lost
        *silent = gone == UINT64_MAX ? 0 : gone;
        return LEAVE_LOST;
    }
    for (int i = 0; i < k; i++)
        if (q[i] > *silent && !member_is_spectator(ips[i])) *silent = q[i];
    return *silent < LEAVE_HEARD_MS ? LEAVE_IN_PLAY : LEAVE_PARTLY;
}

// The departure's error: its code (left-by-player, or connection-lost for a match already lost), its sentence, and
// the status members in_play and session_silent_ms (status_leave)
static const char *departure(const char *how, char *why, size_t cap) {
    uint64_t silent = 0;
    int st = leave_state(&silent);
    snprintf(status_leave, sizeof status_leave, "\"in_play\": %s, \"session_silent_ms\": %llu",
             st == LEAVE_IN_PLAY ? "true" : "false", (unsigned long long)silent);
    if (st == LEAVE_LOST) {
        snprintf(why, cap, "the player left the match %s %.1f s after the session's traffic stopped (the match was "
                           "already lost)", how, silent / 1000.0);
        return "connection-lost";
    }
    snprintf(why, cap, "the player left the match %s%s", how,
             st == LEAVE_IN_PLAY ? " while it was being played"
             : st == LEAVE_BEFORE ? " before it began"
                                  : " after another player's game went silent");
    return "left-by-player";
}

// "Leave match" of the in-game menu: the same stop as any other failure (the error in the status file, window hidden,
// exit code 3), taken from its own thread because fail() waits for the failure shot and the menu draws on the present
// path. Once only; the flow's own failure, if it comes first, wins.
static volatile int32_t leaving;
static void leave_main(void *arg) {
    (void)arg;
    error_show = 0;
    char why[200];
    const char *code = departure("from the game's menu", why, sizeof why);
    fail(code, "%s", why);
}

void automatch_leave(void) {
    if (!automatch_on || plat_xchg32(&leaving, 1)) return;
    if (!plat_thread_spawn(leave_main, NULL, "automatch-leave", 0)) leaving = 0;
}

// The window closed (its close button, the menu's Quit to launcher) while the stage loads or the match runs: the same
// departure as Leave match, written to the status file before gfx_window_closed ends the process (exit code 0), so the
// launcher reports what the game knew (in_play, or connection-lost for a match already lost) rather than guessing from
// the exit code. Nothing in the other steps: there the launcher's own reading of the exit stands.
void automatch_closing(void) {
    if (!automatch_on) return;
    plat_mutex_lock(&status_lock);
    int playing = !strcmp(cur_state, "in-match") || !strcmp(cur_state, "loading-stage");
    plat_mutex_unlock(&status_lock);
    if (!playing || plat_xchg32(&leaving, 1)) return;
    char why[200];
    const char *code = departure("by closing the game's window", why, sizeof why);
    rt_log("automatch: +%.1f s the window closed in the match: %s (%s)", now_s(), why, code);
    status_write("error", code, 0, 0, why);
}

// The room's stage list into the status (stages_json); with the room up, up to ms for the save to fill it
static int stages_note(uint32_t ms) {
    int32_t ids[64];
    int n = 0;
    uint64_t end = plat_now_ms() + ms;
    while (!(n = stage_list(ids, 64)) && before(end)) sleep_ms(100);
    int k = 0;
    jcat(stages_json, sizeof stages_json, &k, "\"stages\": [");
    for (int i = 0; i < n; i++) jcat(stages_json, sizeof stages_json, &k, "%s%d", i ? ", " : "", ids[i]);
    // the same stages by their scene, the identifier stage= also takes (RSDB/VersusSceneInfo __RowId)
    jcat(stages_json, sizeof stages_json, &k, "], \"stage_scenes\": [");
    RsdbTable *t = rsdb_open("VersusSceneInfo");
    for (int i = 0; i < n; i++) {
        const char *scene = "";
        for (uint32_t r = 0; t && r < rsdb_rows(t) && !*scene; r++)
            if (rsdb_int(t, r, "Id", -1) == ids[i]) scene = rsdb_str(t, r, "__RowId");
        jcat(stages_json, sizeof stages_json, &k, "%s\"%s\"", i ? ", " : "", JE(scene));
    }
    rsdb_close(t);
    jcat(stages_json, sizeof stages_json, &k, "]");
    jdone(stages_json, k, "the room's stage list");
    rt_log("automatch: the room's stage list (VersusSceneInfo Id): %s", stages_json);
    room_status(NULL);
    return n;
}

// ---- the command file (SPLAT3_MATCH_CMD): one line per command, appended by the launcher; only lines appended after
// the game started count. "next [id=<match id>] [<rules>]": another round in the same room (<rules> as
// SPLAT3_MATCH_RULES); "quit": close the game.
static char cmd_path[512];
static uint64_t cmd_taken;   // bytes of the command file already taken (its size when the game started, at first)

static int cmd_take(char *line, size_t cap) {
    uint64_t n = 0;
    uint8_t *b = cmd_path[0] ? plat_file_read(cmd_path, &n) : NULL;
    int got = 0;
    if (b && n > cmd_taken) {
        uint8_t *nl = memchr(b + cmd_taken, '\n', (size_t)(n - cmd_taken));
        if (nl) {   // a complete line
            size_t k = (size_t)(nl - (b + cmd_taken));
            snprintf(line, cap, "%.*s", (int)(k < cap - 1 ? k : cap - 1), (const char *)b + cmd_taken);
            line[strcspn(line, "\r")] = 0;
            cmd_taken += k + 1;
            got = 1;
        }
    } else if (b && n < cmd_taken) {
        cmd_taken = n;   // the launcher truncated the file: its next lines count from here
    }
    free(b);
    return got;
}

// The result and status files of a round: a path with "{id}" gets the round's match id there
static char result_tmpl[512], status_tmpl[512];
static void round_paths(void) {
    char *paths[2] = {result_path, status_path};
    const char *tmpls[2] = {result_tmpl, status_tmpl};
    for (int i = 0; i < 2; i++) {
        const char *at = strstr(tmpls[i], "{id}");
        if (at) snprintf(paths[i], 512, "%.*s%s%s", (int)(at - tmpls[i]), tmpls[i], match_id, at + 4);
    }
}

// Waits for the launcher's next command between rounds (out of sight). 1: next (settings taken), 0: quit
static int wait_command(void) {
    status("waiting-command", "round %d is over; waiting for the next command in %s", round_no, cmd_path);
    char line[600];
    for (;;) {
        if (!cmd_take(line, sizeof line)) {
            sleep_ms(100);
            continue;
        }
        rt_log("automatch: +%.1f s command \"%s\"", now_s(), line);
        if (!strcmp(line, "quit")) return 0;
        if (strncmp(line, "next", 4) || (line[4] && line[4] != ' ')) {
            rt_log("automatch: command \"%s\" unknown (next [id=<match id>] [<rules>], quit); ignored", line);
            continue;
        }
        char *p = line + 4, rules[500] = "";
        while (*p) {
            p += strspn(p, " ");
            size_t k = strcspn(p, " ");
            if (!k) break;
            if (!strncmp(p, "id=", 3)) {
                char id[65];
                snprintf(id, sizeof id, "%.*s", (int)(k - 3 < 64 ? k - 3 : 64), p + 3);
                if (*id && strspn(id, "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_") == strlen(id))
                    snprintf(match_id, sizeof match_id, "%s", id);
            } else {
                snprintf(rules + strlen(rules), sizeof rules - strlen(rules), "%s%.*s", rules[0] ? "," : "", (int)k, p);
            }
            p += k;
        }
        rules_error[0] = 0;
        parse_rules(rules);
        if (!rules_error[0]) teams_check(rules_error, sizeof rules_error);   // refused: why in rules_error
        if (rules_error[0] || (want_stage >= 0 && !stage_scene(want_stage, want_stage_scene, sizeof want_stage_scene))) {
            rt_log("automatch: command rules \"%s\": %s; ignored", rules, rules_error[0] ? rules_error : "stage unknown");
            status("waiting-command", "the last command's rules cannot work (%s); waiting for another",
                   rules_error[0] ? rules_error : "stage unknown");
            continue;
        }
        if (want_stage < 0) snprintf(want_stage_scene, sizeof want_stage_scene, "random");
        seat_rules = rules_seat();   // new teams may seat this player elsewhere (a guest's next names none: kept)
        return 1;
    }
}

// Back to a round's start: the counters of the stage, its intro and the record, the room's settings seen
static void round_reset(void) {
    plat_xchg32(&in_room, 0);
    plat_xchg32(&stage_open, 0);
    plat_xchg32(&seq_intro, 0);
    plat_xchg32(&seq_intro_frames, 0);
    plat_xchg32(&seq_vss_go, 0);
    plat_xchg32(&stage_check, 0);
    seat_game = SEAT_UNKNOWN;   // this round's record says it again
    room_known = 0;
    ntimeline = 0;
    uint64_t st = lobby_state();
    base_utc = st ? rd64(st + REC_OFF + 0x98) : 0;   // the record of the round before
    round_paths();
}

// Host, between rounds: the room screen up (measured: after the result screens the game is back on it, its cursor on
// the mode, the session kept). Its commit runs every frame; if it does not, L opens the match menu and A the session.
static int host_back_to_room(uint64_t end) {
    int32_t c = LOAD(n_commit);
    sleep_ms(1000);
    if (LOAD(n_commit) != c) {
        mark("room screen up");
        return 1;
    }
    for (int attempt = 1; before(end); attempt++) {
        int32_t c0 = LOAD(n_commit);
        press("L");
        sleep_ms(1500);
        press("A");
        uint64_t e = plat_now_ms() + 4000;
        while (LOAD(n_commit) == c0 && before(e)) sleep_ms(50);
        if (LOAD(n_commit) != c0) {
            mark("room screen up again");
            return 1;
        }
        rt_log("automatch: the room screen did not come up (attempt %d): back out and again", attempt);
        press("B");
        sleep_ms(1000);
        press("B");
        sleep_ms(2500);
    }
    return 0;
}

// ---- the match's save (fs_host.c "the match's copy of the save"): the game plays on a copy of the player's save,
// which got SPLAT3_MATCH_LOADOUT's profile and the season that opens every stage of the room's list.
// 1 while the copy is fine (or not made yet); 0 when the flow stopped on it (fail called)
static int match_save_ok(void) {
    char why[320];
    int s = fs_match_save(why, sizeof why);
    if (s == -2) fail("loadout-failed", "%s", why);
    else if (s < 0) fail("save-unavailable", "%s", why);
    if (s < 0) return 0;
    if (s == 1 && !status_save[0]) {
        lo_on = save_profile_match_loadout(&lo_weapon, lo_gear);
        char lo[160] = "";
        if (lo_on)
            snprintf(lo, sizeof lo, ", \"loadout\": {\"weapon\": %lld, \"gear\": [%lld, %lld, %lld]}", (long long)lo_weapon,
                     (long long)lo_gear[0], (long long)lo_gear[1], (long long)lo_gear[2]);
        plat_mutex_lock(&status_lock);
        snprintf(status_save, sizeof status_save, "\"save\": \"copy\"%s", lo);
        plat_mutex_unlock(&status_lock);
        rt_log("automatch: the match plays on a copy of the save%s", lo_on ? ", with the launcher's loadout" : "");
    }
    return 1;
}

// Setup of the first round: boot, the local lobby, the session list, the session created (host) or joined (guest).
// 0 when the flow stopped (fail called).
static int setup(void) {
    const LanCfg *lc = lan_cfg();
    char ra[16];
    uint64_t end = t0_ms + (uint64_t)t_s[T_BOOT] * 1000;
    while (!LOAD(seq_lobby) && before(end) && fs_match_save(NULL, 0) >= 0) sleep_ms(50);
    if (!match_save_ok()) return 0;
    if (!LOAD(seq_lobby)) {
        fail("boot-timeout", "the game did not reach its local lobby within %d s (step boot)", t_s[T_BOOT]);
        return 0;
    }
    if (fs_match_save(NULL, 0) != 1) {   // the lobby reads the save: its copy is made by now
        fail("save-unavailable", "the game reached its lobby without the match's copy of the save");
        return 0;
    }
    match_save_ok();
    mark("lobby loading");
    status("lobby", "the local lobby is loading");
    uint64_t failed_at = 0;
    while (!LOAD(n_nifm) && before(end)) {
        if (LOAD(n_nifm_failed) && !failed_at) failed_at = plat_now_ms();
        if (failed_at && plat_now_ms() - failed_at > 5000) break;
        sleep_ms(50);
    }
    if (!LOAD(n_nifm)) {
        int detail = 0;
        const char *why, *code = refusal_code(lan_relay_refusal(&detail), detail, &why);
        if (lc->mode == LAN_RELAY && why)   // the relay said why: the player is told (docs/auto-match.md)
            fail(code, "the relay %s:%u refused this player in room %s: %s", lan_ip_str(lc->relay_ip, ra), lc->relay_port,
                 lc->room, why);
        else if (lc->mode == LAN_RELAY)
            fail("network-unavailable", "the relay %s:%u did not take this player into room %s", lan_ip_str(lc->relay_ip, ra),
                 lc->relay_port, lc->room);
        else
            fail("network-unavailable", "the game's LAN network did not start%s",
                 failed_at ? " (the local network request failed)" : " in time (step boot)");
        return 0;
    }
    mark("lobby network up");
    if (role == HOST) plat_xchg32(&rules_on, 1);   // the room screen's commit takes the wanted settings (room_commit)
    status("lobby", "opening the session list");
    fnprof_on(FNPROF_AUTO);   // the session jobs and the room's settings are watched until the stage loads
    sleep_ms(4000);   // the lobby's entry animation; open_list retries anyway
    { uint64_t st = lobby_state(); base_utc = st ? rd64(st + REC_OFF + 0x98) : 0; }   // a record from before
    shot("lobby");
    if (!open_list(until(T_LIST))) {
        fail("list-timeout", "the session list did not open within %d s (step list)", t_s[T_LIST]);
        return 0;
    }
    shot("list");
    if (role == HOST) {
        status("creating-room", "creating the session");
        if (!host_create(until(T_CREATE))) {
            fail("create-failed", "no session could be created within %d s (step create)", t_s[T_CREATE]);
            return 0;
        }
        shot("room");
        return 1;
    }
    status("joining", host_name[0] ? "looking for the session of %s" : "looking for the session", host_name);
    int j = join_session(until(T_JOIN));
    if (j <= 0) {
        if (!j)
            fail("room-not-found", "no session%s%s%s answered within %d s (step join)", host_name[0] ? " of \"" : "",
                 host_name, host_name[0] ? "\"" : "", t_s[T_JOIN]);
        else
            fail("join-failed", "a session was listed but joining it did not succeed within %d s (step join)", t_s[T_JOIN]);
        return 0;
    }
    shot("joined");
    stages_note(3000);
    return 1;
}

static void close_game(const char *why);

// A round's room: host: the players in, the settings applied and checked, teams set, started; guest: the start.
// 0 when the flow stopped.
static int room_round(void) {
    if (role == HOST) {
        stages_note(10000);
        if (want_stage >= 0) {   // the commit checked the wanted stage against the room's list
            uint64_t e = plat_now_ms() + 10000;
            while (!LOAD(stage_check) && before(e)) sleep_ms(100);
            if (LOAD(stage_check) != 1) {
                fail("stage-unavailable", "stage %d (%s) is not in this player's private-battle stage list (%s)",
                     want_stage, want_stage_scene, stages_json[0] ? stages_json + 11 : "unknown");
                return 0;
            }
        }
        // Everyone asked for, or with teams by name every player and spectators_wait s more for the spectators: a
        // spectator who does not come never holds the players' match (it starts without them; their rows are not set)
        uint64_t end = until(T_PLAYERS), players_in_at = 0;
        int told = -1, k = 0, sp_want = spectators_asked(), sp_in = 0, all_players = 0;
        while ((k = members_present(NULL)) < players - 1) {
            int pl_in = sp_want ? named_present(&sp_in) : 0;
            all_players = sp_want && pl_in >= players - sp_want;
            if (all_players && !players_in_at) players_in_at = plat_now_ms();
            if (!all_players) players_in_at = 0;
            if (k * 2 + all_players != told) {
                told = k * 2 + all_players;
                char d[160];
                if (all_players)
                    snprintf(d, sizeof d, "every player is in; waiting up to %d s for the spectators (%d of %d in)",
                             spectators_wait, sp_in, sp_want);
                else
                    snprintf(d, sizeof d, "waiting for the players");
                status_write("waiting-for-players", NULL, k + 1, players, d);
            }
            if (players_in_at && plat_now_ms() - players_in_at >= (uint64_t)spectators_wait * 1000) break;
            if (!before(end)) break;
            sleep_ms(100);
        }
        if (k < players - 1 && !all_players) {
            status_write("waiting-for-players", NULL, k + 1, players, "waiting for the players");
            fail("players-timeout", "%d of %d players were in the room after %d s (step players)", k + 1, players,
                 t_s[T_PLAYERS]);
            return 0;
        }
        in_session = k + 1;
        if (k < players - 1) {
            char m[96];
            snprintf(m, sizeof m, "every player in, %d of %d spectators: starting without the others", sp_in, sp_want);
            mark(m);
            status_write("waiting-for-players", NULL, k + 1, players, m);
        } else {
            status_write("waiting-for-players", NULL, players, players, "every player is in the room");
            mark("all players in");
        }
        status("in-room", "setting the mode and the teams, then starting");
        sleep_ms(round_no == 1 ? 3000 : 5000);   // the newcomer's row appears; after a match, the screen's entry
        shot("room-full");
        plat_xchg32(&in_room, 1);
        RoomNow rn;
        if (!room_applied(10000, &rn)) {
            room_status(&rn);
            fail("rules-not-applied", "the room's settings did not become mode %s, stage %s within 10 s (commits %d; room: "
                 "rule %d, stage \"%s\", type %d)", mode_keys[want_rule], want_stage_scene, LOAD(n_commit), rn.rule,
                 rn.random ? "random" : rn.scene, rn.type);
            return 0;
        }
        room_status(&rn);
        mark("room settings applied");
        char why[200];
        if (!host_setup(why, sizeof why)) {
            fail("teams-not-set", "%s", why);
            return 0;
        }
        if (bench_teams) {   // a test aid: the lobby up to the teams, without the match
            close_game("bench: stopped once the teams were set");
            return 0;
        }
        int r = to_stage(2500, until(T_START));
        if (r < 0) {
            fail("player-left", "a player left the room before the match started");
            return 0;
        }
        if (!r) {
            fail("start-timeout", "the match did not start within %d s (step start)", t_s[T_START]);
            return 0;
        }
        return 1;
    }
    plat_xchg32(&in_room, 1);
    status("in-room", "in the host's room, waiting for the match to start");
    int r = to_stage(3000, until(T_START));
    if (r < 0) {
        fail("host-left", "the host left the room before the match started");
        return 0;
    }
    if (!r) {
        fail("start-timeout", "the match did not start within %d s (step start)", t_s[T_START]);
        return 0;
    }
    return 1;
}

// The game's own "the last match was abandoned" flag in the local lobby's state (its serializer main+0x34f8ee4 writes
// it as IsLastVersusAbandoned, a byte at +0xac08)
#define LOBBY_ABANDONED 0xac08ull
static int last_abandoned(void) {
    uint64_t st = lobby_state();
    return st && readable(st + LOBBY_ABANDONED, 1) ? *(volatile const uint8_t *)(uintptr_t)(st + LOBBY_ABANDONED) : -1;
}

// A round's match: the stage, its intro, the match, the record and the result screens. 1 when the game is back in its
// room (or the result screens' time is up), 0 when the flow stopped.
static int match_round(void) {
    char vss[64];
    plat_mutex_lock(&lock);
    snprintf(vss, sizeof vss, "%s", vss_name);
    plat_mutex_unlock(&lock);
    plat_xchg32(&frames_fine, 1);
    char m[96];
    snprintf(m, sizeof m, "stage loading (%s)", vss);
    mark(m);
    if (room_now(&room_last)) {   // the settings the room held when the match started (guest: as received)
        room_known = 1;
        if (!room_last.scene[0] && !room_last.random)   // a member that did not see the stage state: the stage loading
            snprintf(room_last.scene, sizeof room_last.scene, "%s", vss);
        room_status(&room_last);
        rt_log("automatch: the room's settings: rule %d (%s), stage %s", room_last.rule, rule_name(room_last.rule),
               room_last.random ? "random" : room_last.scene);
    }
    {   // the places as this member's copy of the room's net state holds them (+0x38: the 10 places' teams)
        uint64_t nv = LOAD(net_versus);
        if (nv && readable(nv, 0x80)) hexdump("the room's versus net state at the stage's load", (const uint8_t *)(uintptr_t)nv, 0x80);
    }
    fnprof_off(FNPROF_AUTO);
    if (reveal_mode == REVEAL_LOADING) reveal("stage loading");
    status("loading-stage", "loading the stage %s", vss);
    // The intro: its music started and its frames are presented (frames_main), or at the latest the match's music
    uint64_t end = until(T_STAGE);
    while (before(end) && !(LOAD(seq_intro) && LOAD(seq_intro_frames)) && !LOAD(seq_vss_go)) sleep_ms(10);
    if (!(LOAD(seq_intro) && LOAD(seq_intro_frames)) && !LOAD(seq_vss_go)) {
        fail("stage-timeout", "the stage %s did not reach its intro within %d s (step stage)", vss, t_s[T_STAGE]);
        return 0;
    }
    snprintf(m, sizeof m, "stage intro (%s)", LOAD(seq_intro_frames) ? "its frames" : "the match's music");
    mark(m);
    if (reveal_mode == REVEAL_INTRO) reveal("stage intro");
    else if (reveal_mode == REVEAL_NEVER) shot("intro");
    if (command_ms) {   // round 2 on: from the launcher's next command to this match's intro
        snprintf(m, sizeof m, "in the match %.1f s after the next command", (plat_now_ms() - command_ms) / 1000.0);
        mark(m);
    }
    note_match_members();
    status("in-match", "the match is on (%s)", vss);
    int32_t l0 = LOAD(seq_lobby);
    uint32_t silent_ms = match_silent_ms();
    sleep_ms(10000);
    plat_xchg32(&frames_fine, 0);
    shot("in-stage");
    if (play_s > 0 && seat_now() != SEAT_SPECTATOR) {   // a test aid: walk forward inking, so the teams' turf differs
        char l[64];
        snprintf(l, sizeof l, "ZR %d", play_s * 60);
        input_inject(l);
        snprintf(l, sizeof l, "LSTICK 0 32767 %d", play_s * 60);
        input_inject(l);
        sleep_ms((uint32_t)play_s * 1000 + 500);
        shot("inked");
    }
    // The match runs, then the results; the record appears when they start. Out of sight (SPLAT3_MATCH_REVEAL=never)
    // A every 4 s (a jump in the match) moves the results on; a shown game is the player's. The members' session
    // traffic is watched: its silence is logged (the game's own handling of a lost member is what decides).
    // A member whose session traffic stops (5 s) turns the room screen's hook on: the game back on its room (or its
    // lobby: the LobbyLocal pack loads again) without a record is a match the game ended early.
    uint64_t s = 0, next = 0, told = 0;
    int32_t r_watch = -1;
    end = until(T_MATCH);
    while (before(end) && !(s = find_record()) && LOAD(seq_lobby) == l0 &&
           (r_watch < 0 || LOAD(n_room_frames) == r_watch)) {
        if (!LOAD(shown) && plat_now_ms() >= next) press("A"), next = plat_now_ms() + 4000;
        uint64_t silent = 0;
        members_present(&silent);
        if (role == JOIN) silent = host_silent_ms();
        if (silent >= 5000 && silent / 5000 != told) {
            told = silent / 5000;
            rt_log("automatch: +%.1f s in the match, a member silent for %.1f s", now_s(), silent / 1000.0);
            if (r_watch < 0) {
                r_watch = LOAD(n_room_frames);
                fnprof_on(FNPROF_AUTO);
                mark("in the match, a member silent 5 s");
                shot("member-silent");
            } else if (told == 6) {
                shot("member-silent-30s");
            }
        }
        // the host stops only when no member is heard any more (others may play on after one left): its most recent
        // traffic from any member is that old (before SPLAT3_MATCH_SILENT it also waited for PEER_SILENT_MS, 20 s,
        // which kept a 1v1 host 5 s past a 15 s limit: measured)
        if ((role == JOIN ? silent : session_silent_ms()) >= silent_ms) {
            // measured: a guest whose host died at the stage's load stayed in its match,
            // no record, for 8 minutes; the game's own handling does not end it
            fail("connection-lost", "a member's session traffic stopped for %u s in the match; the game did not end it",
                 silent_ms / 1000);
            return 0;
        }
        sleep_ms(100);
    }
    if (!s && (LOAD(seq_lobby) != l0 || (r_watch >= 0 && LOAD(n_room_frames) != r_watch))) {
        // back in the lobby or the room: the record, if any, is there by now
        mark(LOAD(seq_lobby) != l0 ? "lobby loading again" : "the room screen is up again");
        uint64_t e2 = plat_now_ms() + 10000;
        while (!(s = find_record()) && before(e2)) sleep_ms(100);
        if (!s) {   // the game ended the match early: the result says aborted
            int ab = last_abandoned();
            char why[200];
            snprintf(why, sizeof why, "the game went back to its lobby without a match result (a player left or the "
                                      "connection was lost; the game's last-match-abandoned flag: %d)", ab);
            write_result(0, "aborted", why);
            fail("match-aborted", "%s", why);
            return 0;
        }
    }
    if (!s) {
        fail("record-timeout", "no match result within %d s of the intro (step match)", t_s[T_MATCH]);
        return 0;
    }
    mark("match record");
    seat_game = record_seat(s);
    if (seat_rules && seat_game != seat_rules)
        rt_log("automatch: the record seats this player as a %s, the rules as a %s", seat_word(seat_game), seat_word(seat_rules));
    {
        uint64_t r = s + REC_OFF, pb = rd64(r + 0x5e8);
        uint32_t np = rd32(r + 0x5f8), pcap = rd32(r + 0x5f0), pst = rd32(r + 0x5f4);
        rt_log("automatch: the record: %u teams, %u players, MyPersonalIndex %d", rd32(r + 0xb8), np, (int32_t)rd32(r + 0x7900));
        for (uint32_t i = 0; i < np && pcap && i < 8; i++) {
            uint64_t p = pb + (uint64_t)((pst + i) % pcap) * PERSON_SIZE;
            char name[96];
            json_str(name, sizeof name, rd64(p + 0xe0));
            rt_log("automatch:   player %u: %s, team %d, player index %d", i, name, (int32_t)rd32(p + 0x150),
                   (int32_t)rd32(p + 0xe50));
        }
    }
    shot("result");
    write_result(s, "completed", NULL);
    if (result_path[0]) {
        char dump[600];
        snprintf(dump, sizeof dump, "%s.bin", result_path);
        dump_state(s, dump);
    }
    status("results", "the result screens; the result is in %s", result_path[0] ? result_path : "(no file)");
    // The result screens are the player's: without input from the player for results_hold s (out of sight: at once)
    // the runtime presses A, which moves them on, until the game is back on its room screen (its update runs again:
    // the game reloads no scene pack for it) or the step's time is up; the other players read their record at the
    // same moment, so the game stays 8 s at least
    uint64_t min_end = plat_now_ms() + 8000, idle_from = plat_now_ms(), advancing = 0;
    end = until(T_RESULTS);
    int32_t r0 = LOAD(n_room_frames);
    fnprof_on(FNPROF_AUTO);   // the room screen's update (automatch_fn_hit)
    next = 0;
    // A guest has no such signal: the room screen's update also runs in its lobby (measured 53 a second there), and a
    // guest left on its result screens (its screen after the round: the last match's board) does not take the next
    // pre-match confirmation; a guest presses A for 20 s once advancing, which closes them (GUEST_RESULTS_MS)
    int guest = role == JOIN;
    while (before(end) && (before(min_end) || (guest ? !advancing || plat_now_ms() - advancing < GUEST_RESULTS_MS
                                                   : LOAD(n_room_frames) == r0))) {
        uint64_t now = plat_now_ms(), h = input_human_last_ms();
        if (h > idle_from) idle_from = h;   // the player is at the controls
        int idle = now - idle_from >= (uint64_t)results_hold * 1000;
        if ((!LOAD(shown) || idle) && now >= next && (guest || LOAD(n_room_frames) == r0)) {
            if (!advancing) advancing = now, mark("results: no input, advancing");
            press("A"), next = now + (LOAD(shown) ? 2000 : 4000);
        }
        sleep_ms(50);
    }
    mark(guest && advancing ? "results over: pressed through" : LOAD(n_room_frames) != r0 ? "results over: the room screen is up" : "results over: time up");
    return 1;
}

static void close_game(const char *why) {
    hide();
    status("finished", "%s", why);
    sleep_ms(1500);
    rt_log("automatch: closing the game (%s)", why);
    gfx_window_request_close();
}

// ---- the flow
static void run(void *arg) {
    (void)arg;
    rt_log("automatch: %s, match %s, %d players%s%s", role == HOST ? "host" : "join", match_id, players,
           role == HOST && teams[0] ? ", teams " : "", role == HOST ? teams : "");
    if (!setup()) return;
    for (;;) {
        if (!room_round() || !match_round()) return;
        if (want_exit) {
            close_game("closing the game");
            return;
        }
        if (!cmd_path[0]) {
            status("finished", "the match is over; the game stays open");
            return;
        }
        hide();   // between rounds the room is not shown: the next match reveals the game again
        if (!wait_command()) {
            close_game("the launcher's quit command");
            return;
        }
        uint64_t t_cmd = plat_now_ms();
        round_no++;
        round_reset();
        rt_log("automatch: round %d, match %s: mode %s, stage %s", round_no, match_id, mode_keys[want_rule],
               want_stage_scene);
        status("in-room", "round %d: back in the room", round_no);
        fnprof_on(FNPROF_AUTO);
        if (role == HOST && !host_back_to_room(until(T_LIST))) {
            fail("room-timeout", "the room screen did not come up again within %d s (step list)", t_s[T_LIST]);
            return;
        }
        if (role == JOIN) {   // measured: after a match a guest stands in the lobby, where A does not confirm the
            // pre-match screen (the game started the match only at its own countdown's end, 90 s); L and A open the
            // room as at the first round. The room screen's update counts are logged around it.
            int32_t f0 = LOAD(n_room_frames);
            sleep_ms(1000);
            int32_t f1 = LOAD(n_room_frames);
            press("L");
            sleep_ms(1500);
            press("A");
            sleep_ms(2000);
            int32_t f2 = LOAD(n_room_frames);
            sleep_ms(1000);
            rt_log("automatch: guest room screen updates: %d in the second before L, A; %d in the second after",
                   f1 - f0, LOAD(n_room_frames) - f2);
            shot("room-again");
        }
        command_ms = t_cmd;
    }
}

// ---- settings
static void copy_setting(char *out, size_t cap, const char *name) {
    const char *v = cfg_get(name);
    if (v) snprintf(out, cap, "%s", v);
}

// A setting that makes the flow impossible: the error in the files, then the process ends before the window opens
// (or, with SPLAT3_MATCH_ON_ERROR=show, the game starts as without SPLAT3_MATCH)
static void bad_settings(const char *fmt, ...) {
    char why[300];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(why, sizeof why, fmt, ap);
    va_end(ap);
    rt_log("automatch: %s; off", why);
    status_write("error", "bad-settings", 0, 0, why);
    if (!error_show) {
        rt_log("automatch: exiting (exit code 3)");
        fs_match_end();   // before the game's first storage use there is no copy yet: nothing to do
        plat_exit(3);
    }
}

static void parse_timeouts(const char *v) {
    for (const char *p = v; p && *p;) {
        size_t n = strcspn(p, ",");
        char item[48];
        snprintf(item, sizeof item, "%.*s", (int)(n < 47 ? n : 47), p);
        char *eq = strchr(item, '=');
        int i = 0;
        if (eq) {
            *eq = 0;
            while (i < T_N && strcmp(item, t_name[i])) i++;
        }
        if (eq && i < T_N && atoi(eq + 1) > 0) t_s[i] = atoi(eq + 1);
        else if (n) rt_log("automatch: SPLAT3_MATCH_TIMEOUTS item \"%.*s\" unknown; ignored", (int)n, p);
        p += n + (p[n] == ',');
    }
}

// The RomFS stage table (RSDB/VersusSceneInfo): a scene's Id (scene "Vss_..."), or the scene of an Id; -1 / 0 if none
static int stage_by_scene(const char *scene) {
    RsdbTable *t = rsdb_open("VersusSceneInfo");
    int id = -1;
    for (uint32_t r = 0; t && r < rsdb_rows(t) && id < 0; r++)
        if (!strcmp(rsdb_str(t, r, "__RowId"), scene)) id = (int)rsdb_int(t, r, "Id", -1);
    rsdb_close(t);
    return id;
}

static int stage_scene(int id, char *out, size_t cap) {
    RsdbTable *t = rsdb_open("VersusSceneInfo");
    int ok = 0;
    for (uint32_t r = 0; t && r < rsdb_rows(t) && !ok; r++)
        if (rsdb_int(t, r, "Id", -1) == id) ok = 1, snprintf(out, cap, "%s", rsdb_str(t, r, "__RowId"));
    rsdb_close(t);
    return ok;
}

// SPLAT3_MATCH_RULES=players=<2-8>,teams=<A|B|- per player in arrival order | name:A|B|-,...>,mode=<...>,stage=<...>,
// <other setting>=<0|1>, results_hold=<s>. A value that cannot work goes to rules_error (bad-settings).
static void parse_rules(const char *v) {
    int in_teams = 0;
    for (const char *p = v; p && *p;) {
        size_t n = strcspn(p, ",");
        char item[48];
        snprintf(item, sizeof item, "%.*s", (int)(n < 47 ? n : 47), p);
        char *colon = strrchr(item, ':');
        const char *t = !strncmp(item, "teams=", 6) ? item + 6 : in_teams && !strchr(item, '=') ? item : NULL;
        if (!strncmp(item, "teams=", 6)) nby_name = 0, teams[0] = 0;   // a new teams list replaces the last one
        // teams by name: <name>:<one letter>. colon[1] first: past the item's NUL is the buffer's end (47 bytes)
        if (t && colon && colon > t && colon[1] && !colon[2] && nby_name < 10) {
            in_teams = 1;
            snprintf(by_name[nby_name].name, sizeof by_name[0].name, "%.*s", (int)(colon - t), t);
            by_name[nby_name++].team = colon[1];
        } else if (!strncmp(item, "teams=", 6)) {
            in_teams = 0;
            snprintf(teams, sizeof teams, "%s", item + 6);
        } else if (!strncmp(item, "players=", 8)) {
            in_teams = 0;
            // everyone in the session, the spectators included: 8 players and 2 spectators at most (teams_check)
            players = atoi(item + 8) < 2 ? 2 : atoi(item + 8) > 10 ? 10 : atoi(item + 8);
        } else if (!strncmp(item, "mode=", 5)) {
            in_teams = 0;
            int m = -1;
            for (int i = 0; i < MODES; i++)   // a neutral key, or the game's own rule name (rule_names_read)
                if (!strcmp(item + 5, mode_keys[i]) || (i < nrule_names && !strcmp(item + 5, rule_names[i]))) m = i;
            if (m < 0) snprintf(rules_error, sizeof rules_error, "mode %s unknown (turf, zones, tower, rainmaker, clams)", item + 5);
            else want_rule = m;
        } else if (!strncmp(item, "stage=", 6)) {
            in_teams = 0;
            const char *v = item + 6;
            char *end;
            long id = strtol(v, &end, 10);
            if (!strcmp(v, "random")) want_stage = -1;
            else if (*v && !*end && id >= 0) want_stage = (int)id;
            else if ((id = stage_by_scene(v)) >= 0) want_stage = (int)id;
            else snprintf(rules_error, sizeof rules_error, "stage %s unknown (random, a VersusSceneInfo Id, a Vss_ scene)", v);
        } else if (!strncmp(item, "results_hold=", 13)) {
            in_teams = 0;
            char *end;
            long h = strtol(item + 13, &end, 10);
            if (item[13] && !*end && h >= 0 && h <= 600) results_hold = (int)h;
            else snprintf(rules_error, sizeof rules_error, "results_hold=%s is not 0-600 seconds", item + 13);
        } else if (!strncmp(item, "spectators_wait=", 16)) {
            in_teams = 0;
            char *end;
            long w = strtol(item + 16, &end, 10);
            if (item[16] && !*end && w >= 0 && w <= 600) spectators_wait = (int)w;
            else snprintf(rules_error, sizeof rules_error, "spectators_wait=%s is not 0-600 seconds", item + 16);
        } else if (strchr(item, '=') && strlen(item) > 2) {
            in_teams = 0;
            char *eq = strchr(item, '=');
            *eq = 0;
            int i = 0;
            while (i < OPTS && strcmp(item, opt_keys[i]) && (!opt_alias[i][0] || strcmp(item, opt_alias[i]))) i++;
            if (i < OPTS && (!strcmp(eq + 1, "0") || !strcmp(eq + 1, "1"))) want_opt[i] = eq[1] - '0';
            else snprintf(rules_error, sizeof rules_error, "SPLAT3_MATCH_RULES item %s=%s unknown", item, eq + 1);
        } else if (n) {
            rt_log("automatch: SPLAT3_MATCH_RULES item \"%s\" unknown; ignored", item);
        }
        p += n + (p[n] == ',');
    }
}

void automatch_init(void) {
    t0_ms = plat_now_ms();
    t0_unix = wall_now();
    if (rt_target.family != RT_GAME_SPLATOON3 || !ldr_count()) return;
    const char *m = cfg_get("SPLAT3_MATCH");
    if (!m || !*m || !strcmp(m, "off")) return;
    role = !strcmp(m, "host") ? HOST : !strcmp(m, "join") ? JOIN : 0;
    copy_setting(result_path, sizeof result_path, "SPLAT3_MATCH_RESULT");
    copy_setting(status_path, sizeof status_path, "SPLAT3_MATCH_STATUS");
    const char *v = cfg_get("SPLAT3_MATCH_ON_ERROR");
    error_show = v && !strcmp(v, "show");
    const char *id = cfg_get("SPLAT3_MATCH_ID");
    if (id && *id) {
        size_t n = strspn(id, "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_");
        if (n != strlen(id) || n > 64) {
            bad_settings("SPLAT3_MATCH_ID must be 1-64 of A-Z a-z 0-9 - _");
            return;
        }
        snprintf(match_id, sizeof match_id, "%s", id);
    }
    snprintf(result_tmpl, sizeof result_tmpl, "%s", result_path);   // "{id}" in a path: the round's match id
    snprintf(status_tmpl, sizeof status_tmpl, "%s", status_path);
    round_paths();
    copy_setting(cmd_path, sizeof cmd_path, "SPLAT3_MATCH_CMD");
    if (cmd_path[0]) {   // only the lines appended from now on are commands
        uint64_t n = 0;
        uint8_t *b = plat_file_read(cmd_path, &n);
        cmd_taken = b ? n : 0;
        free(b);
    }
    if (!role) { bad_settings("SPLAT3_MATCH=%s unknown (host, join)", m); return; }
    if (lan_cfg()->mode == LAN_OFF) { bad_settings("SPLAT3_MATCH needs LAN play (SPLAT3_LAN)"); return; }
    const char *st = scene_start_value();
    if (!st || strcmp(st, "lobby-lan")) { bad_settings("SPLAT3_MATCH needs SPLAT3_START=lobby-lan"); return; }
    v = cfg_get("SPLAT3_MATCH_REVEAL");
    if (v && *v) {
        if (!strcmp(v, "intro")) reveal_mode = REVEAL_INTRO;
        else if (!strcmp(v, "loading")) reveal_mode = REVEAL_LOADING;
        else if (!strcmp(v, "never")) reveal_mode = REVEAL_NEVER;
        else { bad_settings("SPLAT3_MATCH_REVEAL=%s unknown (intro, loading, never)", v); return; }
    }
    v = cfg_get("SPLAT3_MATCH_STAGES");   // fs_host.c applies it to the match's copy of the save
    if (v && *v && strcmp(v, "all") && strcmp(v, "save")) { bad_settings("SPLAT3_MATCH_STAGES=%s unknown (all, save)", v); return; }
    v = cfg_get("SPLAT3_MATCH_LOADOUT");
    if (v && *v && !plat_file_exists(v)) { bad_settings("SPLAT3_MATCH_LOADOUT: %s is not a file", v); return; }
    s3 = splat3_addr();
    if (!s3) { bad_settings("SPLAT3_MATCH: no main offsets for target %s", rt_target.name); return; }
    main_base = ldr_module(0)->base;
    copy_setting(shots_dir, sizeof shots_dir, "SPLAT3_MATCH_SHOTS");
    copy_setting(host_name, sizeof host_name, "SPLAT3_MATCH_HOST");
    copy_setting(self_name, sizeof self_name, "SPLAT3_LAN_NAME");
    copy_setting(self_name, sizeof self_name, "SPLAT3_NICKNAME");
    v = cfg_get("SPLAT3_MATCH_EXIT");
    want_exit = v && !strcmp(v, "1");
    v = cfg_get("SPLAT3_MATCH_PLAY");
    if (v) play_s = atoi(v) < 0 ? 0 : atoi(v) > 120 ? 120 : atoi(v);
    v = cfg_get("SPLAT3_MATCH_DIAG");
    diag = v && !strcmp(v, "1");
    v = cfg_get("SPLAT3_MATCH_BENCH");
    if (v && *v && strcmp(v, "teams")) { bad_settings("SPLAT3_MATCH_BENCH=%s unknown (teams)", v); return; }
    bench_teams = v && !strcmp(v, "teams");
    v = cfg_get("SPLAT3_MATCH_SILENT");
    if (v && *v) {
        if (atoi(v) < 5 || atoi(v) > 600) { bad_settings("SPLAT3_MATCH_SILENT=%s out of range (5-600 s)", v); return; }
        silent_set_ms = (uint32_t)atoi(v) * 1000;
    }
    v = cfg_get("SPLAT3_MATCH_WAIT");   // the older setting: the host's wait for the players and a guest's for a session
    if (v && atoi(v) > 0) t_s[T_PLAYERS] = t_s[T_JOIN] = atoi(v);
    parse_timeouts(cfg_get("SPLAT3_MATCH_TIMEOUTS"));
    rule_names_read();
    parse_rules(cfg_get("SPLAT3_MATCH_RULES"));
    if (rules_error[0]) { bad_settings("SPLAT3_MATCH_RULES: %s", rules_error); return; }
    char team_why[160];
    if (!teams_check(team_why, sizeof team_why)) { bad_settings("SPLAT3_MATCH_RULES: %s", team_why); return; }
    seat_rules = rules_seat();
    if (want_stage >= 0 && !stage_scene(want_stage, want_stage_scene, sizeof want_stage_scene)) {
        bad_settings("SPLAT3_MATCH_RULES: stage %d is not in the game's stage table (RSDB/VersusSceneInfo)", want_stage);
        return;
    }
    const LanCfg *lc = lan_cfg();
    if (lc->mode == LAN_RELAY) {   // one relay room per match keeps its players apart: the room is the match id
        char want[33];
        size_t k = 0;
        for (; match_id[k] && k < 32; k++) want[k] = match_id[k] >= 'a' && match_id[k] <= 'z' ? match_id[k] - 32 : match_id[k];
        want[k] = 0;
        if (strcmp(want, lc->room))
            rt_log("automatch: relay room %s is not the match id (%s): other matches' hosts may be listed", lc->room, want);
    }
    if (nby_name && lc->mode != LAN_RELAY) rt_log("automatch: teams by name need relay mode (the relay names the members)");
    lan_set_observer(on_net);
    automatch_on = 1;
    input_human(0);
    audio_mute(1);
    status("booting", "the game is starting");
    if (!plat_thread_spawn(run, NULL, "automatch", 0)) {
        automatch_on = 0;
        lan_set_observer(NULL);
        input_human(1);
        audio_mute(0);
        bad_settings("cannot start the automatic flow's thread");
        return;
    }
    plat_thread_spawn(frames_main, NULL, "automatch-frames", 0);
    char tl[256];
    int k = 0;
    for (int i = 0; i < T_N; i++) k += snprintf(tl + k, sizeof tl - k, "%s%s %d", i ? ", " : "", t_name[i], t_s[i]);
    rt_log("automatch: on (%s, match %s, mode %s (game rule %d %s), stage %s, reveal %s, on error %s, result %s, status %s%s); "
           "time limits (s): %s", m, match_id, mode_keys[want_rule], want_rule, rule_name(want_rule), want_stage_scene,
           reveal_mode == REVEAL_INTRO ? "intro" : reveal_mode == REVEAL_LOADING ? "loading" : "never",
           error_show ? "show" : "exit", result_path[0] ? result_path : "(none)", status_path[0] ? status_path : "(none)",
           want_exit ? ", exit after the match" : "", tl);
}
