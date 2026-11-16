// Splatoon 3's main offsets per game build (splat3_addr.h). The 11.2.0 row was measured on that build (scene_start.c,
// auto_match.c); the 11.3.0 row is the 11.2.0 one carried by tools/port_offsets.py: every code offset lies in a run of
// thousands of instructions equal to 11.2.0's but for their targets, the callers of the function entries agree, and
// every adrp pair that formed a data address in 11.2.0 forms the new one in 11.3.0. The gyro tuning and camera offsets
// (mouse_native.c) and the gyro processor (input_first.c) were found in 11.3.0 and are not carried to 11.2.0, whose row
// leaves them 0. The RomFS tag is the one in the names of the build's own RSDB and Mals files.
#include <string.h>

#include "host.h"
#include "splat3_addr.h"

static const Splat3Addr rows[] = {
    {.target = "splatoon3-v1120",
     .boot_choice = 0x375338c, .request_scene = 0x3d2f440, .boot_plaza_call = 0x37534c0,
     .boot_playermake_call = 0x3753478, .scene_states = 0x60f24e0,
     .pia_browse_done = 0x974f10, .net_browse = 0x44ea9f0, .save_holder = 0x6097e78,
     .room_commit = 0x38f9930, .room_screen = 0x38f7ff0,
     .net_versus_out = 0x3d7c6f8, .net_versus_in = 0x3d7c844,
     .net_common_out = 0x3d7cda4, .net_common_in = 0x3d7ced4, .net_common_in_b = 0x3d7cf68,
     .rule_names = 0x4f30499, .romfs_tag = "b20"},
    {.target = "splatoon3-v1130",
     .boot_choice = 0x3753ccc, .request_scene = 0x3d2fd80, .boot_plaza_call = 0x3753e00,
     .boot_playermake_call = 0x3753db8, .scene_states = 0x60f34f0,
     .pia_browse_done = 0x974f10, .net_browse = 0x44eb360, .save_holder = 0x6098e88,
     .room_commit = 0x38fa270, .room_screen = 0x38f8930,
     .net_versus_out = 0x3d7d038, .net_versus_in = 0x3d7d184,
     .net_common_out = 0x3d7d6e4, .net_common_in = 0x3d7d814, .net_common_in_b = 0x3d7d8a8,
     .rule_names = 0x4f31499, .gyro_tuning = 0x60c1df8, .gyro_attitude_load = 0x2afb014,
     .pitch_curve = 0x28fba70, .camera_pose = 0x28eb664, .gyro_processor = 0x2afaf6c, .romfs_tag = "b30"},
};

const Splat3Addr *splat3_addr(void) {
    if (rt_target.family != RT_GAME_SPLATOON3 || !rt_target.name) return NULL;
    for (size_t i = 0; i < sizeof rows / sizeof *rows; i++)
        if (!strcmp(rows[i].target, rt_target.name)) return &rows[i];
    return NULL;
}

const char *splat3_romfs_tag(void) {
    const Splat3Addr *a = splat3_addr();
    return a ? a->romfs_tag : "";
}
