// Splatoon 3: what the runtime knows of one game build (recomp/targets.py), one row per build: offsets in the game's
// main module and the tag its RomFS names carry. What each offset is: scene_start.c (the boot choice, the scene states), auto_match.c (the
// session search, the room screen, the room's net states, the rule names), mouse_native.c (the gyro tuning switch) and
// input_first.c (the gyro processor).
// A new build's row is carried from a known one with tools/port_offsets.py, which matches the two builds' code.
#ifndef SPLAT3_ADDR_H
#define SPLAT3_ADDR_H

#include <stdint.h>

typedef struct {
    const char *target;                // the target's name (rt_target.name)
    // scene_start.c
    uint64_t boot_choice;              // the function that picks the first scene
    uint64_t request_scene;            // scene request (scene manager, const char *const *name, flags)
    uint64_t boot_plaza_call;          // bl request_scene ("Plaza") in the boot choice
    uint64_t boot_playermake_call;     // bl request_scene ("PlayerMake") in the boot choice
    uint64_t scene_states;             // data: the scene states' holder
    // auto_match.c
    uint64_t pia_browse_done;          // BrowseSessionJob::CompleteProcess
    uint64_t net_browse;               // the game's network system: browse
    uint64_t save_holder;              // data: the save data object's pointer
    uint64_t room_commit;              // the room screen's commit (x0 the screen)
    uint64_t room_screen;              // the room screen's update
    uint64_t net_versus_out, net_versus_in;                      // VersusPrivateSettingNetState serialize / deserialize
    uint64_t net_common_out, net_common_in, net_common_in_b;     // CommonPrivateSettingNetState; _b: through its second base
    uint64_t rule_names;               // read-only data: the VersusRule enum's text
    // mouse_native.c (0: the build has no row for it, native mouse aiming stays off)
    uint64_t gyro_tuning;              // data: the game's gyro tuning switches
    uint64_t gyro_attitude_load;       // the gyro processor's ldrb of the switch that hands out the newest attitude
    uint64_t pitch_curve;              // the player camera's map from a tilt in degrees to its pitch, -1 to 1
    uint64_t camera_pose;              // the player camera's position and target for a pitch of -1 to 1
    // input_first.c (0: the build has no row for it, input first leaves the gyro processor where the game runs it)
    uint64_t gyro_processor;           // the gyro processor's frame update (x0 its object), a component of a late job
    // RomFS: the build's tag in the names of versioned files (RSDB/<table>.Product.<tag>.rstbl.byml.zs,
    // Mals/<language>.Product.<tag>.sarc.zs)
    const char *romfs_tag;
} Splat3Addr;

// The running target's row, or NULL when the target is not a Splatoon 3 build with a row.
const Splat3Addr *splat3_addr(void);
// The running target's RomFS tag, "" without a row (no versioned file is found then)
const char *splat3_romfs_tag(void);

#endif
