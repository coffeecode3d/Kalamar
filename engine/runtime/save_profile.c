// Launcher profile for Splatoon 3, save_profile (TOTK_SAVE_PROFILE=<profile.json>): the player's appearance,
// splashtag, gear, weapon and win emote written into the save before the game reads it, and the first-launch flow
// (character creation, tutorial) marked done, so the game boots to the plaza with the launcher's character. The same
// edit as `s3save.py profile` (work/tools/profile, outside the repository), whose header documents the format.
//
// The save format, measured with TOTK_SAVE_TRACE (savetrace.c):
//   save.dat = seed u32le | check u32le | AES-256-CTR(plaintext)
//   key, counter = the top byte of each of the first 48 outputs of sead::Random(T[sead::Random(seed).getU32() >> 27]),
//                  T a table of 32 words in main (found here by the code that indexes it, never stored)
//   check = MurmurHash3_x86_32(ciphertext, seed) + the first output of that second generator
//   plaintext = "MS" 00 04 | game version u32be | MessagePack map (game data) | MessagePack map (player data); field
//               names are keys hashed with MurmurHash3_x86_32 (seed 0), written as uint32; item ids are int keys.
// The option ids are checked against the game's RomFS tables (RSDB/*.rstbl.byml.zs) and owned items are recorded the
// way the game recorded the starter items of a new save.
//
// With a save: the profile is applied at the first use of the save storage (fs_host.c storage_init), after a copy of
// the save into <profile>/save-profile-backup/<time>/. Without one the game creates a new save at its first boot and
// goes on into the first-launch flow: the profile is applied to that save once the game commits it, and the game
// restarts (gfx_window_request_restart) so it loads it.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <zstd.h>

#include "cfg.h"
#include "fs.h"
#include "host.h"
#include "save_profile.h"
#include "splat3_addr.h"
#include "platform/platform.h"
#include "ui/menu.h"

#define MASK32 0xffffffffu

// ---- JSON (the profile): objects, arrays, strings, numbers, true/false/null
enum { J_NULL, J_BOOL, J_NUM, J_STR, J_ARR, J_OBJ };
typedef struct Json Json;
struct Json {
    int kind;
    double num;
    char *str;        // J_STR
    char *key;        // a member of an object
    Json *kids;       // J_ARR, J_OBJ
    int n;
};

static const char *jp;

static void jws(void) {
    while (*jp == ' ' || *jp == '\t' || *jp == '\n' || *jp == '\r') jp++;
}

static int json_parse(Json *out);

static char *json_string(void) {
    if (*jp != '"') return NULL;
    jp++;
    size_t cap = 64, n = 0;
    char *s = malloc(cap);
    while (s && *jp && *jp != '"') {
        unsigned c = (unsigned char)*jp++;
        if (c == '\\') {
            char e = *jp++;
            if (e == 'u') {
                unsigned v = 0;
                for (int i = 0; i < 4; i++) {
                    char h = *jp++;
                    v = v * 16 + (h >= '0' && h <= '9' ? h - '0' : (h | 32) >= 'a' && (h | 32) <= 'f' ? (h | 32) - 'a' + 10 : 0);
                }
                char u[3];
                int k = 0;
                if (v < 0x80) u[k++] = (char)v;
                else if (v < 0x800) u[k++] = (char)(0xc0 | v >> 6), u[k++] = (char)(0x80 | (v & 63));
                else u[k++] = (char)(0xe0 | v >> 12), u[k++] = (char)(0x80 | ((v >> 6) & 63)), u[k++] = (char)(0x80 | (v & 63));
                for (int i = 0; i < k; i++) {
                    if (n + 2 > cap) s = realloc(s, cap *= 2);
                    if (s) s[n++] = u[i];
                }
                continue;
            }
            c = e == 'n' ? '\n' : e == 't' ? '\t' : e == 'r' ? '\r' : e == 'b' ? '\b' : e == 'f' ? '\f' : (unsigned char)e;
        }
        if (n + 2 > cap) s = realloc(s, cap *= 2);
        if (s) s[n++] = (char)c;
    }
    if (!s || *jp != '"') {
        free(s);
        return NULL;
    }
    jp++;
    s[n] = 0;
    return s;
}

static int json_list(Json *out, char close, int obj) {
    jp++;
    out->kind = obj ? J_OBJ : J_ARR;
    jws();
    if (*jp == close) {
        jp++;
        return 1;
    }
    for (;;) {
        Json *k = realloc(out->kids, (size_t)(out->n + 1) * sizeof *k);
        if (!k) return 0;
        out->kids = k;
        Json *e = &out->kids[out->n++];
        memset(e, 0, sizeof *e);
        jws();
        char *key = NULL;
        if (obj) {
            key = json_string();
            jws();
            if (!key || *jp != ':') {
                free(key);
                return 0;
            }
            jp++;
        }
        if (!json_parse(e)) {
            free(key);
            return 0;
        }
        e->key = key;
        jws();
        if (*jp == ',') {
            jp++;
            continue;
        }
        if (*jp == close) {
            jp++;
            return 1;
        }
        return 0;
    }
}

static int json_parse(Json *out) {
    jws();
    memset(out, 0, sizeof *out);
    if (*jp == '{') return json_list(out, '}', 1);
    if (*jp == '[') return json_list(out, ']', 0);
    if (*jp == '"') return (out->kind = J_STR, out->str = json_string()) != NULL;
    if (!strncmp(jp, "true", 4)) return jp += 4, out->kind = J_BOOL, out->num = 1, 1;
    if (!strncmp(jp, "false", 5)) return jp += 5, out->kind = J_BOOL, 1;
    if (!strncmp(jp, "null", 4)) return jp += 4, out->kind = J_NULL, 1;
    char *end;
    out->num = strtod(jp, &end);
    if (end == jp) return 0;
    jp = end;
    out->kind = J_NUM;
    return 1;
}

static void json_free(Json *j) {
    for (int i = 0; i < j->n; i++) {
        json_free(&j->kids[i]);
        free(j->kids[i].key);
    }
    free(j->kids);
    free(j->str);
    memset(j, 0, sizeof *j);
}

// The member key of an object, or NULL.
static const Json *jget(const Json *o, const char *key) {
    if (!o || o->kind != J_OBJ) return NULL;
    for (int i = 0; i < o->n; i++)
        if (o->kids[i].key && !strcmp(o->kids[i].key, key)) return &o->kids[i];
    return NULL;
}

// An integer member: 1 and *v when present and whole, 0 when absent; -1 when present but not an integer.
static int jint(const Json *o, const char *key, int64_t *v) {
    const Json *j = jget(o, key);
    if (!j || j->kind == J_NULL) return 0;
    if (j->kind != J_NUM || j->num != (double)(int64_t)j->num) return -1;
    *v = (int64_t)j->num;
    return 1;
}

// ---- the save's crypto
static uint32_t murmur3(const uint8_t *d, size_t n, uint32_t seed) {
    const uint32_t c1 = 0xcc9e2d51, c2 = 0x1b873593;
    uint32_t h = seed, k;
    size_t i = 0;
    for (; i + 4 <= n; i += 4) {
        k = (uint32_t)d[i] | (uint32_t)d[i + 1] << 8 | (uint32_t)d[i + 2] << 16 | (uint32_t)d[i + 3] << 24;
        k *= c1, k = k << 15 | k >> 17, k *= c2;
        h ^= k, h = h << 13 | h >> 19, h = h * 5 + 0xe6546b64;
    }
    k = 0;
    for (size_t j = n - i; j-- > 0;) k = k << 8 | d[i + j];
    if (n - i) k *= c1, k = k << 15 | k >> 17, k *= c2, h ^= k;
    h ^= (uint32_t)n;
    h ^= h >> 16, h *= 0x85ebca6b, h ^= h >> 13, h *= 0xc2b2ae35, h ^= h >> 16;
    return h;
}

static uint32_t name_hash(const char *s) { return murmur3((const uint8_t *)s, strlen(s), 0); }

typedef struct { uint32_t x[4]; } SeadRandom;

static void sead_init(SeadRandom *r, uint32_t seed) {
    uint32_t v = seed;
    for (uint32_t i = 0; i < 4; i++) r->x[i] = v = 0x6c078965u * (v ^ v >> 30) + i + 1;
}

static uint32_t sead_u32(SeadRandom *r) {
    uint32_t t = r->x[0] ^ r->x[0] << 11, w = r->x[3];
    r->x[0] = r->x[1], r->x[1] = r->x[2], r->x[2] = w;
    return r->x[3] = w ^ w >> 19 ^ t ^ t >> 8;
}

static uint32_t key_table[32];

// key[32], counter[16]; returns the second generator's first output (the checksum's addend).
static uint32_t derive(uint32_t seed, uint8_t key[32], uint8_t ctr[16]) {
    SeadRandom a, b;
    sead_init(&a, seed);
    sead_init(&b, key_table[sead_u32(&a) >> 27]);
    uint32_t first = 0;
    for (int i = 0; i < 48; i++) {
        uint32_t v = sead_u32(&b);
        if (!i) first = v;
        (i < 32 ? key : ctr - 32)[i] = (uint8_t)(v >> 24);
    }
    return first;
}

// AES-256, forward cipher only (CTR), FIPS-197.
static uint8_t sbox[256];

static uint8_t xt(uint8_t a) { return (uint8_t)(a << 1 ^ (a & 0x80 ? 0x1b : 0)); }

static uint8_t gmul(uint8_t a, uint8_t b) {
    uint8_t r = 0;
    for (; b; b >>= 1, a = xt(a))
        if (b & 1) r ^= a;
    return r;
}

// The S-box: the inverse in GF(2^8), then the affine map.
static void sbox_init(void) {
    if (sbox[0]) return;
    for (int a = 0; a < 256; a++) {
        uint8_t inv = 0;
        for (int b = 1; a && b < 256 && !inv; b++)
            if (gmul((uint8_t)a, (uint8_t)b) == 1) inv = (uint8_t)b;
        uint8_t x = inv ^ (uint8_t)(inv << 1 | inv >> 7) ^ (uint8_t)(inv << 2 | inv >> 6) ^
                    (uint8_t)(inv << 3 | inv >> 5) ^ (uint8_t)(inv << 4 | inv >> 4);
        sbox[a] = x ^ 0x63;
    }
}

typedef struct { uint8_t rk[15][16]; } Aes256;

static void aes_init(Aes256 *a, const uint8_t key[32]) {
    sbox_init();
    uint8_t w[60][4];
    memcpy(w, key, 32);
    uint8_t rcon = 1;
    for (int i = 8; i < 60; i++) {
        uint8_t t[4];
        memcpy(t, w[i - 1], 4);
        if (i % 8 == 0) {
            uint8_t t0 = t[0];
            t[0] = sbox[t[1]] ^ rcon, t[1] = sbox[t[2]], t[2] = sbox[t[3]], t[3] = sbox[t0];
            rcon = xt(rcon);
        } else if (i % 8 == 4) {
            for (int k = 0; k < 4; k++) t[k] = sbox[t[k]];
        }
        for (int k = 0; k < 4; k++) w[i][k] = w[i - 8][k] ^ t[k];
    }
    memcpy(a->rk, w, sizeof a->rk);
}

static void aes_block(const Aes256 *a, const uint8_t in[16], uint8_t out[16]) {
    uint8_t s[16], t[16];
    for (int i = 0; i < 16; i++) s[i] = in[i] ^ a->rk[0][i];
    for (int r = 1; r < 15; r++) {
        for (int i = 0; i < 16; i++) t[i] = sbox[s[(i + 4 * (i % 4)) % 16]];   // SubBytes, ShiftRows
        if (r < 14)
            for (int c = 0; c < 4; c++) {   // MixColumns
                uint8_t *x = t + 4 * c, all = x[0] ^ x[1] ^ x[2] ^ x[3], x0 = x[0];
                for (int i = 0; i < 4; i++) x[i] ^= all ^ xt(x[i] ^ (i < 3 ? x[i + 1] : x0));
            }
        for (int i = 0; i < 16; i++) s[i] = t[i] ^ a->rk[r][i];
    }
    memcpy(out, s, 16);
}

static void aes_ctr(const uint8_t key[32], const uint8_t ctr0[16], uint8_t *d, size_t n) {
    Aes256 a;
    aes_init(&a, key);
    uint8_t ctr[16], ks[16];
    memcpy(ctr, ctr0, 16);
    for (size_t i = 0; i < n; i += 16) {
        aes_block(&a, ctr, ks);
        for (size_t k = 0; k < 16 && i + k < n; k++) d[i + k] ^= ks[k];
        for (int k = 15; k >= 0 && !++ctr[k]; k--) {
        }
    }
}

// The table: the code that indexes it, `lsr wN, wN, #27` then `ldr wT, [xB, wN, uxtw #2]`, with xB from adrp + add
// and a sead::Random initialiser (`movk wX, #0x6c07, lsl #16`) within 24 instructions before. 1 when one table is found.
static int find_key_table(void) {
    const RtLoaded *m = ldr_module(0);
    if (!m) return 0;
    const uint32_t *t = (const uint32_t *)(uintptr_t)m->base;
    uint64_t n = m->text_end / 4, found = 0;
    int hits = 0;
    for (uint64_t i = 24; i + 1 < n; i++) {
        uint32_t lsr = t[i], ldr = t[i + 1];
        if ((lsr & 0xfffffc00u) != 0x531b7c00u || (lsr & 31) != (lsr >> 5 & 31)) continue;
        if ((ldr & 0xffe0fc00u) != 0xb8605800u || (ldr >> 16 & 31) != (lsr & 31)) continue;
        uint32_t base = ldr >> 5 & 31;
        int movk = 0;
        uint64_t lo = UINT64_MAX, page = 0;
        for (uint64_t k = i; k-- > i - 24;) {
            uint32_t w = t[k];
            if ((w & 0xffffffe0u) == 0x72ad80e0u) movk = 1;
            if (lo == UINT64_MAX && (w & 0xffc00000u) == 0x91000000u && (w & 31) == base && (w >> 5 & 31) == base)
                lo = w >> 10 & 0xfff;
            else if (lo != UINT64_MAX && !page && (w & 0x9f00001fu) == (0x90000000u | base)) {
                int64_t imm = (int64_t)((w >> 29 & 3) | (w >> 5 & 0x7ffff) << 2);
                if (imm & (1 << 20)) imm -= 1 << 21;
                page = ((m->base + 4 * k) & ~0xfffull) + (uint64_t)(imm << 12);
            }
        }
        if (!movk || !page) continue;
        if (hits && found != page + lo) return 0;
        found = page + lo, hits++;
    }
    if (!hits || found < m->base || found + sizeof key_table > m->base + m->size) return 0;
    memcpy(key_table, (const void *)(uintptr_t)found, sizeof key_table);
    return 1;
}

// ---- MessagePack, each value keeping the type byte it was read with, so unchanged data encodes to the same bytes
enum { M_INT, M_NIL, M_BOOL, M_F32, M_F64, M_STR, M_BIN, M_ARR, M_MAP };
typedef struct Mp Mp;
struct Mp {
    uint8_t kind, fmt;   // fmt 0: new value, smallest encoding
    int64_t i;           // M_INT, M_BOOL; uint64 0xcf values as their bit pattern
    double f;
    uint8_t *s;          // M_STR, M_BIN (owned)
    uint32_t n;          // bytes, elements, or pairs of a map
    Mp *kids;            // M_ARR: n values; M_MAP: 2n (key, value)
};

static void mp_free(Mp *m) {
    if (m->kind == M_ARR || m->kind == M_MAP)
        for (uint32_t k = 0; k < (m->kind == M_MAP ? 2 * m->n : m->n); k++) mp_free(&m->kids[k]);
    free(m->kids);
    free(m->s);
    memset(m, 0, sizeof *m);
}

static uint64_t be(const uint8_t *p, int n) {
    uint64_t v = 0;
    for (int i = 0; i < n; i++) v = v << 8 | p[i];
    return v;
}

static uint32_t le32(const uint8_t *p) {
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}

// One value at b[*i]; 0 on malformed data.
static int mp_read(const uint8_t *b, size_t n, size_t *i, Mp *out, int depth) {
    memset(out, 0, sizeof *out);
    if (*i >= n || depth > 32) return 0;
    uint8_t t = b[(*i)++];
    out->fmt = t;
    size_t need = 0, len = 0;
    int count = -1, map = 0;
    if (t <= 0x7f || t >= 0xe0) return out->kind = M_INT, out->i = (int8_t)t, out->i = t <= 0x7f ? t : out->i, 1;
    if (t >= 0x80 && t <= 0x8f) count = t & 15, map = 1;
    else if (t >= 0x90 && t <= 0x9f) count = t & 15;
    else if (t == 0xde || t == 0xdc || t == 0xdf || t == 0xdd) {
        int w = t == 0xde || t == 0xdc ? 2 : 4;
        if (*i + w > n) return 0;
        count = (int)be(b + *i, w), *i += w, map = t == 0xde || t == 0xdf;
    }
    if (count >= 0) {
        out->kind = map ? M_MAP : M_ARR;
        out->n = (uint32_t)count;
        size_t k = (size_t)count * (map ? 2 : 1);
        if (k > n) return 0;
        out->kids = calloc(k ? k : 1, sizeof *out->kids);
        if (!out->kids) return 0;
        for (size_t j = 0; j < k; j++)
            if (!mp_read(b, n, i, &out->kids[j], depth + 1)) return 0;
        return 1;
    }
    if ((t >= 0xa0 && t <= 0xbf) || t == 0xd9 || t == 0xda || t == 0xdb || t == 0xc4 || t == 0xc5 || t == 0xc6) {
        out->kind = t == 0xc4 || t == 0xc5 || t == 0xc6 ? M_BIN : M_STR;
        int w = t <= 0xbf ? 0 : t == 0xd9 || t == 0xc4 ? 1 : t == 0xda || t == 0xc5 ? 2 : 4;
        if (*i + w > n) return 0;
        len = w ? (size_t)be(b + *i, w) : (size_t)(t & 31);
        *i += w;
        if (*i + len > n) return 0;
        out->s = malloc(len + 1);
        if (!out->s) return 0;
        memcpy(out->s, b + *i, len);
        out->s[len] = 0;
        out->n = (uint32_t)len;
        *i += len;
        return 1;
    }
    switch (t) {
    case 0xc0: out->kind = M_NIL; return 1;
    case 0xc2: case 0xc3: out->kind = M_BOOL, out->i = t == 0xc3; return 1;
    case 0xcc: need = 1; break;
    case 0xcd: need = 2; break;
    case 0xce: need = 4; break;
    case 0xcf: need = 8; break;
    case 0xd0: need = 1; break;
    case 0xd1: need = 2; break;
    case 0xd2: need = 4; break;
    case 0xd3: need = 8; break;
    case 0xca: need = 4; break;
    case 0xcb: need = 8; break;
    default: return 0;
    }
    if (*i + need > n) return 0;
    uint64_t v = be(b + *i, (int)need);
    *i += need;
    if (t == 0xca) {
        uint32_t u = (uint32_t)v;
        float f;
        memcpy(&f, &u, 4);
        return out->kind = M_F32, out->f = f, 1;
    }
    if (t == 0xcb) {
        double f;
        memcpy(&f, &v, 8);
        return out->kind = M_F64, out->f = f, 1;
    }
    out->kind = M_INT;
    if (t >= 0xd0) {   // signed: sign-extend
        int bits = (int)need * 8;
        out->i = bits == 64 ? (int64_t)v : (int64_t)(v << (64 - bits)) >> (64 - bits);
    } else {
        out->i = (int64_t)v;
    }
    return 1;
}

typedef struct { uint8_t *p; size_t n, cap; int bad; } Buf;

static void put(Buf *b, const void *d, size_t n) {
    if (b->n + n > b->cap) {
        size_t c = b->cap ? b->cap * 2 : 65536;
        while (c < b->n + n) c *= 2;
        uint8_t *q = realloc(b->p, c);
        if (!q) {
            b->bad = 1;
            return;
        }
        b->p = q, b->cap = c;
    }
    memcpy(b->p + b->n, d, n);
    b->n += n;
}

static void put_be(Buf *b, uint8_t t, uint64_t v, int w) {
    uint8_t x[9] = {t};
    for (int k = 0; k < w; k++) x[1 + k] = (uint8_t)(v >> (8 * (w - 1 - k)));
    put(b, x, (size_t)w + 1);
}

static void put_int(Buf *b, int64_t v, uint8_t fmt) {
    static const struct { uint8_t f; int w; int64_t lo, hi; } fx[] = {
        {0xcc, 1, 0, 0xff}, {0xcd, 2, 0, 0xffff}, {0xce, 4, 0, 0xffffffffll}, {0xcf, 8, 0, INT64_MAX},
        {0xd0, 1, -128, 127}, {0xd1, 2, -32768, 32767}, {0xd2, 4, INT32_MIN, INT32_MAX}, {0xd3, 8, INT64_MIN, INT64_MAX}};
    if (fmt == 0xcf) {
        put_be(b, 0xcf, (uint64_t)v, 8);   // the stored bit pattern
        return;
    }
    if (fmt && fmt <= 0x7f && v >= 0 && v <= 0x7f) {
        uint8_t x = (uint8_t)v;
        put(b, &x, 1);
        return;
    }
    if (fmt >= 0xe0 && v >= -32 && v < 0) {
        uint8_t x = (uint8_t)v;
        put(b, &x, 1);
        return;
    }
    for (size_t k = 0; k < sizeof fx / sizeof *fx; k++)
        if (fx[k].f == fmt && v >= fx[k].lo && v <= fx[k].hi) {
            put_be(b, fmt, (uint64_t)v, fx[k].w);
            return;
        }
    if (v >= -32 && v <= 0x7f) {
        uint8_t x = (uint8_t)v;
        put(b, &x, 1);
        return;
    }
    for (size_t k = v >= 0 ? 0 : 4; k < sizeof fx / sizeof *fx; k++)
        if (v >= fx[k].lo && v <= fx[k].hi) {
            put_be(b, fx[k].f, (uint64_t)v, fx[k].w);
            return;
        }
}

// The length prefix of a str/arr/map: the format it was read with when the length fits it, else the smallest.
static void put_len(Buf *b, uint32_t n, uint8_t small, uint32_t small_max, uint8_t f8, uint8_t f16, uint8_t f32,
                    uint8_t fmt) {
    if (fmt && fmt == f8 && n <= 0xff) put_be(b, f8, n, 1);
    else if (fmt == f16 && n <= 0xffff) put_be(b, f16, n, 2);
    else if (fmt == f32) put_be(b, f32, n, 4);
    else if (n <= small_max) put_be(b, (uint8_t)(small | n), 0, 0);
    else if (f8 && n <= 0xff) put_be(b, f8, n, 1);
    else if (n <= 0xffff) put_be(b, f16, n, 2);
    else put_be(b, f32, n, 4);
}

static void mp_write(Buf *b, const Mp *m) {
    switch (m->kind) {
    case M_INT: put_int(b, m->i, m->fmt); break;
    case M_NIL: put_be(b, 0xc0, 0, 0); break;
    case M_BOOL: put_be(b, m->i ? 0xc3 : 0xc2, 0, 0); break;
    case M_F32: {
        float f = (float)m->f;
        uint32_t u;
        memcpy(&u, &f, 4);
        put_be(b, 0xca, u, 4);
        break;
    }
    case M_F64: {
        uint64_t u;
        memcpy(&u, &m->f, 8);
        put_be(b, 0xcb, u, 8);
        break;
    }
    case M_STR: put_len(b, m->n, 0xa0, 31, 0xd9, 0xda, 0xdb, m->fmt), put(b, m->s, m->n); break;
    case M_BIN:   // no short form: bin8 unless it was read wider
        if (m->fmt == 0xc6 || m->n > 0xffff) put_be(b, 0xc6, m->n, 4);
        else if (m->fmt == 0xc5 || m->n > 0xff) put_be(b, 0xc5, m->n, 2);
        else put_be(b, 0xc4, m->n, 1);
        put(b, m->s, m->n);
        break;
    case M_ARR:
        put_len(b, m->n, 0x90, 15, 0, 0xdc, 0xdd, m->fmt);
        for (uint32_t k = 0; k < m->n; k++) mp_write(b, &m->kids[k]);
        break;
    case M_MAP:
        put_len(b, m->n, 0x80, 15, 0, 0xde, 0xdf, m->fmt);
        for (uint32_t k = 0; k < 2 * m->n; k++) mp_write(b, &m->kids[k]);
        break;
    }
}

static Mp *mp_get(Mp *map, int64_t key) {
    if (!map || map->kind != M_MAP) return NULL;
    for (uint32_t k = 0; k < map->n; k++)
        if (map->kids[2 * k].kind == M_INT && map->kids[2 * k].i == key) return &map->kids[2 * k + 1];
    return NULL;
}

// A path "0/Common/Coordinates/HairId": the root index, then field names (hashed).
typedef struct { uint8_t head[8]; Mp roots[4]; int nroots; } Plain;

static Mp *path(Plain *p, const char *pth) {
    int r = pth[0] - '0';
    if (r < 0 || r >= p->nroots || pth[1] != '/') return NULL;
    Mp *m = &p->roots[r];
    char name[64];
    for (const char *s = pth + 2; m && *s;) {
        size_t n = strcspn(s, "/");
        if (n >= sizeof name) return NULL;
        memcpy(name, s, n);
        name[n] = 0;
        m = mp_get(m, name_hash(name));
        s += n + (s[n] == '/');
    }
    return m;
}

static void mp_clone(Mp *out, const Mp *in) {
    *out = *in;
    out->s = NULL, out->kids = NULL;
    if (in->s) {
        out->s = malloc((size_t)in->n + 1);
        if (out->s) memcpy(out->s, in->s, (size_t)in->n + 1);
    }
    uint32_t k = in->kind == M_MAP ? 2 * in->n : in->kind == M_ARR ? in->n : 0;
    if (k) {
        out->kids = calloc(k, sizeof *out->kids);
        for (uint32_t j = 0; out->kids && j < k; j++) mp_clone(&out->kids[j], &in->kids[j]);
    }
}

// Appends key -> *value (moved in) to a map. 0 on allocation failure.
static int mp_append(Mp *map, int64_t key, Mp *value) {
    Mp *k = realloc(map->kids, (size_t)(map->n + 1) * 2 * sizeof *k);
    if (!k) return 0;
    map->kids = k;
    memset(&k[2 * map->n], 0, sizeof *k);
    k[2 * map->n].kind = M_INT, k[2 * map->n].i = key;   // fmt 0: ids as the game writes them, smallest
    k[2 * map->n + 1] = *value;
    map->n++;
    return 1;
}

// ---- the save file
static int plain_parse(Plain *p, const uint8_t *b, size_t n) {
    memset(p, 0, sizeof *p);
    if (n < 8 || memcmp(b, "MS\0\4", 4)) return 0;
    memcpy(p->head, b, 8);
    size_t i = 8;
    while (i < n) {
        if (p->nroots == 4 || !mp_read(b, n, &i, &p->roots[p->nroots], 0)) return 0;
        p->nroots++;
    }
    return p->nroots >= 2;
}

static void plain_free(Plain *p) {
    for (int r = 0; r < p->nroots; r++) mp_free(&p->roots[r]);
}

static uint8_t *decrypt(const uint8_t *d, size_t n, size_t *out_n) {
    if (n < 16) return NULL;
    uint32_t seed = le32(d), check = le32(d + 4);
    uint8_t key[32], ctr[16];
    uint32_t first = derive(seed, key, ctr);
    if (murmur3(d + 8, n - 8, seed) + first != check) return NULL;
    uint8_t *p = malloc(n - 8);
    if (!p) return NULL;
    memcpy(p, d + 8, n - 8);
    aes_ctr(key, ctr, p, n - 8);
    *out_n = n - 8;
    return p;
}

static uint8_t *encrypt(const uint8_t *p, size_t n, size_t *out_n) {
    uint32_t seed;
    if (!plat_random(&seed, sizeof seed)) seed = (uint32_t)plat_now_ns();
    uint8_t key[32], ctr[16];
    uint32_t first = derive(seed, key, ctr);
    uint8_t *d = malloc(n + 8);
    if (!d) return NULL;
    memcpy(d + 8, p, n);
    aes_ctr(key, ctr, d + 8, n);
    uint32_t check = murmur3(d + 8, n, seed) + first;
    for (int k = 0; k < 4; k++) d[k] = (uint8_t)(seed >> 8 * k), d[4 + k] = (uint8_t)(check >> 8 * k);
    *out_n = n + 8;
    return d;
}

// ---- RomFS tables (RSDB/*.rstbl.byml.zs: BYML v7, an array of rows of scalars)
typedef struct { uint8_t *b; size_t n; uint32_t keys, strs; const uint8_t *rows; uint32_t nrows; } Table;

static int table_load(Table *t, const char *name) {
    memset(t, 0, sizeof *t);
    char rel[128];
    snprintf(rel, sizeof rel, "RSDB/%s.Product.%s.rstbl.byml.zs", name, splat3_romfs_tag());
    size_t zn = 0;
    void *z = romfs_read_original_file(rel, &zn);
    if (!z) return 0;
    unsigned long long raw = ZSTD_getFrameContentSize(z, zn);
    if (raw == ZSTD_CONTENTSIZE_UNKNOWN || raw == ZSTD_CONTENTSIZE_ERROR || raw > (64u << 20) || raw < 16) {
        free(z);
        return 0;
    }
    t->b = malloc((size_t)raw);
    size_t got = t->b ? ZSTD_decompress(t->b, (size_t)raw, z, zn) : 0;
    free(z);
    if (!t->b || ZSTD_isError(got) || got != raw || memcmp(t->b, "YB", 2)) {
        free(t->b);
        t->b = NULL;
        return 0;
    }
    t->n = (size_t)raw;
    t->keys = le32(t->b + 4), t->strs = le32(t->b + 8);
    uint32_t root = le32(t->b + 12);
    if (root + 4 > t->n || t->b[root] != 0xc0) {
        free(t->b);
        t->b = NULL;
        return 0;
    }
    t->nrows = le32(t->b + root) >> 8;
    t->rows = t->b + root;
    return 1;
}

static const char *table_str(const Table *t, uint32_t tab, uint32_t i) {
    if (!tab || tab + 8 > t->n || i >= le32(t->b + tab) >> 8) return "";
    return (const char *)t->b + tab + le32(t->b + tab + 4 + 4 * i);
}

// Row r's field: its type byte (0 when absent) and raw value.
static int table_field(const Table *t, uint32_t r, const char *field, uint32_t *v) {
    uint32_t base = 4 + ((t->nrows + 3) & ~3u);
    if (r >= t->nrows || t->rows[4 + r] != 0xc1) return 0;
    uint32_t off = le32(t->rows + base + 4 * r);
    if (off + 4 > t->n) return 0;
    const uint8_t *d = t->b + off;
    uint32_t n = le32(d) >> 8;
    for (uint32_t i = 0; i < n && off + 12 + 8 * i <= t->n; i++) {
        const uint8_t *e = d + 4 + 8 * i;
        if (!strcmp(table_str(t, t->keys, (uint32_t)e[0] | e[1] << 8 | e[2] << 16), field)) {
            *v = le32(e + 4);
            return e[3];
        }
    }
    return 0;
}

static int64_t row_int(const Table *t, uint32_t r, const char *field, int64_t def) {
    uint32_t v;
    int ty = table_field(t, r, field, &v);
    return ty == 0xd1 ? (int32_t)v : ty == 0xd3 ? (int64_t)v : ty == 0xd0 ? (int64_t)v : def;
}

static const char *row_str(const Table *t, uint32_t r, const char *field) {
    uint32_t v;
    return table_field(t, r, field, &v) == 0xa0 ? table_str(t, t->strs, v) : "";
}

static int64_t row_of(const Table *t, int64_t id) {
    for (uint32_t r = 0; r < t->nrows; r++)
        if (row_int(t, r, "Id", INT64_MIN) == id) return r;
    return -1;
}

// The same tables for other modules (auto_match.c: the stages): save_profile.h rsdb_*
struct RsdbTable { Table t; };

RsdbTable *rsdb_open(const char *name) {
    fs_init();   // the RomFS image and index, once (also before the game asks for its first file)
    RsdbTable *r = calloc(1, sizeof *r);
    if (r && table_load(&r->t, name)) return r;
    free(r);
    return NULL;
}

uint32_t rsdb_rows(const RsdbTable *r) { return r ? r->t.nrows : 0; }
int64_t rsdb_int(const RsdbTable *r, uint32_t row, const char *field, int64_t def) {
    return r ? row_int(&r->t, row, field, def) : def;
}
const char *rsdb_str(const RsdbTable *r, uint32_t row, const char *field) { return r ? row_str(&r->t, row, field) : ""; }
int rsdb_bool(const RsdbTable *r, uint32_t row, const char *field) {
    uint32_t v;
    return r && table_field(&r->t, row, field, &v) == 0xd0 && v != 0;
}

void rsdb_close(RsdbTable *r) {
    if (!r) return;
    free(r->t.b);
    free(r);
}

// ---- the profile
typedef struct {
    char path[1100];
    Json root;
    int ok;
    char name[33];
} Profile;

static Profile prof;         // TOTK_SAVE_PROFILE
static Profile match_prof;   // SPLAT3_MATCH_LOADOUT (save_profile_apply_copy)
static PlatOnce prof_once = PLAT_ONCE_INIT;
static volatile int32_t pending_new;   // no save at start: apply to the game's first commit, then restart
static char backup_dir[1100];
static char fail_why[300];   // the last refusal (fail), for save_profile_apply_copy's caller

// The profile.json at path into *pr; 0 with the reason logged (and in fail_why) when unreadable or not an object.
static int prof_read(Profile *pr, const char *p) {
    json_free(&pr->root);
    memset(pr, 0, sizeof *pr);
    snprintf(pr->path, sizeof pr->path, "%s", p);
    uint64_t n = 0;
    uint8_t *b = plat_file_read(p, &n);
    if (!b) {
        snprintf(fail_why, sizeof fail_why, "cannot read %s", p);
        rt_log("save profile: %s; the save is left as it is", fail_why);
        return 0;
    }
    jp = (const char *)b + (n >= 3 && !memcmp(b, "\xef\xbb\xbf", 3) ? 3 : 0);
    if (!json_parse(&pr->root) || pr->root.kind != J_OBJ) {
        snprintf(fail_why, sizeof fail_why, "%s is not a JSON object", p);
        rt_log("save profile: %s; the save is left as it is", fail_why);
        free(b);
        return 0;
    }
    free(b);
    const Json *nm = jget(&pr->root, "name");
    if (nm && nm->kind == J_STR) snprintf(pr->name, sizeof pr->name, "%s", nm->str);
    return pr->ok = 1;
}

static void prof_load(void *arg) {
    (void)arg;
    const char *p = cfg_get("TOTK_SAVE_PROFILE");
    if (p && *p) prof_read(&prof, p);
}

static int profile_on(void) {
    plat_once(&prof_once, prof_load, NULL);
    return prof.ok;
}

const char *save_profile_name(void) { return profile_on() && *prof.name ? prof.name : NULL; }

// Changes made, for the log.
typedef struct { char text[4096]; size_t n; int count; } Log;

static void note(Log *l, const char *fmt, ...) __attribute__((format(printf, 2, 3)));
static void note(Log *l, const char *fmt, ...) {
    l->count++;
    if (l->n + 2 >= sizeof l->text) return;
    va_list ap;
    va_start(ap, fmt);
    int k = vsnprintf(l->text + l->n, sizeof l->text - l->n, fmt, ap);
    va_end(ap);
    if (k > 0) l->n = l->n + (size_t)k < sizeof l->text ? l->n + (size_t)k : sizeof l->text - 1;
}

static int set_int(Plain *p, Log *l, const char *pth, int64_t v) {
    Mp *m = path(p, pth);
    if (!m || m->kind != M_INT) return 0;
    if (m->i != v) note(l, " %s %lld->%lld;", pth, (long long)m->i, (long long)v), m->i = v;
    return 1;
}

static int set_bool(Plain *p, Log *l, const char *pth, int v) {
    Mp *m = path(p, pth);
    if (!m || m->kind != M_BOOL) return 0;
    if (m->i != v) note(l, " %s %s;", pth, v ? "true" : "false"), m->i = v;
    return 1;
}

static int set_str(Plain *p, Log *l, const char *pth, const char *v) {
    Mp *m = path(p, pth);
    if (!m || m->kind != M_STR) return 0;
    size_t n = strlen(v);
    if (m->n == n && !memcmp(m->s, v, n)) return 1;
    uint8_t *s = malloc(n + 1);
    if (!s) return 0;
    memcpy(s, v, n + 1);
    free(m->s);
    m->s = s, m->n = (uint32_t)n;
    note(l, " %s \"%s\";", pth, v);
    return 1;
}

static Mp new_int(int64_t v) {
    Mp m = {0};
    m.kind = M_INT, m.i = v;
    return m;
}

// Field keys are written as uint32, as the game does.
static int map_put(Mp *map, const char *field, Mp v) {
    Mp *k = realloc(map->kids, (size_t)(map->n + 1) * 2 * sizeof *k);
    if (!k) return 0;
    map->kids = k;
    memset(&k[2 * map->n], 0, sizeof *k);
    k[2 * map->n].kind = M_INT, k[2 * map->n].fmt = 0xce, k[2 * map->n].i = name_hash(field);
    k[2 * map->n + 1] = v;
    map->n++;
    return 1;
}

// The ownership records, made as the game made the starter items' records of a new save: a copy of an entry already
// in the map (its fields, order and encodings) with the values below, or built from the field list when it is empty.
enum { REC_TRUE, REC_WEAPON, REC_GEAR, REC_SEEN, REC_BANNER };

static void zero_values(Mp *m) {
    for (uint32_t k = 0; k < m->n; k++) {
        Mp *v = &m->kids[2 * k + 1];
        if (v->kind == M_INT) v->i = 0;
        else if (v->kind == M_F32 || v->kind == M_F64) v->f = 0;
    }
}

static int make_record(Mp *out, const Mp *tmpl, int kind, int64_t rarity, int64_t skill, int64_t now) {
    memset(out, 0, sizeof *out);
    if (kind == REC_TRUE) return out->kind = M_BOOL, out->i = 1, 1;
    if (tmpl && tmpl->kind == M_MAP) {
        mp_clone(out, tmpl);
        if (kind == REC_WEAPON || kind == REC_BANNER || kind == REC_GEAR) zero_values(out);
        if (kind == REC_SEEN) {
            Mp *d = mp_get(out, name_hash("DateTimeUtc")), *s = mp_get(out, name_hash("IsLookedCustomize"));
            if (d) d->i = now;
            if (s) s->i = 1;
        }
    } else {
        out->kind = M_MAP;
        if (kind == REC_WEAPON) {
            static const char *const f[] = {"TotalPaintTubo", "LastPlayDateTimeUtc", "RegularWinPoint",
                                            "RegularWinHighGrade", "Exp", "Level", "RewardAcceptLevel", "WinCount"};
            for (size_t i = 0; i < sizeof f / sizeof *f; i++) map_put(out, f[i], new_int(0));
            Mp w = {0};
            w.kind = M_F32;
            map_put(out, "MaxWeaponPower", w);
        } else if (kind == REC_GEAR) {
            static const char *const f[] = {"Exp", "TotalExp", "Rarity", "MainSkill", "ExSkillArray",
                                            "LastPlayDateTimeUtc", "RandomContext"};
            for (size_t i = 0; i < sizeof f / sizeof *f; i++) {
                Mp v = new_int(0);
                if (!strcmp(f[i], "ExSkillArray")) v.kind = M_ARR;
                map_put(out, f[i], v);
            }
        } else if (kind == REC_SEEN) {
            Mp b = {0};
            b.kind = M_BOOL, b.i = 1;
            map_put(out, "DateTimeUtc", new_int(now));
            map_put(out, "IsLookedCustomize", b);
        } else {
            map_put(out, "AcquiredDateTimeUtc", new_int(0));
        }
    }
    if (kind == REC_GEAR) {   // Rarity = the table's, MainSkill = its main ability, rarity + 1 empty sub slots
        Mp *r = mp_get(out, name_hash("Rarity")), *s = mp_get(out, name_hash("MainSkill")),
           *a = mp_get(out, name_hash("ExSkillArray"));
        if (!r || !s || !a || a->kind != M_ARR) return 0;
        r->i = rarity, s->i = skill;
        Mp tmpl_slot = a->n ? a->kids[0] : new_int(-1);
        tmpl_slot.kids = NULL, tmpl_slot.s = NULL;
        for (uint32_t k = 0; k < a->n; k++) mp_free(&a->kids[k]);
        free(a->kids);
        a->n = (uint32_t)rarity + 1;
        a->kids = calloc(a->n, sizeof *a->kids);
        if (!a->kids) return 0;
        for (uint32_t k = 0; k < a->n; k++) a->kids[k] = tmpl_slot, a->kids[k].i = -1;
    }
    return 1;
}

// Adds item to the map at pth unless present. 1 when added, 0 when present, -1 on an error.
static int own(Plain *p, Log *l, const char *pth, int64_t item, int kind, int64_t rarity, int64_t skill, int64_t now) {
    Mp *m = path(p, pth);
    if (!m || m->kind != M_MAP) return -1;
    if (mp_get(m, item)) return 0;
    Mp rec;
    if (!make_record(&rec, m->n ? &m->kids[1] : NULL, kind, rarity, skill, now) || !mp_append(m, item, &rec)) return -1;
    if (l->count < 64) note(l, " +%s %lld;", strrchr(pth, '/') + 1, (long long)item);
    else l->count++;
    return 1;
}

// The ability enum's reflection string in main ("None = -1 , <name> = 0 , ..."), NULL when main has none.
static const char *ability_enum(void) {
    static const char *enum_s;
    if (!enum_s) {
        const RtLoaded *m = ldr_module(0);
        const char *first = "None = -1 , MainInk_Save = 0";
        for (uint64_t a = m->base + m->text_end; a + 64 < m->base + m->data_off && !enum_s; a++)
            if (*(const char *)(uintptr_t)a == 'N' && !memcmp((const void *)(uintptr_t)a, first, strlen(first)))
                enum_s = (const char *)(uintptr_t)a;
    }
    return enum_s;
}

// The next "<name> = <value>" entry of an enum string at *s: 1 with its name's span and value, 0 at the end.
static int enum_next(const char **s, const char **name, size_t *len, int64_t *v) {
    const char *q = *s;
    while (*q == ' ' || *q == ',') q++;
    const char *eq = strchr(q, '=');
    if (!*q || !eq) return 0;
    *name = q, *len = strcspn(q, " =");
    *v = strtoll(eq + 1, NULL, 10);
    *s = eq + strcspn(eq, ",");
    return 1;
}

// The value of name in the enum string e, -2 when e is NULL or has no such name.
static int64_t enum_value(const char *e, const char *name) {
    const char *nm;
    size_t len, n = strlen(name);
    int64_t v;
    while (e && enum_next(&e, &nm, &len, &v))
        if (len == n && !memcmp(nm, name, n)) return v;
    return -2;
}

static int enum_has(const char *e, int64_t value) {
    const char *nm;
    size_t len;
    int64_t v;
    while (e && enum_next(&e, &nm, &len, &v))
        if (v == value) return 1;
    return 0;
}

// The ability enum's value of a gear's main ability name, from main's reflection string of the enum.
static int64_t ability(const char *name) { return enum_value(ability_enum(), name); }

int64_t save_profile_ability_at(int pos) {
    const char *e = ability_enum(), *nm;
    size_t len;
    int64_t v;
    for (int i = 0; e && enum_next(&e, &nm, &len, &v); i++)
        if (i == pos) return v;
    return -2;
}

// ---- gear_abilities: the three sub abilities of the equipped head, clothes and shoes.
// A sub slot of the save (ExSkillArray) holds an ability enum value, -1 when empty. The enum numbers the abilities a
// sub slot can hold from 0 and the ones only a gear's main slot has from SUB_ABILITY_END.
#define SUB_ABILITY_END 100
#define SUB_SLOTS 3
static const char *const gear_part_names[] = {"head", "clothes", "shoes"};

// The profile's gear_abilities against the enum string e: has[part] and sub[part][slot] (-1: empty). NULL when it
// is absent or well formed, else what is wrong (in why).
static const char *gear_subs_parse(const Json *J, const char *e, int has[3], int64_t sub[3][SUB_SLOTS], char *why,
                                   size_t cap) {
    memset(has, 0, 3 * sizeof *has);
    const Json *ga = jget(J, "gear_abilities");
    if (!ga || ga->kind == J_NULL) return NULL;
    if (ga->kind != J_OBJ) return "gear_abilities must be an object {\"head\": [a, b, c], \"clothes\": ..., \"shoes\": ...}";
    for (int i = 0; i < ga->n; i++) {
        int known = 0;
        for (int g = 0; g < 3; g++) known |= !strcmp(ga->kids[i].key, gear_part_names[g]);
        if (!known) return snprintf(why, cap, "gear_abilities has an unknown part \"%.40s\" (head, clothes, shoes)", ga->kids[i].key), why;
    }
    if (!e) return "the ability names were not found in main";
    for (int g = 0; g < 3; g++) {
        const Json *a = jget(ga, gear_part_names[g]);
        if (!a || a->kind == J_NULL) continue;
        if (a->kind != J_ARR || a->n != SUB_SLOTS)
            return snprintf(why, cap, "gear_abilities.%s must be an array of %d abilities", gear_part_names[g], SUB_SLOTS), why;
        for (int k = 0; k < SUB_SLOTS; k++) {
            const Json *x = &a->kids[k];
            int64_t v;
            if (x->kind == J_NULL) v = -1;
            else if (x->kind == J_STR) {
                v = enum_value(e, x->str);
                if (v == -2)
                    return snprintf(why, cap, "gear_abilities.%s[%d]: the game has no ability named \"%.40s\"",
                                    gear_part_names[g], k, x->str), why;
            } else if (x->kind == J_NUM && x->num == (double)(int64_t)x->num) {
                v = (int64_t)x->num;
                if (!enum_has(e, v))
                    return snprintf(why, cap, "gear_abilities.%s[%d]: %lld is not a value of the game's ability enum",
                                    gear_part_names[g], k, (long long)v), why;
            } else {
                return snprintf(why, cap, "gear_abilities.%s[%d] must be an ability name, an enum value or null",
                                gear_part_names[g], k), why;
            }
            if (v < -1 || v >= SUB_ABILITY_END)
                return snprintf(why, cap, "gear_abilities.%s[%d]: ability %lld is one only a main slot has, not a sub slot",
                                gear_part_names[g], k, (long long)v), why;
            sub[g][k] = v;
        }
        has[g] = 1;
    }
    return NULL;
}

int save_profile_gear_abilities_check(const char *json, const char *enum_s, int64_t sub[3][3], char *why, size_t cap) {
    Json j;
    int has[3] = {0}, mask = -1;
    char buf[200] = "";
    jp = json;
    const char *bad = json_parse(&j) && j.kind == J_OBJ ? gear_subs_parse(&j, enum_s, has, sub, buf, sizeof buf)
                                                        : "not a JSON object";
    if (!bad) mask = has[0] | has[1] << 1 | has[2] << 2;
    if (why && cap) snprintf(why, cap, "%s", bad ? bad : "");
    json_free(&j);
    return mask;
}

typedef struct { const char *part, *table, *equip, *map; } GearPart;
static const GearPart gear_parts[] = {
    {"head", "GearInfoHead", "GearHeadId", "HaveGearHeadMap"},
    {"clothes", "GearInfoClothes", "GearClothesId", "HaveGearClothesMap"},
    {"shoes", "GearInfoShoes", "GearShoesId", "HaveGearShoesMap"},
};

static int fail(const char *why, int64_t v) {
    snprintf(fail_why, sizeof fail_why, "%s (%lld)", why, (long long)v);
    rt_log("save profile: %s; the save is left as it is", fail_why);
    return 0;
}

// Gear the game removes from the ownership maps when it loads a save that lists them (measured with unlock_all; the
// reason is not known; all HowToGet: Shop). unlock_all leaves them out, so that applying the profile again finds
// nothing to add, and a profile cannot equip them. The same list as GEAR_DROPPED in s3save.py.
static const struct { int part; int64_t id; } gear_dropped[] = {
    {0, 3007}, {1, 1030}, {1, 6003}, {2, 2011}, {2, 3009}, {2, 3012},   // part: 0 head, 1 clothes, 2 shoes
};

int save_profile_gear_dropped(int part, int64_t id) {
    for (size_t i = 0; i < sizeof gear_dropped / sizeof *gear_dropped; i++)
        if (gear_dropped[i].part == part && gear_dropped[i].id == id) return 1;
    return 0;
}

// Selectable appearance rows: Order >= 0 and not a Side Order variant. 1 when id is one and belongs to the species
// (squid 1: Inkling, 0: Octoling, -1: either), 2 when it is one of the other species, 0 when it is not selectable.
static int selectable(const Table *t, int64_t id, int squid) {
    int64_t r = row_of(t, id);
    if (r < 0 || row_int(t, (uint32_t)r, "Order", -1) < 0 || strstr(row_str(t, (uint32_t)r, "__RowId"), "Sdodr"))
        return 0;
    uint32_t v;
    return squid < 0 || (table_field(t, (uint32_t)r, "IsSquid", &v) == 0xd0 && (int)v == squid) ? 1 : 2;
}

// The save's current value of a coordinates field; 0 when the save has none.
static int current(Plain *p, const char *field, int64_t *v) {
    char q[96];
    snprintf(q, sizeof q, "0/Common/Coordinates/%s", field);
    Mp *m = path(p, q);
    if (!m || m->kind != M_INT) return 0;
    *v = m->i;
    return 1;
}

// A hair or eyebrow id of the species of the style: 1 when it is, else 0 with the reason logged.
static int check_species(const char *what, const char *table, int64_t id, int squid, const char *style, int from_save) {
    Table t;
    if (!table_load(&t, table)) return fail("an RSDB appearance table is unreadable", 0);
    int ok = selectable(&t, id, squid);
    free(t.b);
    if (ok == 1) return 1;
    char why[300];
    snprintf(why, sizeof why, "%s %lld %s%s%s", what, (long long)id,
             ok == 2 ? (squid ? "belongs to Octolings, not to the style " : "belongs to Inklings, not to the style ")
                     : "is not a selectable one of the style ",
             style, from_save ? " (it is the save's value: give a matching one in the profile too)" : "");
    return fail(why, id);
}

// The fields of the appearance that a new save needs and the profile lacks (absent or null), comma separated in
// names; returns how many.
static int missing_appearance(const Json *J, char *names, size_t cap) {
    static const char *const f[] = {"style", "hair", "eyebrows", "legwear", "skin", "eyes"};
    int n = 0;
    if (cap) names[0] = 0;
    for (size_t i = 0; i < sizeof f / sizeof *f; i++) {
        const Json *j = jget(J, f[i]);
        if (j && j->kind != J_NULL) continue;
        if (cap > strlen(names) + strlen(f[i]) + 3) strcat(strcat(names, n ? ", " : ""), f[i]);
        n++;
    }
    return n;
}

int save_profile_missing_appearance(const char *json, char *names, size_t cap) {
    Json j;
    jp = json;
    int n = json_parse(&j) && j.kind == J_OBJ ? missing_appearance(&j, names, cap) : -1;
    json_free(&j);
    return n;
}

// The appearance: every absent or null field stays as the save has it. Hair and eyebrows must belong to the species
// of the style (the profile's, else the save's), so a profile that changes the style without matching hair and
// eyebrows, or gives hair of the other species, is refused whole. *complete: the profile gives the whole set.
static int apply_appearance(const Json *J, Plain *p, Log *l, int *complete) {
    static const char *const styles[] = {"inkling_f", "inkling_m", "octoling_f", "octoling_m"};
    const Json *st = jget(J, "style");
    int64_t style = -1, hair = 0, brows = 0, legwear = 0, variation = 0, skin = 0, eyes = 0;
    if (st && st->kind != J_NULL) {
        for (int i = 0; i < 4 && st->kind == J_STR; i++)
            if (!strcmp(st->str, styles[i])) style = i;
        if (style < 0) return fail("style must be inkling_f, inkling_m, octoling_f or octoling_m", 0);
    }
    int hs = style >= 0, hh = jint(J, "hair", &hair), hb = jint(J, "eyebrows", &brows),
        hl = jint(J, "legwear", &legwear), hk = jint(J, "skin", &skin), he = jint(J, "eyes", &eyes),
        hv = jint(J, "legwear_variation", &variation);
    if (hh < 0 || hb < 0 || hl < 0 || hk < 0 || he < 0 || hv < 0)
        return fail("hair, eyebrows, legwear, legwear_variation, skin and eyes must be integers", 0);
    *complete = hs && hh && hb && hl && hk && he;
    if (hs || hh || hb) {
        int64_t cur;
        if (!hs) {
            if (!current(p, "ModelType", &cur) || cur < 0 || cur > 3) return fail("the save has no ModelType", 0);
            style = cur;
        }
        if (!hh) hair = current(p, "HairId", &cur) ? cur : -1;
        if (!hb) brows = current(p, "Eyebrow", &cur) ? cur : -1;
        if (!check_species("hair", "HairInfo", hair, style < 2, styles[style], !hh) ||
            !check_species("eyebrows", "EyebrowInfo", brows, style < 2, styles[style], !hb))
            return 0;
    }
    if (hl || hv) {
        if (!hl && !current(p, "BottomId", &legwear)) return fail("the save has no BottomId", 0);
        if (!hv && !current(p, "BottomVariation", &variation)) variation = 0;
        Table t;
        if (!table_load(&t, "BottomInfo")) return fail("RSDB/BottomInfo unreadable", 0);
        int64_t lr = selectable(&t, legwear, -1) == 1 ? row_of(&t, legwear) : -1;
        int64_t nvar = lr >= 0 ? row_int(&t, (uint32_t)lr, "VariationNum", 0) : 0;
        free(t.b);
        if (lr < 0) return fail("legwear is not a selectable legwear", legwear);
        if (!hv && hl && variation >= (nvar > 1 ? nvar : 1)) variation = 0;   // the old legwear's variation
        if (variation < 0 || variation >= (nvar > 1 ? nvar : 1)) return fail("legwear_variation out of range", variation);
    }
    // No table holds the skin tones and eye colours; RSDB NpcInfo's rows use 0..8 and 0..20 (s3save.py SKIN, EYES).
    if (hk && (skin < 0 || skin > 8)) return fail("skin must be 0..8", skin);
    if (he && (eyes < 0 || eyes > 20)) return fail("eyes must be 0..20", eyes);

    static const char *const coords[] = {"0/Common/Coordinates/", "1/LastPlayCoordinates/"};
    char q[160];
    for (int c = 0; c < 2; c++) {
#define SETC(field, v) (snprintf(q, sizeof q, "%s%s", coords[c], field), set_int(p, l, q, v))
        if ((hs && !SETC("ModelType", style)) || (hh && !SETC("HairId", hair)) || (hb && !SETC("Eyebrow", brows)) ||
            (hl && !SETC("BottomId", legwear)) || ((hl || hv) && !SETC("BottomVariation", variation)) ||
            (hk && !SETC("SkinColor", skin)) || (he && !SETC("EyeColor", eyes)))
            return fail("the save has no coordinates block where 11.2.0 has it", c);
#undef SETC
    }
    return 1;
}

// gear_abilities: the sub abilities of the equipped pieces (the save's Coordinates, after the profile's gear), written
// into their records of the player data's ownership maps, the only place the save keeps a piece's abilities. A piece
// with fewer than three slots gets three, and its Rarity (the save's star count, which the game ties the slot count
// to: rarity + 1 slots up to three) is raised to match.
static int apply_gear_abilities(const Json *J, Plain *p, Log *l) {
    int has[3];
    int64_t sub[3][SUB_SLOTS];
    char why[200];
    const char *bad = gear_subs_parse(J, ability_enum(), has, sub, why, sizeof why);
    if (bad) return fail(bad, 0);
    for (int g = 0; g < 3; g++) {
        if (!has[g]) continue;
        const GearPart *gp = &gear_parts[g];
        char q[96];
        int64_t id;
        if (!current(p, gp->equip, &id)) return fail("the save has no equipped gear where 11.2.0 has it", g);
        snprintf(q, sizeof q, "1/%s", gp->map);
        Mp *rec = mp_get(path(p, q), id);
        Mp *r = mp_get(rec, name_hash("Rarity")), *a = mp_get(rec, name_hash("ExSkillArray"));
        if (!r || r->kind != M_INT || !a || a->kind != M_ARR)
            return fail("gear_abilities: the save does not own the equipped gear (give it in \"gear\" too)", id);
        int same = a->n == SUB_SLOTS;
        for (uint32_t k = 0; same && k < SUB_SLOTS; k++) same = a->kids[k].kind == M_INT && a->kids[k].i == sub[g][k];
        if (!same) {
            Mp slot = a->n && a->kids[0].kind == M_INT ? a->kids[0] : new_int(-1);   // the game's encoding of a slot
            Mp *kids = calloc(SUB_SLOTS, sizeof *kids);
            if (!kids) return fail("out of memory", 0);
            for (uint32_t k = 0; k < a->n; k++) mp_free(&a->kids[k]);
            free(a->kids);
            a->kids = kids, a->n = SUB_SLOTS;
            for (uint32_t k = 0; k < SUB_SLOTS; k++) a->kids[k] = slot, a->kids[k].i = sub[g][k];
            note(l, " 1/%s[%lld]/ExSkillArray [%lld, %lld, %lld];", gp->map, (long long)id, (long long)sub[g][0],
                 (long long)sub[g][1], (long long)sub[g][2]);
        }
        if (r->i < SUB_SLOTS - 1) {
            note(l, " 1/%s[%lld]/Rarity %lld->%d;", gp->map, (long long)id, (long long)r->i, SUB_SLOTS - 1);
            r->i = SUB_SLOTS - 1;
        }
    }
    return 1;
}

// chunks: "max" or a count 0..CHUNKS_MAX: the player data's HaveSkillChipMap gets that count for every ability a sub
// slot can hold (the enum's values below SUB_ABILITY_END). Separate from gear_abilities.
#define CHUNKS_MAX 999
static int apply_chunks(const Json *J, Plain *p, Log *l) {
    const Json *c = jget(J, "chunks");
    if (!c || c->kind == J_NULL) return 1;
    int64_t n = CHUNKS_MAX;
    if (c->kind == J_NUM && c->num == (double)(int64_t)c->num) n = (int64_t)c->num;
    else if (c->kind != J_STR || strcmp(c->str, "max")) return fail("chunks must be \"max\" or a count", 0);
    if (n < 0 || n > CHUNKS_MAX) return fail("chunks must be 0..999", n);
    Mp *m = path(p, "1/HaveSkillChipMap");
    const char *e = ability_enum(), *nm;
    if (!m || m->kind != M_MAP) return fail("the save has no HaveSkillChipMap where 11.2.0 has it", 0);
    if (!e) return fail("the ability names were not found in main", 0);
    size_t len;
    int64_t v;
    int changed = 0;
    while (enum_next(&e, &nm, &len, &v)) {
        if (v < 0 || v >= SUB_ABILITY_END) continue;
        Mp *have = mp_get(m, v);
        if (have && have->kind == M_INT && have->i == n) continue;
        if (have && have->kind != M_INT) return fail("HaveSkillChipMap holds a record this code does not know", v);
        Mp count = new_int(n);
        if (have) have->i = n;
        else if (!mp_append(m, v, &count)) return fail("out of memory", 0);
        changed++;
    }
    if (changed) note(l, " 1/HaveSkillChipMap %d abilities = %lld;", changed, (long long)n);
    return 1;
}

static int apply(const Profile *pr, Plain *p, Log *l) {
    const Json *J = &pr->root;
    int64_t now = (int64_t)(plat_unix_time_ns() / 1000000000u);
    int complete = 0;
    char q[160];
    Table t;
    if (!apply_appearance(J, p, l, &complete)) return 0;
    if (*pr->name && !set_str(p, l, "1/UserName", pr->name)) return fail("the save has no UserName", 0);
    // skip_first_launch: true writes Plaza/IsDoneFirstBootSeq; absent counts as true only in a profile with the whole
    // appearance (a partial profile must not end the first launch of a save that is still in it); false leaves it.
    const Json *skip = jget(J, "skip_first_launch");
    int want_skip = skip && skip->kind == J_BOOL ? (int)skip->num : skip && skip->kind != J_NULL ? 0 : complete;
    if (want_skip && !set_bool(p, l, "0/Plaza/IsDoneFirstBootSeq", 1))
        return fail("the save has no Plaza/IsDoneFirstBootSeq", 0);

    const Json *ua = jget(J, "unlock_all");
    int unlock = ua && ua->kind == J_BOOL && ua->num;
    // Weapon: WeaponInfoMain rows of type Versus.
    int64_t w;
    int has_w = jint(J, "weapon", &w);
    if (has_w < 0) return fail("weapon must be an integer", 0);
    if (has_w || unlock) {
        if (!table_load(&t, "WeaponInfoMain")) return fail("RSDB/WeaponInfoMain unreadable", 0);
        int64_t r = has_w ? row_of(&t, w) : 0;
        if (has_w && (r < 0 || strcmp(row_str(&t, (uint32_t)r, "Type"), "Versus"))) {
            free(t.b);
            return fail("weapon is not a Versus main weapon", w);
        }
        for (uint32_t k = 0; k < t.nrows; k++) {
            int64_t id = row_int(&t, k, "Id", -1);
            if (strcmp(row_str(&t, k, "Type"), "Versus") || (!unlock && id != w)) continue;
            if (own(p, l, "1/HaveWeaponMap", id, REC_WEAPON, 0, 0, now) < 0 ||
                own(p, l, "0/Customize/HaveWeaponMap", id, REC_SEEN, 0, 0, now) < 0) {
                free(t.b);
                return fail("the weapon ownership maps are missing", id);
            }
        }
        free(t.b);
        if (has_w) set_int(p, l, "0/Common/Coordinates/WeaponId", w);
    }
    // Gear: rows that are not HowToGet Impossible (NPC gear).
    const Json *gear = jget(J, "gear");
    for (size_t g = 0; g < sizeof gear_parts / sizeof *gear_parts; g++) {
        const GearPart *gp = &gear_parts[g];
        int64_t id = 0;
        int has = jint(gear, gp->part, &id);
        if (has < 0) return fail("gear ids must be integers", 0);
        if (has == 1 && save_profile_gear_dropped((int)g, id))
            return fail("the game removes this gear from the save when it loads it; do not equip it", id);
        if (!has && !unlock) continue;
        if (!table_load(&t, gp->table)) return fail("an RSDB gear table is unreadable", (int64_t)g);
        if (has) {
            int64_t r = row_of(&t, id);
            if (r < 0 || !strcmp(row_str(&t, (uint32_t)r, "HowToGet"), "Impossible")) {
                free(t.b);
                return fail("gear id is not a player gear", id);
            }
        }
        char own_p[96], seen_p[96], eq[96];
        snprintf(own_p, sizeof own_p, "1/%s", gp->map);
        snprintf(seen_p, sizeof seen_p, "0/Customize/%s", gp->map);
        for (uint32_t k = 0; k < t.nrows; k++) {
            int64_t gid = row_int(&t, k, "Id", -1);
            if (!strcmp(row_str(&t, k, "HowToGet"), "Impossible") || (!unlock && gid != id) ||
                save_profile_gear_dropped((int)g, gid))
                continue;
            int64_t rarity = row_int(&t, k, "Rarity", 0), skill = ability(row_str(&t, k, "Skill"));
            if (skill == -2) {
                free(t.b);
                return fail("main ability names not found in main", gid);
            }
            if (own(p, l, own_p, gid, REC_GEAR, rarity < 0 ? 0 : rarity > 2 ? 2 : rarity, skill, now) < 0 ||
                own(p, l, seen_p, gid, REC_SEEN, 0, 0, now) < 0) {
                free(t.b);
                return fail("the gear ownership maps are missing", gid);
            }
        }
        free(t.b);
        if (has) {
            snprintf(eq, sizeof eq, "0/Common/Coordinates/%s", gp->equip);
            set_int(p, l, eq, id);
            snprintf(eq, sizeof eq, "0/Common/Coordinates/%.*sVariationId", (int)(strlen(gp->equip) - 2), gp->equip);
            set_int(p, l, eq, 0);
        }
    }
    if (!apply_gear_abilities(J, p, l) || !apply_chunks(J, p, l)) return 0;
    // Win emote.
    int64_t e;
    int has_e = jint(J, "emote", &e);
    if (has_e < 0) return fail("emote must be an integer", 0);
    if (has_e || unlock) {
        if (!table_load(&t, "EmoteInfo")) return fail("RSDB/EmoteInfo unreadable", 0);
        if (has_e && row_of(&t, e) < 0) {
            free(t.b);
            return fail("emote is not in the game's table", e);
        }
        for (uint32_t k = 0; k < t.nrows; k++) {
            int64_t id = row_int(&t, k, "Id", -1);
            if (!unlock && id != e) continue;
            if (own(p, l, "1/HaveEmoteMap", id, REC_TRUE, 0, 0, now) < 0 ||
                own(p, l, "0/Customize/HaveEmoteMap", id, REC_SEEN, 0, 0, now) < 0) {
                free(t.b);
                return fail("the emote ownership maps are missing", id);
            }
        }
        free(t.b);
        if (has_e) set_int(p, l, "0/Common/Coordinates/WinEmote", e);
    }
    // Splashtag: banner, title words; badges only when the save already owns them (the record of a badge is not known).
    const Json *tag = jget(J, "splashtag");
    static const struct { const char *key, *table, *own, *field; int rec; } parts[] = {
        {"banner", "NamePlateBgInfo", "1/HaveNamePlateBgMap", "Background", REC_BANNER},
        {"adjective", "BynameAdjectiveInfo", "1/HaveBynameAdjectiveMap", "Byname/Adjective", REC_TRUE},
        {"subject", "BynameSubjectInfo", "1/HaveBynameSubjectMap", "Byname/Subject", REC_TRUE},
    };
    static const char *const plates[] = {"0/NamePlateEditor/NamePlate/", "1/LastPlayNamePlate/"};
    for (size_t k = 0; k < sizeof parts / sizeof *parts; k++) {
        int64_t id = 0;
        int has = jint(tag, parts[k].key, &id);
        if (has < 0) return fail("splashtag ids must be integers", 0);
        if (!has && !unlock) continue;
        if (!table_load(&t, parts[k].table)) return fail("an RSDB splashtag table is unreadable", (int64_t)k);
        if (has && row_of(&t, id) < 0) {
            free(t.b);
            return fail("splashtag id is not in the game's table", id);
        }
        for (uint32_t r = 0; r < t.nrows; r++) {
            int64_t rid = row_int(&t, r, "Id", -1);
            if (!unlock && rid != id) continue;
            if (own(p, l, parts[k].own, rid, parts[k].rec, 0, 0, now) < 0) {
                free(t.b);
                return fail("the splashtag ownership maps are missing", rid);
            }
        }
        free(t.b);
        for (int c = 0; has && c < 2; c++) {
            snprintf(q, sizeof q, "%s%s", plates[c], parts[k].field);
            set_int(p, l, q, id);
        }
    }
    int64_t gender;
    if (jint(tag, "title_gender", &gender) == 1)
        for (int c = 0; c < 2; c++) snprintf(q, sizeof q, "%sByname/Gender", plates[c]), set_int(p, l, q, gender);
    const Json *badges = jget(tag, "badges");
    if (badges && badges->kind == J_ARR && badges->n == 3) {
        Mp *a = path(p, "0/Common/HaveNamePlateBadgeMap"), *b = path(p, "1/LastPlayHaveBadgeMap");
        for (int i = 0; i < 3; i++) {
            int64_t id = (int64_t)badges->kids[i].num;
            if (id && !mp_get(a, id) && !mp_get(b, id)) return fail("badge not owned in this save (unknown record)", id);
            for (int c = 0; c < 2; c++) snprintf(q, sizeof q, "%sBadge%d", plates[c], i + 1), set_int(p, l, q, id);
        }
    }
    return 1;
}

// ---- backups: <profile>/save-profile-backup/<yyyymmdd-hhmmss>/save.dat, the newest `save_profile_backups` kept
#define BACKUPS_DEFAULT 10

static int backups_keep(void) {
    const char *v = cfg_get("TOTK_SAVE_PROFILE_BACKUPS");
    if (!v || !*v) return BACKUPS_DEFAULT;
    char *end;
    long n = strtol(v, &end, 10);
    if (*end || n < 0 || n > 100000) {
        rt_log("save profile: save_profile_backups=%s is not 0 (keep all) or a count; keeping %d", v, BACKUPS_DEFAULT);
        return BACKUPS_DEFAULT;
    }
    return (int)n;
}

static int backup_name(const char *s) {   // yyyymmdd-hhmmss
    for (int i = 0; i < 15; i++)
        if (i == 8 ? s[i] != '-' : s[i] < '0' || s[i] > '9') return 0;
    return !s[15];
}

typedef struct { char (*name)[16]; int n, cap; } Names;

static int names_add(const char *name, int kind, uint64_t size, void *ctx) {
    (void)size;
    Names *l = ctx;
    if (kind != PLAT_FS_DIR || !backup_name(name)) return 1;   // a link is never followed, so never taken
    if (l->n == l->cap) {
        void *q = realloc(l->name, (size_t)(l->cap ? l->cap * 2 : 32) * sizeof *l->name);
        if (!q) return 0;
        l->name = q, l->cap = l->cap ? l->cap * 2 : 32;
    }
    memcpy(l->name[l->n++], name, 16);
    return 1;
}

static int names_older(const void *a, const void *b) { return strcmp((const char *)b, (const char *)a); }   // newest first

// Deletes a backup folder: files in it, then the folder. A link inside is removed as a link, never followed.
static int remove_backup(const char *path, int depth);

typedef struct { const char *dir; int depth, bad; } Rm;

static int rm_child(const char *name, int kind, uint64_t size, void *ctx) {
    (void)size;
    Rm *r = ctx;
    char child[1300];
    if (snprintf(child, sizeof child, "%s/%s", r->dir, name) >= (int)sizeof child) return r->bad = 1, 0;
    if (kind == PLAT_FS_DIR) r->bad |= remove_backup(child, r->depth + 1);
    else if (plat_fs_unlink(child) && (kind != PLAT_FS_LINK || plat_fs_rmdir(child))) r->bad = 1;
    return 1;
}

static int remove_backup(const char *path, int depth) {
    Rm r = {path, depth, 0};
    if (depth > 4 || plat_fs_kind(path) != PLAT_FS_DIR) return 1;
    if (plat_fs_list(path, rm_child, &r)) r.bad = 1;
    return r.bad || plat_fs_rmdir(path);
}

// Keeps the newest `keep` backups of dir (a folder named save-profile-backup) and deletes the older ones, by name
// pattern only: nothing else in the folder is touched, no link is followed. keep 0 keeps all. Returns the number
// deleted.
int save_profile_prune_backups(const char *dir, int keep) {
    size_t dn = strlen(dir);
    if (keep <= 0 || dn < 19 || strcmp(dir + dn - 19, "save-profile-backup") ||
        (dn > 19 && dir[dn - 20] != '/' && dir[dn - 20] != 0x5c))
        return 0;
    Names l = {0};
    plat_fs_list(dir, names_add, &l);
    qsort(l.name, (size_t)l.n, sizeof *l.name, names_older);
    int removed = 0;
    for (int i = keep; i < l.n; i++) {
        char path[1300];
        snprintf(path, sizeof path, "%s/%s", dir, l.name[i]);
        if (remove_backup(path, 0)) rt_log("save profile: cannot delete the old backup %s", path);
        else removed++;
    }
    free(l.name);
    return removed;
}

// An edit of the save's tree (a profile, the stage season): 1 when it applies (l counts the changes), 0 refused.
typedef int (*Edit)(const void *ctx, Plain *p, Log *l);

static int edit_profile(const void *ctx, Plain *p, Log *l) { return apply(ctx, p, l); }

// edit applied to <dir>/save.dat; 1 when the file was changed. backup: the old file goes into backup_dir first (the
// player's own save); without it (the match's copy) the file is replaced as it is. what: the edit, for the log (a
// profile's path). *ok (when given): 1 when the edit applied, 0 changes included.
static int edit_file(const char *dir, int backup, Edit edit, const void *ctx, const char *what, int *ok) {
    char file[1200];
    snprintf(file, sizeof file, "%s/save.dat", dir);
    if (ok) *ok = 0;
    uint64_t n = 0;
    uint8_t *d = plat_file_read(file, &n);
    if (!d) {
        snprintf(fail_why, sizeof fail_why, "%s cannot be read", file);
        return 0;
    }
    size_t pn = 0, en = 0;
    uint8_t *plain = decrypt(d, (size_t)n, &pn), *out = NULL;
    Plain p;
    int changed = 0, parsed = plain && plain_parse(&p, plain, pn);
    if (!plain) snprintf(fail_why, sizeof fail_why, "%s does not pass its check (another game version?)", file);
    else if (!parsed) snprintf(fail_why, sizeof fail_why, "%s does not hold the supported game version's layout", file);
    if (!parsed) rt_log("save profile: %s; left as it is", fail_why);
    Log l = {.n = 0};
    int applied = parsed && edit(ctx, &p, &l);
    if (applied && l.count) {
        Buf b = {0};
        put(&b, p.head, 8);
        for (int r = 0; r < p.nroots; r++) mp_write(&b, &p.roots[r]);
        out = b.bad ? NULL : encrypt(b.p, b.n, &en);
        free(b.p);
        size_t check_n = 0;
        uint8_t *check = out ? decrypt(out, en, &check_n) : NULL;   // the game's own check, before anything is replaced
        struct tm tm;
        plat_local_time(&tm);
        char bdir[1200] = "", bfile[1300];
        if (backup) {
            snprintf(bdir, sizeof bdir, "%s/%04d%02d%02d-%02d%02d%02d", backup_dir, tm.tm_year + 1900, tm.tm_mon + 1,
                     tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec);
            snprintf(bfile, sizeof bfile, "%s/save.dat", bdir);
            plat_file_mkdir_p(bdir);
        }
        if (!check) {
            snprintf(fail_why, sizeof fail_why, "the new save does not pass its own check");
            rt_log("save profile: %s; %s left as it is", fail_why, file);
        } else if (backup && !plat_file_write_atomic(bfile, d, (size_t)n)) {
            snprintf(fail_why, sizeof fail_why, "cannot back the save up into %s", bdir);
            rt_log("save profile: %s; left as it is", fail_why);
        } else if (!plat_file_write_atomic(file, out, en)) {
            snprintf(fail_why, sizeof fail_why, "cannot write %s", file);
            rt_log("save profile: %s%s%s%s", fail_why, backup ? " (the backup is in " : "", bdir, backup ? ")" : "");
        } else {
            changed = 1;
            if (ok) *ok = 1;
            rt_log("save profile: %s applied to %s (%d changes, %s%s):%s", what, file, l.count,
                   backup ? "backup " : "no backup: a match copy", bdir, l.text);
            if (backup) {
                int keep = backups_keep(), gone = save_profile_prune_backups(backup_dir, keep);
                if (gone) rt_log("save profile: deleted %d older backups (the newest %d are kept)", gone, keep);
            }
        }
        free(check);
    } else if (applied) {
        if (ok) *ok = 1;
        rt_log("save profile: %s applied to %s: 0 changes (the save already matches; nothing written, no backup)",
               what, file);
    }
    if (parsed) plain_free(&p);
    free(out);
    free(plain);
    free(d);
    return changed;
}

void save_profile_at_start(const char *save_dir, const char *profile_dir) {
    if (!profile_on()) return;
    snprintf(backup_dir, sizeof backup_dir, "%s/save-profile-backup", profile_dir);
    if (!find_key_table()) {
        rt_log("save profile: the save's key table was not found in main (not Splatoon 3 11.2.0?); profile off");
        prof.ok = 0;
        return;
    }
    char file[1200];
    snprintf(file, sizeof file, "%s/save.dat", save_dir);
    if (!plat_file_exists(file)) {
        char miss[160];
        if (missing_appearance(&prof.root, miss, sizeof miss)) {
            rt_log("save profile: no save yet, and a new save needs the whole appearance: the profile lacks %s; the "
                   "profile is not applied, the game creates its own character",
                   miss);
            return;
        }
        plat_xchg32(&pending_new, 1);
        rt_log("save profile: no save yet; the profile goes into the save the game creates, then the game restarts");
        return;
    }
    edit_file(save_dir, 1, edit_profile, &prof, prof.path, NULL);
}

void save_profile_after_commit(const char *save_dir) {
    // Only the first commit of a profile that started without a save. plat_cas32 takes the new value before the
    // expected one: with them the other way round this set the flag at the first commit of every run and then took
    // each later commit for a new save's, so a weapon chosen in the game was written back to the profile's and the
    // game restarted as soon as it saved.
    if (!plat_xchg32(&pending_new, 0)) return;
    if (edit_file(save_dir, 1, edit_profile, &prof, prof.path, NULL)) {
        rt_log("save profile: restarting so the game loads the new save");
        gfx_window_request_restart();
    }
}

int save_profile_apply_copy(const char *save_dir, const char *path, char *why, size_t cap) {
    if (why && cap) why[0] = 0;
    fail_why[0] = 0;
    if (!find_key_table()) {
        snprintf(fail_why, sizeof fail_why, "the save's key table was not found in main (not Splatoon 3 11.2.0?)");
    } else if (prof_read(&match_prof, path)) {
        int ok = 0;
        edit_file(save_dir, 0, edit_profile, &match_prof, match_prof.path, &ok);
        if (ok) return 1;
        if (!fail_why[0]) snprintf(fail_why, sizeof fail_why, "the profile was not applied");
    }
    if (why && cap) snprintf(why, cap, "%s", fail_why);
    return 0;
}

// What the match profile asks for: its weapon and gear ids, -1 for one it leaves as the save has it. 0 without one.
int save_profile_match_loadout(int64_t *weapon, int64_t gear[3]) {
    if (!match_prof.ok) return 0;
    *weapon = -1;
    if (jint(&match_prof.root, "weapon", weapon) != 1) *weapon = -1;
    const Json *g = jget(&match_prof.root, "gear");
    for (size_t k = 0; k < 3; k++)
        if (jint(g, gear_parts[k].part, &gear[k]) != 1) gear[k] = -1;
    return 1;
}

// ---- the private room's stage list. The room's list (the private battle settings' stage list, main+0x3903e90 at the
// room's start) takes the rows of RSDB/VersusSceneInfo that are shown in stage lists and whose Season is at most
// max(1, the player data's Season/CurrentSeasonId, the online season when connected); -1 in a save that never went
// online: the 13 stages of seasons -1 to 1 (measured; CurrentSeasonId 8 gave the 25 stages shown in stage lists).
typedef struct { int64_t season; } Stages;

static int edit_stages(const void *ctx, Plain *p, Log *l) {
    const Stages *s = ctx;
    Mp *m = path(p, "1/Season/CurrentSeasonId");
    if (!m || m->kind != M_INT) return fail("the save has no Season/CurrentSeasonId where 11.2.0 has it", 0);
    if (m->i < s->season) note(l, " 1/Season/CurrentSeasonId %lld->%lld;", (long long)m->i, (long long)s->season), m->i = s->season;
    return 1;
}

int64_t save_profile_open_stages(const char *save_dir, char *why, size_t cap) {
    if (why && cap) why[0] = 0;
    fail_why[0] = 0;
    Stages s = {-1};
    Table t;
    if (!find_key_table()) {
        snprintf(fail_why, sizeof fail_why, "the save's key table was not found in main (not Splatoon 3 11.2.0?)");
    } else if (!table_load(&t, "VersusSceneInfo")) {
        snprintf(fail_why, sizeof fail_why, "RSDB/VersusSceneInfo unreadable");
    } else {
        uint32_t v;
        for (uint32_t r = 0; r < t.nrows; r++)
            if (table_field(&t, r, "IsDisplayInStageList", &v) == 0xd0 && v && row_int(&t, r, "Season", -1) > s.season)
                s.season = row_int(&t, r, "Season", -1);
        free(t.b);
        int ok = 0;
        if (s.season < 0) snprintf(fail_why, sizeof fail_why, "RSDB/VersusSceneInfo has no season");
        else edit_file(save_dir, 0, edit_stages, &s, "the stage season", &ok);
        if (ok) return s.season;
        if (!fail_why[0]) snprintf(fail_why, sizeof fail_why, "the stage season was not applied");
    }
    if (why && cap) snprintf(why, cap, "%s", fail_why);
    return -1;
}
