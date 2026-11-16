// --list-match-options (Splatoon 3): the automatic match's mode and stage values (docs/auto-match.md), read from the
// user's game files at run time, with the game's own labels. Nothing of them is kept in this source:
//   - modes: the neutral keys of SPLAT3_MATCH_RULES mode= in the order of the private room's mode selector, with the
//     game's rule id and name (main's VersusRule enum text) and its label (Mals CommonMsg/VS/VSRuleName);
//   - stages: RSDB/VersusSceneInfo's rows (Id, scene, shown in stage lists) with their labels (Mals
//     CommonMsg/VS/VSStageName, by the scene name without "Vss_").
// The labels' language is SPLAT3_LIST_LANG, a Mals file prefix (USen by default; EUfr, JPja, ...). A room offers every
// stage shown in stage lists: the match plays on a copy of the save whose season opens them all (fs_host.c; the status
// file's "stages" once a room is up). With SPLAT3_MATCH_STAGES=save it offers the host's save's own list.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <zstd.h>

#include "cfg.h"
#include "fs.h"
#include "auto_match.h"
#include "host.h"
#include "json_esc.h"
#include "restbl.h"
#include "save_profile.h"
#include "splat3_addr.h"

// ---- MSBT (a message file): labels (LBL1) to UTF-16 texts (TXT2), tags dropped, as UTF-8
typedef struct { const uint8_t *p; size_t n; const char *want; char *out; size_t cap; int found; } MsgFind;

static uint32_t u32le(const uint8_t *p) { return p[0] | p[1] << 8 | p[2] << 16 | (uint32_t)p[3] << 24; }
static uint16_t u16le(const uint8_t *p) { return (uint16_t)(p[0] | p[1] << 8); }

// The text of label `want` in message file p[0..n) as UTF-8 into out; 0 when absent
static int msbt_text(const uint8_t *p, size_t n, const char *want, char *out, size_t cap) {
    if (n < 0x20 || memcmp(p, "MsgStdBn", 8) || p[8] != 0xff || p[9] != 0xfe || p[0xc] != 1) return 0;
    const uint8_t *lbl = NULL, *txt = NULL;
    size_t lbl_n = 0, txt_n = 0, o = 0x20;
    for (unsigned s = 0, ns = u16le(p + 0xe); s < ns && o + 16 <= n; s++) {
        size_t sz = u32le(p + o + 4);
        if (o + 16 + sz > n) return 0;
        if (!memcmp(p + o, "LBL1", 4)) lbl = p + o + 16, lbl_n = sz;
        if (!memcmp(p + o, "TXT2", 4)) txt = p + o + 16, txt_n = sz;
        o = (o + 16 + sz + 15) & ~(size_t)15;
    }
    if (!lbl || !txt || lbl_n < 4 || txt_n < 4) return 0;
    size_t want_n = strlen(want);
    long index = -1;
    for (uint32_t slot = 0, slots = u32le(lbl); slot < slots && 4 + 8 * slot + 8 <= lbl_n && index < 0; slot++) {
        uint32_t cnt = u32le(lbl + 4 + 8 * slot), at = u32le(lbl + 8 + 8 * slot);
        for (uint32_t k = 0; k < cnt && at < lbl_n; k++) {
            size_t len = lbl[at];
            if (at + 1 + len + 4 > lbl_n) break;
            if (len == want_n && !memcmp(lbl + at + 1, want, len)) index = (long)u32le(lbl + at + 1 + len);
            at += 1 + (uint32_t)len + 4;
        }
    }
    if (index < 0 || (uint32_t)index >= u32le(txt) || 4 + 4 * (size_t)index + 4 > txt_n) return 0;
    size_t i = u32le(txt + 4 + 4 * index), k = 0;
    while (i + 2 <= txt_n && k + 4 < cap) {
        uint32_t c = u16le(txt + i);
        if (!c) break;
        if (c == 0x0e && i + 8 <= txt_n) {   // a tag: group, type, its parameters' size, the parameters
            i += 8 + u16le(txt + i + 6);
            continue;
        }
        if (c == 0x0f) {   // a tag's end: group, type
            i += 6;
            continue;
        }
        i += 2;
        if (c >= 0xd800 && c < 0xdc00 && i + 2 <= txt_n) c = 0x10000 + ((c - 0xd800) << 10) + (u16le(txt + i) - 0xdc00), i += 2;
        if (c < 0x80) out[k++] = (char)c;
        else if (c < 0x800) out[k++] = (char)(0xc0 | c >> 6), out[k++] = (char)(0x80 | (c & 0x3f));
        else if (c < 0x10000) out[k++] = (char)(0xe0 | c >> 12), out[k++] = (char)(0x80 | (c >> 6 & 0x3f)), out[k++] = (char)(0x80 | (c & 0x3f));
        else out[k++] = (char)(0xf0 | c >> 18), out[k++] = (char)(0x80 | (c >> 12 & 0x3f)), out[k++] = (char)(0x80 | (c >> 6 & 0x3f)), out[k++] = (char)(0x80 | (c & 0x3f));
    }
    out[k] = 0;
    return 1;
}

typedef struct { const char *name; const uint8_t *p; size_t n; } Member;

static void find_member(const char *name, const uint8_t *data, size_t size, void *ctx) {
    Member *m = ctx;
    if (!strcmp(name, m->name)) m->p = data, m->n = size;
}

// JSON string contents (json_esc.h): the labels come from the game's message files, which a mod may replace
static void jstr(FILE *f, const char *s) { json_fput(f, s); }

int list_match_options(void) {
    const char *lang = cfg_get("SPLAT3_LIST_LANG");
    if (!lang || !*lang) lang = "USen";
    fs_init();
    // the labels: Mals/<lang>.Product.<tag>.sarc.zs (the tag: splat3_addr.h), a zstd-compressed SARC of message files
    char rel[96];
    snprintf(rel, sizeof rel, "Mals/%s.Product.%s.sarc.zs", lang, splat3_romfs_tag());
    size_t zn = 0;
    void *z = romfs_read_original_file(rel, &zn);
    unsigned long long raw = z ? ZSTD_getFrameContentSize(z, zn) : 0;
    uint8_t *sarc = raw && raw < (64u << 20) ? malloc((size_t)raw) : NULL;
    size_t got = sarc ? ZSTD_decompress(sarc, (size_t)raw, z, zn) : 0;
    free(z);
    Member rules = {"CommonMsg/VS/VSRuleName.msbt", NULL, 0}, stages = {"CommonMsg/VS/VSStageName.msbt", NULL, 0};
    if (sarc && !ZSTD_isError(got)) {
        restbl_sarc_each(sarc, got, find_member, &rules);
        restbl_sarc_each(sarc, got, find_member, &stages);
    } else {
        fprintf(stderr, "list-match-options: %s not readable; labels left out\n", rel);
    }
    // the game's rule names: main's VersusRule enum text (auto_match.c automatch_rule_names)
    ldr_load(rt_config()->exefs);
    char names[8][8] = {{0}};
    int nnames = automatch_rule_names(ldr_module(0)->base, names, 8);
    FILE *f = stdout;
    fprintf(f, "{\"format\": \"splat3-match-options\", \"version\": 1, \"language\": \"");
    jstr(f, lang);
    fprintf(f, "\",\n \"modes\": [");
    int nmodes = 0;
    for (int i = 0; automatch_mode_key(i); i++, nmodes++) {
        char label[160] = "";
        if (i < nnames && rules.p) msbt_text(rules.p, rules.n, names[i], label, sizeof label);
        fprintf(f, "%s\n  {\"key\": \"%s\", \"rule\": %d, \"name\": \"", i ? "," : "", JE(automatch_mode_key(i)), i);
        jstr(f, i < nnames ? names[i] : "?");
        fprintf(f, "\", \"label\": \"");
        jstr(f, label);
        fprintf(f, "\"}");
    }
    fprintf(f, "\n ],\n \"not_in_private_rooms\": [");
    for (int i = nmodes; i < nnames; i++) {
        char label[160] = "";
        if (rules.p) msbt_text(rules.p, rules.n, names[i], label, sizeof label);
        fprintf(f, "%s{\"rule\": %d, \"name\": \"", i > nmodes ? ", " : "", i);
        jstr(f, names[i]);
        fprintf(f, "\", \"label\": \"");
        jstr(f, label);
        fprintf(f, "\"}");
    }
    fprintf(f, "],\n \"stages\": [");
    RsdbTable *t = rsdb_open("VersusSceneInfo");
    uint32_t nrows = rsdb_rows(t);
    int first = 1;
    for (int64_t order = 0; order <= 1000 && t; order++)   // in the game's DisplayOrder
        for (uint32_t r = 0; r < nrows; r++) {
            if (rsdb_int(t, r, "DisplayOrder", -1) != order) continue;
            const char *scene = rsdb_str(t, r, "__RowId");
            char label[160] = "";
            if (stages.p && !strncmp(scene, "Vss_", 4) && !msbt_text(stages.p, stages.n, scene + 4, label, sizeof label)) {
                char base[64];   // the labels name a stage without its version digits (a scene "Vss_<base><digits>" has the label "<base>")
                snprintf(base, sizeof base, "%s", scene + 4);
                for (size_t k = strlen(base); k && base[k - 1] >= '0' && base[k - 1] <= '9';) base[--k] = 0;
                msbt_text(stages.p, stages.n, base, label, sizeof label);
            }
            fprintf(f, "%s\n  {\"stage\": %lld, \"scene\": \"", first ? "" : ",", (long long)rsdb_int(t, r, "Id", -1));
            jstr(f, scene);
            fprintf(f, "\", \"label\": \"");
            jstr(f, label);
            fprintf(f, "\", \"in_stage_lists\": %s}", rsdb_bool(t, r, "IsDisplayInStageList") ? "true" : "false");
            first = 0;
        }
    // What this build drives (docs/auto-match.md "Capabilities"): a launcher passes it on to a match server as it is
    // stage_list "all": the room offers every stage the game's stage lists show (in_stage_lists), whatever the host's
    // save (fs_host.c, the match's copy of the save; SPLAT3_MATCH_STAGES=save gives the save's own list instead)
    // spectators (docs/auto-match.md "Spectators"): teams letter S, counted in players=; a room has 2 places for them
    // whatever its mode (the team screen's stands); verified_*: what was run with this build
    fprintf(f, "\n ],\n \"stage_random\": true,\n \"stage_list\": \"all\",\n"
               " \"teams\": {\"places_per_team\": 4, \"players_min\": 2, \"players_max\": 8, \"uneven\": true, "
               "\"any_team\": true, \"by_name\": \"relay\", \"spectators\": true, \"verified_players_max\": 2},\n"
               " \"spectators\": {\"max\": 2, \"team\": \"S\", \"in_players\": true, \"session_max\": 10, \"modes\": [");
    for (int i = 0; automatch_mode_key(i); i++) fprintf(f, "%s\"%s\"", i ? ", " : "", JE(automatch_mode_key(i)));
    fprintf(f, "], \"verified_max\": %d, \"verified_modes\": [%s], \"host_verified\": %s, \"seat\": [\"status\", "
               "\"result\"], \"result_outcome\": \"spectated\", \"wait\": {\"key\": \"spectators_wait\", \"min\": 0, "
               "\"max\": 600, \"default\": 60, \"needs\": \"teams_by_name\"}},\n"
               " \"room_options\": {",
            SPECTATORS_VERIFIED_MAX, SPECTATORS_VERIFIED_MODES, SPECTATOR_HOST_VERIFIED ? "true" : "false");
    for (int i = 0; automatch_option_key(i); i++)
        fprintf(f, "%s\"%s\": {\"values\": [0, 1], \"default\": 0}", i ? ", " : "", JE(automatch_option_key(i)));
    fprintf(f, "},\n \"recon\": false,\n \"time_limit\": false,\n \"rounds\": {\"command\": \"next\", \"faster_than_restart\": true, \"per_round\": "
               "[\"mode\", \"stage\", \"teams\", \"players\", \"room_options\", \"results_hold\", \"spectators_wait\"]},\n"
               " \"results_hold\": {\"min\": 0, \"max\": 600, \"default\": 10},\n"
               " \"loadout\": \"profile\",\n"
               " \"match_loadout\": {\"setting\": \"SPLAT3_MATCH_LOADOUT\", \"format\": \"save_profile\", \"fields\": "
               "[\"weapon\", \"gear\"], \"player_save_written\": false, \"progress_kept\": false, \"checked_in_result\": true},\n"
               // What this engine can do beyond the fields above, by name: a launcher shows a feature only when it
               // is listed. gear-abilities: a save profile's gear_abilities and chunks are applied; main-abilities:
               // its main_abilities are (save-profile.md).
               " \"capabilities\": [\"gear-abilities\", \"main-abilities\"],\n"
               " \"result\": {\"version\": 2, \"outcomes\": [\"completed\", \"aborted\", \"spectated\"], \"seat\": true}\n}\n");
    rsdb_close(t);
    free(sarc);
    return t && nnames > nmodes ? 0 : 1;
}
