// --probe-profile: save_profile's pieces that need no game files: the dropped gear list, the fields a new save is
// missing, and the pruning of old backups (save_profile.c). The application to a save needs the game: docs/save-profile.md.
#include <stdio.h>
#include <string.h>

#include "host.h"
#include "save_profile.h"
#include "platform/platform.h"

static int fails;
#define CHECK(c, what) do { if (!(c)) { fails++; fprintf(stderr, "FAIL profile: %s\n", what); } } while (0)

int probe_profile(void) {
    CHECK(save_profile_gear_dropped(0, 3007) && save_profile_gear_dropped(1, 1030) && save_profile_gear_dropped(1, 6003) &&
              save_profile_gear_dropped(2, 2011) && save_profile_gear_dropped(2, 3009) && save_profile_gear_dropped(2, 3012),
          "the six dropped gear ids");
    CHECK(!save_profile_gear_dropped(1, 3007) && !save_profile_gear_dropped(0, 1018), "other parts and ids are not dropped");

    char n[160];
    CHECK(save_profile_missing_appearance("{\"style\":\"octoling_f\",\"hair\":33,\"eyebrows\":21,\"legwear\":1,"
                                          "\"skin\":2,\"eyes\":18}", n, sizeof n) == 0, "a full appearance misses nothing");
    CHECK(save_profile_missing_appearance("{\"weapon\":50}", n, sizeof n) == 6 && !strncmp(n, "style, hair,", 12),
          "a weapon-only profile misses all six");
    CHECK(save_profile_missing_appearance("{\"style\":\"inkling_m\",\"hair\":null,\"eyebrows\":1,\"legwear\":0,\"skin\":1,"
                                          "\"eyes\":1}", n, sizeof n) == 1 && !strcmp(n, "hair"), "null counts as absent");
    CHECK(save_profile_missing_appearance("[1]", n, sizeof n) == -1, "not an object");

    // gear_abilities against a made-up enum of the game's form (no game files here): names, values, null, refusals
    static const char en[] = "None = -1 , Aa = 0 , Bb = 1 , Cc = 13 , Main = 100";
    int64_t sub[3][3];
    char why[200];
    CHECK(save_profile_gear_abilities_check("{\"weapon\":50}", en, sub, why, sizeof why) == 0, "no gear_abilities: nothing");
    CHECK(save_profile_gear_abilities_check("{\"gear_abilities\":{\"head\":[\"Aa\",1,null],\"shoes\":[\"Cc\",\"None\",13]}}",
                                            en, sub, why, sizeof why) == 5 &&
              sub[0][0] == 0 && sub[0][1] == 1 && sub[0][2] == -1 && sub[2][0] == 13 && sub[2][1] == -1 && sub[2][2] == 13,
          "names, values, null and None");
    static const char *const refused[] = {
        "{\"gear_abilities\":[1]}",                             // not an object
        "{\"gear_abilities\":{\"hat\":[0,0,0]}}",                 // an unknown part
        "{\"gear_abilities\":{\"head\":[0,0]}}",                  // not three
        "{\"gear_abilities\":{\"head\":[0,0,0,0]}}",
        "{\"gear_abilities\":{\"head\":\"Aa\"}}",
        "{\"gear_abilities\":{\"head\":[\"A\",0,0]}}",            // a prefix of a name is not a name
        "{\"gear_abilities\":{\"head\":[\"aa\",0,0]}}",
        "{\"gear_abilities\":{\"head\":[2,0,0]}}",                // not a value of the enum
        "{\"gear_abilities\":{\"head\":[0.5,0,0]}}",
        "{\"gear_abilities\":{\"head\":[true,0,0]}}",
        "{\"gear_abilities\":{\"clothes\":[\"Main\",0,0]}}",      // a main-slot ability
        "{\"gear_abilities\":{\"clothes\":[0,0,100]}}",
    };
    for (size_t i = 0; i < sizeof refused / sizeof *refused; i++)
        CHECK(save_profile_gear_abilities_check(refused[i], en, sub, why, sizeof why) == -1 && why[0], refused[i]);
    CHECK(save_profile_gear_abilities_check("{\"gear_abilities\":{\"head\":[0,0,0]}}", NULL, sub, why, sizeof why) == -1,
          "no enum: refused");

    char root[1024], dir[1100], p[1200];
    if (!plat_exe_path(root, sizeof root)) return 1;
    char *cut = plat_last_sep(root);
    if (!cut) return 1;
    strcpy(cut + 1, "profile-probe");
    plat_file_mkdir_p(root);
    snprintf(dir, sizeof dir, "%s/save-profile-backup", root);
    plat_file_mkdir_p(dir);
    static const char *const names[] = {"20260101-000001", "20260101-000002", "20260101-000003", "20260101-000004",
                                        "20260101-000005", "not-a-backup", "20260101-00000"};
    for (size_t i = 0; i < sizeof names / sizeof *names; i++) {
        snprintf(p, sizeof p, "%s/%s", dir, names[i]);
        plat_file_mkdir_p(p);
        strcat(p, "/save.dat");
        plat_file_write(p, "x", 1);
    }
    snprintf(p, sizeof p, "%s/20260101-000009", dir);   // a file with a backup's name is not a backup folder
    plat_file_write(p, "x", 1);
    CHECK(save_profile_prune_backups(dir, 0) == 0, "0 keeps all");
    CHECK(save_profile_prune_backups(root, 2) == 0, "refuses a folder not named save-profile-backup");
    CHECK(save_profile_prune_backups(dir, 2) == 3, "three of five deleted");
    for (size_t i = 0; i < sizeof names / sizeof *names; i++) {
        snprintf(p, sizeof p, "%s/%s", dir, names[i]);
        int keep = i >= 3;   // the two newest and the two names that do not match the pattern
        CHECK(plat_dir_exists(p) == keep, "the newest two and the non-matching names stay, the others go");
    }
    snprintf(p, sizeof p, "%s/20260101-000009", dir);
    CHECK(plat_file_exists(p), "a file is left alone");
    // clean up
    for (size_t i = 0; i < sizeof names / sizeof *names; i++) {
        snprintf(p, sizeof p, "%s/%s/save.dat", dir, names[i]);
        plat_file_delete(p);
        snprintf(p, sizeof p, "%s/%s", dir, names[i]);
        plat_fs_rmdir(p);
    }
    snprintf(p, sizeof p, "%s/20260101-000009", dir);
    plat_file_delete(p);
    plat_fs_rmdir(dir);
    plat_fs_rmdir(root);
    printf("probe-profile: %s (%d failures)\n", fails ? "FAILED" : "passed", fails);
    return fails != 0;
}
