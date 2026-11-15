# Settings reference

`kalamar.ini.example` describes the settings for players (`totk.ini.example` is the copy for the upstream's targets). This page lists the rest: diagnostics, test hooks, paths and experiments. Set one as `name = value` in `kalamar.ini` (`totk.ini` for the upstream's targets), or as the environment variable `KLM_<NAME>`, which wins over the file (older names: "Legacy names" at the end). Remove the line to go back to the default.

## Safety

- **Safe**: changes only what the row says, with no effect on stability or saves.
- **Caution**: can slow the game, grow files, or change the picture, and undoes cleanly when removed.
- **Risky**: can crash or hang the game, or break the picture or game logic, while it is set.
- **Save risk**: can damage or lose saves, or point the game at other game or save files.
- **Unknown**: the code did not settle it.

Many rows below are developer tools that only act together with another setting or a command-line mode (for example a trace mode or the DLSS path). The row says so where the code does. The rows come from reading the code, and most effects have not been checked in a run.

`tests/settings_doc_check.py` fails when a name in `known[]` (`runtime/cfg.c`) is described neither here nor in `totk.ini.example` or `kalamar.ini.example`, or when a default here differs from the code.

## The Esc menu on the Splatoon 3 target

Other targets than the upstream game get the ink layout of the Esc menu (`runtime/ui/menu.cpp`, `ui/option_rows.cpp`, `ui/ink.h`): five tabs instead of nine. The upstream target keeps its tabs and every one of its rows. A setting is shown when the code path behind it runs on this target, whatever its name.

Shown:

- Aim & controls: `mouse_gyro`, `mouse_gyro_sens` (a stepped scale, Shift + left or right for 0.001 steps), `mouse_gyro_x`, `mouse_gyro_y`, `mouse_gyro_invert_y`, `mouse_gyro_pitch`, `mouse_gyro_recenter_key`; the key and button table with this game's function beside each button; `walk_key`, `walk_scale`; `pad_layout`, `motion`, the rumble settings and the stick dead zones.
- Display: `fullscreen`, `window_size`, `aspect`, `vk_present`, `fps_limit` (30 or 60: the 11.3.0 patch set `fps60` switches the game's own 60 fps mode on every screen, the hub included, at the console's speed; 30 turns it off. Above 60 needs the real-time step, which this target has no patch for, and the log says so), `upscaler`, `sharpness`, `aniso`, the color settings, `gpu_index`, `shadow` ("smoother loading", about 1.2 GB of RAM), and a place for read-only rows "forced by your graphics driver".
- Audio: `volume`, `audio_channels`. Missing: an output device choice and muting when the window loses focus (the runtime has neither).
- Game: `lang`, `operation_mode`, `assume_focus`, `perf_overlay`, `save_profile_backups`, the play time, `menu_lang` and `menu_anim`; `discord_rpc` (the Discord presence, below) only when a Discord application is named and the game was not started by the launcher. Missing: a screenshots key (the runtime has none; `shots` and the input script's `SHOT` are test hooks).
- Advanced: open the log folder, copy system info, reset all settings.

Not shown (still honored from the file or the environment). Reason in each case, from the code:

- `output_res`, `render_scale`, `shadow_res`, `shadow_range`, `fov`: the log of this target says "the generated C has no output and scene patch sets", "no cascade patch set", "no field-of-view patch; KLM_FOV=120 does nothing": `recomp/patches-splatoon3-v1120.txt` has no patch sets.
- `fps60`, `realtime`, `cloth_realtime`, `highfps_fix`, `patch_*`: the same patch sets.
- `shadow_scale`, `pcss`, `contact_shadows`: they recognize a shadow-map or G-buffer shape of the upstream game; no evidence that they match anything here.
- `hdr*`, `dlaa`, `dlss*`, `fxaa`: the HDR tonemap and the DLSS inputs are found by the upstream game's shader and motion-vector passes.
- `movie_aspect`: this target links no libmovie. `prompts`: the button prompts are an overlay of the upstream game's fonts, which this target does not have.
- `mouse_mode`, `mouse_aim_scale`, `stick_sens`, `menu_cursor`: hooks in the upstream game's camera and menus; this target's mouse is the motion sensor.
- `discord_app_id`, `discord_image`: written by the launcher (below), not chosen in the menu. The Mods, Cheats and Amiibo tabs and the scheduling, shader and texture rows of the Advanced tab belong to the upstream game's tooling and are not offered here.

`menu_anim` (default `1`) turns the short open, close and selection animations of the ink layout off. A row's Delete key (X on a gamepad) puts its setting back to the default.

`menu_lang` is the language of the menu and of the performance overlay: `en`, `fr`, `ru`, `pt-BR`, `zh-Hans`, `es` or `ja`; empty (the default) follows the Windows user language (`ru-RU` finds `ru`, `es-MX` finds `es`, `zh-CN` finds `zh-Hans`) and shows English for a language with no menu text. It is independent of `lang`, the language of the game. Spanish (neutral) and Japanese have every menu string; Chinese, Russian and Brazilian Portuguese too, French only the rows of the ink layout (the rest falls back to English). The probe (`--selftest`, `probe_menu_text` in `runtime/ov_probe.c`) fails when Spanish, Japanese, Chinese, Russian or Brazilian Portuguese lacks a menu string or changes the `printf` conversions of the English one. Japanese, like Chinese and Korean, is drawn with a Windows system font read from `C:\Windows\Fonts` when the menu is built (Yu Gothic `YuGothM.ttc`, else Meiryo, else MS Gothic; nothing is embedded): the font of the menu's own language is merged first so that Han characters shared with Chinese keep their Japanese shapes, and the font atlas is made again when `menu_lang` changes. With no Japanese system font the menu stays in English and the log says so (`menu: Japanese menu text needs a system font`).

### Discord presence on the Splatoon 3 target

`runtime/discord_rpc.c`; the launcher's side is its `docs/discord-presence.md`. The launcher is the one source of the
presence: it knows the room, the search, the match and the stands, which the game does not.

- **Started by the launcher**: the launcher passes `KLM_DISCORD_PRESENCE=launcher` to every game it starts. The game
  then never starts the presence thread and never opens Discord's pipe, whatever `discord_rpc` says; the log has one
  line, `discord: started by the launcher, which shows the Discord presence: this game stays quiet`, and the Game tab
  has no Discord row.
- **Started on its own**: `discord_rpc` (default `0`) and `discord_app_id` decide, the same two keys the launcher writes
  into `kalamar.ini` (section `[discord]`) each time the player switches its presence on or off, and at each launch.
  There is no default application on this target (the upstream default names the upstream game's): without
  `discord_app_id` the thread ends at once and nothing is ever opened. The activity is the launcher's minimal level:
  "In game" in the menu language (`UI_INK_DISCORD_IN_GAME`, the launcher's words), the picture `kalamar_logo` (or
  `discord_image` when set) with the brand's name as its hover text, and the time since the game started. No version,
  path or name. The game also takes the lock `Kalamar.DiscordPresence` (`plat_named_lock_try`: the named mutex
  `Local\Kalamar.DiscordPresence`, an flock on Linux) that the launcher takes: while a launcher shows the presence, the
  game does not, and the other way round.
- `KLM_DISCORD_IPC=<name>` (tests only): the pipes are `<name>-0..9`, a stand-in's, instead of `discord-ipc-0..9`. A
  value that is not a plain name (letters, digits, `-`, `_`, `.`) opens no pipe at all. `tests/discord_ipc_check.py`
  runs `--probe-discord` against such a stand-in, with and without `KLM_DISCORD_PRESENCE=launcher`.

## Diagnostics and logging

| Setting | What it does | Effect of changing it | Safety |
| --- | --- | --- | --- |
| `astc_time` | `1` times the GPU dispatches of each ASTC texture decode and logs the time per block footprint and the largest call. | Adds GPU timestamp queries around every decode; if the queue has no timestamps or no `hostQueryReset`, the log says the decodes are not timed and nothing is measured. | Caution |
| `bind_stats` | `1` counts bindings and descriptors per draw of each stage and how much of the bound state changes between draws. | Adds per-draw bookkeeping to the render thread and extra log output; frames get slower. | Caution |
| `contact_debug` | `1` draws the contact-shadow factor in gray in place of the picture. | The frame shows a gray debug image while it is set; has no effect when contact shadows are off. | Caution |
| `core_diag` | `1` records why ready guest threads wait for a virtual core and for how long, with a histogram every 60 presents. | Turns on host-call and SVC depth tracking and writes many log lines (one per present over 25 ms); frames get slower. | Caution |
| `fn_trace` | Splatoon 3: comma-separated offsets of functions in `main` (hex); every call of one of them is logged with its caller, x0 to x4, the strings x0 to x3 point to (directly or through one pointer, as scene names are passed) and the guest frame-pointer chain, up to 400 lines per function. `runtime/scene_start.c`. | Every lifted function checks the list on entry while it is set, so the game runs slower; only adds log lines. | Caution |
| `fn_trace_depth` (default `8`) | Number of guest stack frames `fn_trace` logs per call, 0 to 32. | Longer or shorter log lines; has no effect without `fn_trace`. | Safe |
| `menu_cursor_log` | `1` logs what the menu cursor code reads at every screen change. | Only adds log lines. | Safe |
| `mouse_gyro_attitude` (default `flat`) | How the virtual controller of `mouse_gyro` starts: `flat` (face up, top edge forward) or `upright` (face toward the player, top edge up, as in handheld mode). | Changes the direction matrix and gravity the game reads; the angular velocity it integrates is the same turn about the world's vertical axis. | Safe |
| `perf_slow_ms` (default `50`) | Frames at least this many milliseconds long get a `perf: slow frame` log line with the time blocked per cause. | Changes how many frames are logged; a small value fills the log. | Safe |
| `save_trace` | Splatoon 3: a folder (`1`: `savetrace` in the working directory) where every call `main` makes to the `nn::crypto` functions it imports, every file call on the save, cache and temporary storage, and the opens of the RomFS scene packs (`Pack/Scene/*`) are logged in `trace.log`, with their arguments and guest call stacks, and their buffers of more than 64 bytes are written as `<seq>-<function>-<role>.bin`. `runtime/savetrace.c`. | The folder holds data of the player's own save: keep it with the player's own data, never share it. A save write adds about 1100 log lines. | Caution |
| `replay_check` | `1` reads every texture read-back a second time after a full drain of the replay queue and compares the two. | Stalls the replay queue at every read-back, so frames get much slower; mismatches are only counted and logged. | Caution |
| `resolve_check` | `1` compares every cached pool lookup with the locked lookup and stops on a difference. | Slows every pool lookup; the game stops if the two ever differ. | Caution |
| `speed_probe` | `1` adds the player's position and ground speed to the frame-rate line that `KLM_TRACE=perf` prints every 120 presents. | Only adds log text; has no effect without `KLM_TRACE=perf`. | Safe |

## Profiling and measurement

| Setting | What it does | Effect of changing it | Safety |
| --- | --- | --- | --- |
| `ab` | Splits game time into blocks of this many seconds in the order A B B A and lets settings written as `ab` take effect only in the B blocks. | Settings that accept `ab` flip on and off every few seconds, so frame times and the picture change with the block; without such a setting it does nothing. | Caution |
| `crit` | `<first>-<last>` frame range (with `KLM_TRACE=sched`) that records every state change and wake of the guest threads, then writes a file at the last frame. | Recording adds overhead to every thread switch in the range and writes one file; `tools/crit_path.py` reads it. | Caution |
| `crit_out` (default `crit.bin`) | Path of the file that `crit` writes. | Only moves the file; nothing is written without `crit`. | Safe |
| `profile` | Path of a sampling profile file; the profiler suspends threads to unwind their stacks. | Frames get slower while it samples and the profile file grows; `tools/prof_report.py` reads it. | Caution |
| `profile_chains` | `1` keeps call chains of every thread in the profile. | Larger profile file and more work per sample; has no effect without `profile`. | Caution |
| `profile_frames` | `<lo>-<hi>` samples only while presents lo to hi-1 are pending; `w<lo>-<hi>` uses world-clock seconds. | Limits the profile to a window; has no effect without `profile`. | Safe |
| `profile_main` | `1` samples only the guest main thread. | Narrows the profile; has no effect without `profile`. | Safe |
| `profile_render` | `1` samples only the render thread and keeps its call chains. | Narrows the profile and makes it larger; has no effect without `profile`. | Caution |

## Capture and frame dumps

| Setting | What it does | Effect of changing it | Safety |
| --- | --- | --- | --- |
| `dump` | Frame number to dump: every render pass target and storage image of that frame is copied out as PNG, with `passes.txt` and `images.txt`. | Writes many PNG files and shader hashes to disk and slows the dumped frame; size is capped by the dump code. | Caution |
| `dump_buf` | `<addr>:<size>[:<off>:<stride>:<w>:<h>]` snapshots one GPU buffer range after every logged pass of a dump. | Writes `buf_<pass>.bin` files and `bufs.txt`; has no effect without `dump`. | Caution |
| `dump_dir` (default `dump`) | Folder that receives the frame dump files. | Only moves the output. | Safe |
| `dump_log` | Frame number; logs the frames from there up to the dumped frame into `passes.txt`, with PNGs only of targets up to 64K texels. | Writes a longer `passes.txt` and more PNGs; has no effect without `dump`. | Caution |
| `flight_at` | Comma-separated frame numbers that trigger a flight-recorder dump. | Writes a dump folder when each frame is reached; has no effect unless the flight recorder (`KLM_FLIGHT=1`) is on. | Caution |
| `flight_dir` | Folder for flight-recorder dumps; the default is `flight` beside the folder of `totk.exe`. | Only moves the output. | Safe |
| `flight_frames` (default `600`) | Number of presents the flight recorder keeps. | A larger ring keeps more history and more thumbnails in memory; has no effect unless `KLM_FLIGHT=1`. | Caution |
| `flight_log` | Path of a TSV file that gets every flight-recorder record as it is made: one line per present with the interval, the present interval the game asked for, the world clock, the GPU time read since the previous present (with `KLM_TRACE=perf`), the replay time, the render thread's blocked time per cause and the frame's counts (`runtime/flight.c`). | One buffered line per present, flushed every 60 presents; has no effect unless `KLM_FLIGHT=1`. | Safe |
| `flight_thumbs` (default `1`) | `0` turns off the 320x180 thumbnail the flight recorder saves for each present. | Dumps hold only `frames.tsv`; thumbnails cost a GPU blit per present when on. | Safe |
| `rdoc` | Capture mode for RenderDoc: any value but `0`. A comma-separated list of frame numbers (`<n>`) and world-clock seconds (`w<s>`) names the frames to capture when the game was started by `renderdoccmd capture` or from qrenderdoc; another value (`keys`) leaves the captures to RenderDoc's F12 and Print Screen keys. RenderDoc is never loaded by the game. | The CPU-visible pools get shadow storage instead of host-memory imports (`shadow`), so the device needs no `VK_EXT_external_memory_host`, which RenderDoc hides; the CPU's writes are copied in before each submit and the GPU's copied back at its fence, which costs time. Without RenderDoc in the process only that changes. | Caution |
| `rdoc_dir` | Output folder for RenderDoc captures; unset, the template of the launcher (`renderdoccmd capture -c`) is kept. | Only moves the output. | Safe |
| `shots` | `<lo>-<hi>:<step>` saves every step-th present in the range as `f<n>.png`, scaled to 480x270; `w<lo>-<hi>:<step>` takes them by world-clock seconds. | Writes one PNG per step and adds a readback of the window texture; the pictures clip highlights in HDR. | Caution |
| `shots_dir` (default `.`) | Folder for the `shots` pictures; it is created when needed. | Only moves the output. | Safe |
| `shots_full` | Set to any value to save `shots` at full size instead of 480x270. | Files get far larger and writing them takes longer; has no effect without `shots`. | Caution |

## Automated tests and test hooks

| Setting | What it does | Effect of changing it | Safety |
| --- | --- | --- | --- |
| `vram_limit_mib` | Pretends the device-local memory budget is at most this many MiB, for the paths that react to the budget (`tex_vram_pressure`, mirror refusal); the driver knows nothing of it. | Lets a large card drive those paths; a low value drops released textures and refuses mirrors that would have fit. | Caution |
| `background_window` | `1` keeps the window behind the user's windows, never fullscreen at start, and F11 keeps its place; SDL windows ignore it. | The window does not come to the front or flash, so it is easy to miss; meant for unattended runs. | Caution |
| `gpu` | `vulkan` or `null`: the graphics back end; `null` has no GPU, completes every submit at once and returns zeros for texture reads. | `null` shows no picture; any other value stops the start with an error. | Caution |
| `input` | `none` reports no input from the keyboard, mouse or controllers. | The game gets no controller input and the menu cursor stops following the mouse; `input_script` still applies. | Caution |
| `input_live` | Path of a file a test harness appends lines to while the game runs, read every 50 ms: `[+<ticks>] <button> [hold]` and `[+<ticks>] LSTICK or RSTICK <x> <y> [hold]` as in `input_script` but counted from the moment they are read, and `SHOT <path.png>`, which saves the next presented frame at full size. Works without window focus, so one harness can steer several instances. | The game presses those buttons by itself and writes the pictures asked for; a line still being written waits for its end. | Caution |
| `input_script` | Path of a file with lines `<frame> <button> [hold]` or `<frame> LSTICK or RSTICK <x> <y> [hold]` that are ORed into the real input. | The game presses those buttons by itself; a file that cannot be opened is logged and ignored. | Caution |
| `mgyro_bench` | `all` or a comma-separated list of `look idle lin step phase rate rt fields pitchyaw yawaxis reverse cap recenter limits hitch fps image`: the mouse gyro's test bench ([controls.md](controls.md), "Mouse as the gyro"). A simulated mouse turns the virtual controller, the game's aim is read from its memory and every six-axis read goes to a CSV file. | The player's own input is ignored while it runs and the aim turns by itself; `fps` changes the frame limit for a while. | Caution |
| `mgyro_bench_out` (default `mgyro-bench.csv`) | The bench's CSV file; its screenshots (`image`) go to the folder of the same name with `-shots` added. | Only moves the output. | Safe |
| `mgyro_bench_delay` (default `40`) | Seconds between the first live six-axis read and the bench's start, for the scene to load. | Only moves the start. | Safe |
| `mgyro_bench_exit` | `1` closes the game when the bench is done. | The game closes without a save. | Caution |
| `menu_test` | `<frame>[:<frames per page>]` opens the menu at that frame, shows each tab for 30 frames (or the given count), saves one screenshot per tab and closes the menu. | The menu opens by itself and screenshots are written. | Caution |
| `menu_test_dir` | Folder for the `menu_test` screenshots; the default is the working directory. | Only moves the output. | Safe |
| `menu_test_set` | `<frame>:<key>=<value>[,...][;...]` changes settings live at that frame, with or without the menu open; `SAVE=1` also writes `totk.ini`. | Settings change during play without input, and `SAVE=1` rewrites the file. | Caution |
| `probe_movie` | Text that picks the movie for `totk.exe --probe-movie`: the first file whose path contains it. | Only used by the movie decoder probe; ignored in normal play. | Safe |
| `probe_movie_shot` | Path of a `.ppm` file that the movie probe writes the middle frame to. | Only used by the movie decoder probe. | Safe |
| `probe_movie_bench` | `1` makes the movie probe also time the resampler that fits a movie to a game resolution of another shape, and print the time per frame. | Only used by the movie decoder probe. | Safe |
| `swkbd_answer` | `<text>`, `<cancel>` or `<initial>`: answers the on-screen keyboard (for example a horse name) without opening a window. | The game gets that answer every time it asks for text. | Caution |
| `swkbd_test` | `ok` or `cancel` opens the keyboard window and presses OK or Cancel on the first timer tick. | The keyboard window closes by itself. | Caution |
| `test_oom` | `<site>:<n>[,...]` fails the first n allocations at a site: `commit` (guest memory), `staging` (upload buffers), `image` (texture images), `chunk` (draw data buffers). | Each forced failure takes the out-of-memory path: a memory release, a wait of 10 ms per retry and log lines; a large n stalls up to 30 s and then ends the game. | Caution |
| `test_pad` | `<lx>,<ly>,<rx>,<ry>` attaches a virtual gamepad whose sticks rest at these raw SDL axis values (-32768 to 32767). | The game sees a controller with that drift or deflection; a malformed value is logged and ignored. | Caution |
| `test_prepare_shaders` | `1` translates every recorded shader on all usable CPUs after the start's prewarm. | It runs behind the game unless `prewarm_wait = full`, which makes the start wait for it; it fills the shader cache folder, and the CPU is fully used while it runs. | Caution |

## Input

| Setting | What it does | Effect of changing it | Safety |
| --- | --- | --- | --- |
| `bind_<button>` | Remaps one Switch button or stick direction to `pad:<name>` or `key:<name>` entries; covers `bind_a`, `bind_b`, `bind_x`, `bind_y`, `bind_l`, `bind_r`, `bind_zl`, `bind_zr`, `bind_plus`, `bind_minus`, `bind_left`, `bind_up`, `bind_right`, `bind_down`, `bind_lclick`, `bind_rclick`, `bind_lstick_left`, `bind_lstick_up`, `bind_lstick_right`, `bind_lstick_down`, `bind_rstick_left`, `bind_rstick_up`, `bind_rstick_right`, `bind_rstick_down` and `bind_shake`. | A line replaces every default of that button; button prompts in the game still show the default layout. The Controls tab of the menu writes these lines; `docs/controls.md` lists the entry names. | Safe |

## Paths and files

| Setting | What it does | Effect of changing it | Safety |
| --- | --- | --- | --- |
| `amiibo_dir` | Folder the Amiibo tab starts in; the tab saves the folder you last browsed here. | Only changes which folder the tab lists first. | Safe |
| `exefs` | Folder of the game's ExeFS (with `main.npdm`); a relative path is relative to `totk.exe`'s folder. | Without it, an `exefs` folder next to `totk.exe` is used, else the path set at build time. The game code is compiled into `totk.exe`, and the loader checks the files here by SHA-256, so files from another build or game version stop the start with a message. | Caution |
| `save_profile_backups` (default `10`) | Splatoon 3: how many backups of the save `save_profile` keeps in `save-profile-backup` (one per launch that changed the save); `0` keeps all. Older ones are deleted by their `yyyymmdd-hhmmss` folder name, only in that folder, no link followed. | A smaller number deletes older backups at the next launch that changes the save; has no effect without `save_profile`. | Save risk |
| `mods_dir` (default `mods`) | Folder that holds one subfolder per player mod, beside `totk.exe`. | Mods found there replace game files; see `docs/mods.md`. | Caution |
| `mods_disabled` | Names of mods that are switched off, separated by `\|`. | Listed mods are not applied; the Mods tab writes this line. | Safe |
| `mods_order` | Mod names, top first, separated by `\|`; the top mod wins a file that two mods replace. | Changes which mod's file the game gets; unlisted mods go below the listed ones, by name. | Safe |
| `pipeline_cache` | Path of the Vulkan pipeline cache file (default `pipeline-cache.bin` beside `totk.exe`). | Moves the cache; a new path starts empty and pipelines are built again. | Caution |
| `profile_dir` | Folder of the game profile, which holds the user account id that save data is named after; the default is a `profile` folder beside `totk.exe`, else the build-time path. | Another folder means a different set of saves. | Save risk |
| `romfs` | Folder of the game's RomFS (with `romfs.bin`); a relative path is relative to `totk.exe`'s folder. | Without it, a `romfs` folder next to `totk.exe` is used, else the build-time path; a missing image stops the start with a message. | Save risk |
| `romfs_index` | Path of `romfs-index.bin`, which `tools/romfs_index.py` wrote; the default is beside `totk.exe`. | The index is checked against the image by SHA-256 and size; a mismatch stops the start. | Caution |
| `romfs_mmap` | `1` maps the RomFS image into memory instead of reading it with positional reads. | Changes how the image is read; the log reports when the mapped path is on. | Caution |
| `romfs_mods` | Folder of an overlay with a `manifest.tsv` of sizes and SHA-256 hashes; its files are served in place of the originals. | A missing file, size or hash mismatch, or a changed original stops the start; player mods win over this overlay. | Save risk |
| `romfs_patches` | Folder of an overlay with a `manifest.tsv`, served in place of the original files; the default is `romfs-patches` beside the index. | Same checks as `romfs_mods`; while it is set the Xbox and PC button prompts are off, and the menu says so. | Save risk |
| `romfs_proof_dir` | Folder for the `.romfs-verified-v1` proof file of a full verification; the default is beside the index. | Only moves one small file. | Safe |
| `romfs_verify` | `full` hashes the whole RomFS image at start; `force` hashes it again even when a proof file exists. | Start takes many minutes for a 17 GB image; a proof file is written so later starts skip the hash. | Caution |
| `shader_cache` | Folder of the translated shader cache (default `shader-cache` beside `totk.exe`). New translations are written as loose files (four per shader); after the start's prewarm the runtime folds them into `shader-cache.pack.next` when there are 256 or more of them or no pack yet, on a background-priority thread, and the next start renames it over `shader-cache.pack` and deletes the loose files the pack holds. A start reads the pack's shaders with a few large sequential reads instead of two file opens per shader. | A new folder starts empty, so shaders are translated again during play. | Caution |
| `shared_fonts` | Folder of the extracted system fonts; the default is `shared-fonts` beside `totk.exe`, else beside the RomFS. | Simplified Chinese, Traditional Chinese and Korean need these fonts: without them the game exits before its first frame in those languages. Other languages do not use them. | Caution |

## Shader translation

| Setting | What it does | Effect of changing it | Safety |
| --- | --- | --- | --- |
| `xlate` | Path of the shader translator program; the default is `totk_xlate` beside `totk.exe`, else under `build/xlate`. | Shaders are translated by that program; if it is missing the log says so and graphics shaders cannot be translated. | Risky |
| `xlate_id` | Text or 64 hex digits that identify the translator build in shader cache keys; the default comes from an `.id` file beside the translator, else a hash of the program. | A value that two different builds share lets the cache serve shaders translated by the other build. | Caution |
| `xlate_keep` (default `0`) | `1` keeps the translation job of a last-frame variant as `<key>.job` with its code. | Writes extra files to the shader cache folder; meant for running the translator by hand. | Caution |
| `xlate_procs` (default `2`) | Number of translator processes allowed during play. | More processes translate new shaders sooner and use more CPU and memory; values below 1 count as 1. | Caution |

## Graphics experiments

| Setting | What it does | Effect of changing it | Safety |
| --- | --- | --- | --- |
| `bc_standins` (default `1`) | Draws into a 2D block-compressed image without block views (`block_views` `0`) go to an uncompressed image of its blocks, copied from the texture before each pass and into it after; `0` leaves them out. | `0` leaves the textures the game encodes on the GPU empty: with block views off, the player's bare skin is not drawn (its body mask is one of them). | Caution |
| `block_views` (default `1`; `0` on AMD's Windows driver for RDNA 2 and older, RX 400 to RX 6000) | `0` makes 2D block-compressed images without block views; the game's draws into them go through stand-ins (`bc_standins`). | With `1` on an RX 6600, GPU writes past the end of such an image's memory put noise in other textures and lose the Vulkan device, and the game stops; the older cards on the same driver are assumed to do the same. `--probe-blockviews` shows in a few seconds whether a card does this. | Risky |
| `vb_align` (default `1`; `0` on NVIDIA) | A vertex stream whose stride is no multiple of the component size of its attributes (Splatoon 3's stage signs and stickers: 69-byte vertices with floats) is drawn from a copy with the stride rounded up to 4 bytes, made for each submission; `0` binds it in place. | With `0` on an RX 6600 those draws come out as gray planes across the stage. | Safe |
| `vk_recorder` (default `1`; `0` on AMD's Windows driver) | `1` makes the replay's Vulkan calls on a recorder thread of their own (`vk_rec.c`), which also submits them without the replay waiting (`rec_async`); `0` makes them on the replay thread. | Upstream 0.4.0 turned it on for AMD GPUs again, after device losses and GPU hangs at start on RDNA 2 to 4 had kept it off there; this fork keeps it off on AMD's Windows driver (both its branches) until it has been run with Splatoon 3 there. `1` turns it on there; on any GPU, `0` if the screen stays black or the device is lost at start. | Caution |
| `block_views_3d` (default `0`) | `1` gives 3D block-compressed images block views too. | On some AMD cards a GPU write past the end of the image's memory loses the Vulkan device, and the game stops. | Risky |
| `deswizzle` (default `auto`) | Where block-linear textures are deswizzled: `auto` or `gpu` uses a compute pass where the texture's storage is an imported pool and the CPU elsewhere, `cpu` always uses the CPU, `check` uses the GPU and also deswizzles every upload on the CPU and compares the two. | `cpu` brings back the long frames on entering new areas (the replay thread deswizzles every texture); `check` waits for the GPU at every upload and is for testing only. The log names the mode in use. | Caution |
| `dyn_delta` (default `1`) | Records only the pipeline dynamic states (cull mode, depth and stencil tests, vertex input, blend enables, equations and write masks, ...) whose values changed since the last draw in the command buffer, also across pipeline changes; `0` records the full set at every change of pipeline state, as before; `ab` alternates with the `ab` blocks. | Fewer commands per pipeline change on the render and recorder threads; a driver that loses dynamic state across pipeline binds would draw with stale state. `KLM_TRACE=perf` prints the calls and commands per submission. | Caution |
| `feedback` | `0` samples a target that an earlier draw of the same pass wrote with no barrier; `restart` ends and restarts the pass behind a full barrier; otherwise the feedback-loop layout is used when the GPU supports it. | `0` brings back the old behavior that the barrier fixed, so the picture can break in passes that read a target they draw into; `restart` costs extra passes. | Risky |
| `flush_span` (default `0`) | `1` copies one span from the lowest to the highest flushed address of a pool, as an older version did. | Copies far more data to the GPU in crowded towns: an estimated 30-50 ms of GPU time per large copy over PCIe 3.0 x8. | Caution |
| `shadow_snap` (default `1`) | Shadow storage (Linux, Wine): `1` syncs the pools from per-submission write-watch snapshots, each 2 MiB region queried at most once per submission and shared with the texture checks; `0` queries each pool at every part of a submission, as before. | `1` cut the replay thread's write tracking from about 4.4 to 1.3 ms a frame on a Steam Deck; `0` is for comparison runs. Under Wine/Proton the default is `0` until it has been measured there; `1` turns it on. No effect where shadow storage is off (Windows by default). | Caution |
| `freeze_idle` (default `60`) | Number of submissions a 2 MiB region stays unwritten before it is made read-only; `0` turns the freeze off. | A larger value keeps more regions writable and costs more write tracking; a write to a frozen region costs about 60 microseconds. | Caution |
| `gpl_draw_build` (default `1`) | `0` leaves the missing shader libraries of a waiting draw to its background job instead of building them in parallel at once. | Draws whose pipeline is not ready are skipped meanwhile, until the job finishes. | Caution |
| `lib_prebuild_keep` (default `0`) | Number of recorded graphics shader library variants that stay in memory after the boot prebuild. | Each kept library costs 140-460 KB of memory; the driver cache already makes a rebuild cheap. | Caution |
| `minimal_layout` (default `1`) | `0` skips the compare against one-block tiles when a texture's memory layout is chosen. | The code calls it a negative control for fresh sessions; the comment does not say what the picture does. | Unknown |
| `mirror_gap` (default `2`) | Hot pools at most this many 2 MiB regions apart share one write-watch query. | Changes the number of queries per submission; negative values count as 0. | Caution |
| `movie_host` (default `1`) | `0` leaves every movie decoder to the game's own library. | The code comment says that library aborts at the first movie, so the game stops at the first cutscene. | Risky |
| `movie_pace` (default `1`) | `0` hands every decoded movie frame over at the next present instead of when it is due. | Movies play at the game's frame rate rather than their own, so their speed no longer matches the sound. | Caution |
| `pcss_k` (default `0.5`) | Depth-to-penumbra factor of soft shadows at 100 percent, for tuning. | Changes how wide soft shadows blur; has no effect while `pcss` is `0`. | Caution |
| `pipe_sync` | Any value other than `0` or empty stops graphics pipelines from being built in the background. | Pipelines are built on the render path, which causes stutter when new effects appear. | Caution |
| `push_stage` | `fs` pushes the descriptor set from the fragment stage instead of the vertex stage. | The log names the stage in use; the code does not say what changes in the picture. | Unknown |
| `rec_async` (default `1`) | `0` makes the submitting thread wait until the recorder thread has recorded and submitted each part; `ab` alternates with the `ab` blocks. | With `0` the submitting thread waits for every part (about 0.3 ms a frame in a town on a Ryzen 7900X). | Caution |
| `rec_end` (default `1`) | `0` lets the submitting thread begin, end and submit a recorded part again, as older versions did. | On some AMD cards the GPU hangs or the scene turns green and then white when a submission is split. | Risky |
| `rec_spin_us` (default `60`; `1` for Splatoon 3 except on AMD GPUs) | Microseconds the recorder thread polls for the next packets before it sleeps; `0` polls 4096 pauses instead; `ab` polls only in the B blocks of `ab`. With fewer than 6 logical processors it always polls 4096 pauses. For Splatoon 3 (except on AMD GPUs) the replay also hands packets over in batches of 8192 words instead of 64 (`KLM_REC_PUB`, environment only), as with fewer than 6 processors. | A longer poll saves an OS wake-up per packet and takes more CPU time; a shorter one sleeps sooner. In a Splatoon 3 stage at 60 fps (Ryzen 7 3700X, RTX 3060) the recorder took 0.29 to 0.34 cores with `60` and batches of 64 words, 0.11 with `1` and 8192, with the same frame times. | Caution |
| `tex_noaccess_static` (default `1`) | The mirrored textures of CPU_NO_ACCESS pools (block-compressed, with the content the game wrote before it made the pool) are uploaded once and never checked for CPU writes again; `0` checks them at every submission like the others; `ab` alternates with the `ab` blocks. | With `0` their 2 MiB regions are queried by the write watch until they freeze: in Splatoon 3's plaza right after its load, a third more queries per frame, and slower frames. | Caution |
| `tex_vram_pressure` (default `90`) | Percent of the device-local memory budget above which released textures are dropped, 0 to 100; `0` is off. First those the game has overwritten with a later texture and left alone for 30 s, then those that can be uploaded again. | A low value drops textures sooner and re-uploads them; `0` keeps them and lets the card spill into system memory. Also a slider in the menu. | Caution |
| `txd_transpose` (default `1`) | `0` turns off the swap of gradient components for gradient texture samples. | Terrain textures turned across the screen show crawling patches again. | Caution |
| `ub_overlay` (default `1`) | `0` copies each uniform update inside a pass into its buffer at once, ending the pass with a full barrier; `ab` alternates with the `ab` blocks. | With `0` scenes with many updated blocks (the terrain, the sky islands) take longer on the GPU; `KLM_TRACE=perf` prints how many updates stayed in their pass. | Caution |
| `ub_overlay_out` (default `1`) | `0` copies each uniform update between passes into its buffer at once, behind a full barrier of its own; `ab` alternates with the `ab` blocks. Needs `ub_overlay`. | With `0` a run of updates before a pass costs a full GPU barrier each; `KLM_TRACE=perf` prints how many updates between passes were overlaid. | Caution |
| `ubo_snapshot` (default `1`) | `0` binds live memory for uniform blocks; `ab` alternates with the `ab` blocks. | Without snapshots late draws read another object's matrix, so objects can be drawn in the wrong place. | Risky |
| `vk_no_float64` (default `0`) | `1` clears the 64-bit float feature, so the translator lowers FP64 to FP32. | Shaders change, so the shader cache entries for it are made again; precision of FP64 shaders drops. | Caution |
| `vk_ssbo_align` (default unset) | Power of two that storage buffer offsets are treated as needing, to test the unaligned path. | The translator and the draw code round storage offsets down to it; shaders are translated again. | Caution |
| `vk_subgroup` (default `32`) | Subgroup size to pin where the driver allows it. | A value other than 32 changes what the translator is told about the GPU, and shaders are translated again. | Caution |
| `vram_headroom` (default `1`) | `0` turns off the spare half-block of video memory that a background thread keeps per memory type. | Texture allocations on the render thread can wait for a new block again. | Caution |
| `vs_outs` | `all` declares all 32 vertex shader output locations; otherwise only the union of the paired fragment inputs. | Larger vertex shaders and new pipelines; unwritten outputs read the same as before. | Caution |
| `wb_exact` (default `1`) | `0` decides write-back of GPU-written textures by page only. | A target that shares a page with data the CPU writes can go unwritten, so a name label next to the reticle can blink. | Caution |
| `wb_merge` (default `1`) | `0` copies GPU-written ranges back as plain copies instead of writing word by word the words that changed. | In mirror memory mode the GPU's copy can overwrite words the CPU wrote, which breaks effects such as falling rain and waterfall splashes. | Caution |

## Motion vectors and DLSS

| Setting | What it does | Effect of changing it | Safety |
| --- | --- | --- | --- |
| `dlaa_jitter` (default `1`) | `0` turns off the camera jitter fed to DLSS. | DLSS gets no jitter between frames; a test setting. | Caution |
| `dlaa_maskdebug` (default `0`) | `1` shows the reactive mask in place of the picture. | The frame shows a debug image while it is set. | Caution |
| `dlaa_mv` (default `1`) | `0` sends zero motion vectors to DLSS. | DLSS sees no motion, so moving content can smear; a test setting. | Caution |
| `dlaa_mvdebug` (default `0`) | `1` shows the motion vectors in place of the picture, x in red and y in green. | The frame shows a debug image while it is set. | Caution |
| `dlaa_survey` (default `0`) | `1` logs what is drawn into the light target that DLSS reads. | Adds log output and per-draw work. | Caution |
| `dlaa_times` (default `0`) | `1` times the DLSS passes on the GPU. | Adds GPU timestamps and log lines. | Caution |
| `dlss_reactive` (default `0`) | `1` builds a reactive mask for DLSS where it ran. | Changes how DLSS blends moving areas; a test setting. | Caution |
| `mv_check` | Frame interval; every that many DLSS frames copies the light target and motion images to host memory and logs how well the vectors explain the picture. | Costs a GPU-to-host copy and a comparison on the completion thread at each interval. | Caution |
| `mv_survey` (default `0`) | `1` notes the uniform blocks of each scene draw and compares them with those of earlier frames. | Adds per-draw work and log output. | Caution |
| `mvobj_all` (default `0`) | `1` also lists draws for object motion that read no object blocks (terrain and grass). | More draws are checked and listed, which costs time. | Caution |
| `mvobj_banks` (default `0`) | `1` logs which words of the other uniform banks changed for each scene draw. | Adds log output and per-draw work. | Caution |
| `mvobj_equal` (default `0`) | `1` uses an EQUAL depth compare for the object motion pass, for comparison. | Changes which pixels get object vectors; the code comment names the default as the one that hides objects behind nearer geometry. | Caution |
| `mvobj_exact` | Hex bank mask; only draws whose position uses exactly these banks get object motion. | Fewer draws get object vectors, so their motion falls back to the camera's. | Caution |
| `mvobj_exec` (default `1`) | `0` lists object motion draws but never records them. | Objects get camera motion only. | Caution |
| `mvobj_noc4` (default `0`) | `1` draws again positions that have no scene camera bank. | Those draws get this frame's view, so they can show wrong motion. | Caution |
| `mvobj_only` | Hex bank mask; only draws whose position reads no bank outside the mask get object motion. | Fewer draws get object vectors. | Caution |
| `mvobj_times` (default `0`) | `1` adds timings to the object-motion summary. | The clock reads themselves cost frame time. | Caution |
| `mvprev_check` (default `0`) | `1` verifies the stored first bytes of each previous-frame table entry. | The code comment measures 3.6 ms per frame. | Caution |
| `mvprev_grow` (default `1`) | `0` keeps only three banks in the previous-frame tables. | Object motion that depends on the other banks is no longer tracked. | Caution |
| `mvprev_skip` | Hex bank mask of banks left out of the previous-frame copy, replacing the default. | Different banks are tracked, which changes object motion. | Caution |
| `mvprev_trust` | Hex bank mask of banks whose found previous-frame copy is taken without the check. | A copy that does not match is used anyway, which can give wrong motion. | Caution |

## Scheduling and host

| Setting | What it does | Effect of changing it | Safety |
| --- | --- | --- | --- |
| `app_pool` | Size in bytes of the guest application memory pool; accepts decimal or `0x` hex. | The default depends on the resolution settings; `0` stops the start with a message, and too small a pool leaves the game without memory. | Risky |
| `audio_off` | Any value mixes audio without opening an output device. | The game runs in silence and reports two channels; same as `audio = none`. | Caution |
| `heap_commit` | `lazy` (the default on Windows) commits the game's heap 256 KiB at a time when a page is first read or written, instead of all of it when the game sets the heap size; `full` commits it all at once, as before. Linux and Wine commit it at once: there a commit costs nothing until a page is touched. | `lazy` keeps the 1.5 to 2 GiB of heap the game never touches out of the system's commit charge (RAM plus page file), which leaves room for other programs and for more instances of the game (Splatoon 3 in a stage: 3.7 GiB of process commit instead of 5.4); a chunk first touched costs a fault (in a stage's load about 4800 of them, 66 ms in all). It also shortens the long frames of a load, where the graphics driver walks the committed heap at every memory import. With `lazy`, a commit the system refuses later in play waits for memory and then ends the game, where `full` would have failed at start. | Caution |
| `kspin_us` (default `20`) | Microseconds a guest wait or a wait for a virtual core polls before it sleeps; `0` turns the poll off; `ab` polls only in the B blocks of `ab`. Off with fewer than 6 logical processors. | A wake within the poll costs no OS wake-up; a longer poll takes more CPU time from other threads. | Caution |
| `low_memory` | `auto` (the default) applies the low-memory defaults on a machine with at most 8 GiB of RAM or a GPU whose device-local memory is at most 4 GiB; `1` applies them anywhere, `0` never. They are `vram_headroom = 0`, `vma_block_mib = 32`, `tex_block_mib = 32` and `tex_released_mib = 64`; a key set in `totk.ini` or the environment wins over them. The log says at start whether they apply. | Less VRAM and commit for the same frames: Splatoon 3 on an RTX 3060 at 1080p held 902 MiB of VRAM instead of 1155 in a stage and 1189 instead of 1507 in the plaza, and as much less process commit (NVIDIA's and AMD's Windows drivers charge device memory to it), with the same frame times and loading frames there. A device-memory block is then made on the thread that needs it, which took 8 to 19 ms per block on other GPUs (`vram_headroom`); a scene change makes more released textures again. Not measured on AMD GPUs. | Caution |
| `mem_reserve_mib` (default `1536`) | MiB of system memory kept free for Windows, other programs and the game's own heap when the renderer sizes what it keeps in device memory, where device memory takes system memory (integrated GPUs, and discrete GPUs whose driver charges it to the process). | A larger value keeps fewer released textures and spare blocks when the system runs short; a smaller one lets the game use more before it holds back. | Caution |
| `one_per_core` (default `0`) | `1` keeps the process on the first logical processor of each physical core; `ab` switches it with the `ab` blocks. | Busy threads stop sharing a core with a sibling thread, which can help or hurt on the CPU; it only applies when the CPU has hyperthreads. | Caution |
| `pipe_keep_launches` (default `30`) | Launches after which a recorded pipeline (`pipeline-keys.bin`) that no draw asked for is dropped from the file at start; `0` keeps every one. `pipeline-use.bin` beside it holds, per record, the last launch a draw asked for it and the order of that first ask; a record without an entry (an older key file) counts as used in the current launch. The same file orders the boot prebuild: the pipelines the latest launches needed first are built first. | A smaller value keeps the file, and the prebuild every start runs behind the game, shorter; a dropped pipeline that a draw needs again is built at that draw (fast-linked at once where the driver has graphics pipeline libraries) and recorded again. | Caution |
| `pcore` (default `1`) | On a hybrid CPU (performance and efficiency cores), the game's main and render threads and the replay, recorder and present threads prefer the performance cores; `0` turns that off. | With `0` Windows may run those threads on efficiency cores; no effect on a CPU with one kind of core. The log says which cores were found. | Caution |
| `prewarm_threads` (default `1`) | Threads that build the recorded pipelines behind the game once it runs, at below-normal priority, beside the pipeline workers that take that work as well (2 to 4 of them, one kept for draws from 3 on). Until the game starts every logical processor builds, at normal priority. | More threads take more CPU time from the game for little gain: with a cold driver cache on a Ryzen 7 3700X and an RTX 3060 the first minute had 12 frames over 25 ms with 1, 19 with 2 and 18 to 36 with 4 (10 to 11 when the start waited for the whole prebuild), and the prebuild ended 21, 20 and 15 to 20 s after the start. A pipeline a draw needs is built ahead of the queue whatever the value, and the builder that has it is raised to normal priority until it is done. The log's `vk_pipeline: prebuild done` line gives the driver time per pipeline. | Caution |
| `prewarm_wait` (default `0`) | Seconds the start of the game waits at most for the prewarm: loading the shaders `pipeline-keys.bin` names (translating the ones the translator no longer has) and building its pipelines. `full` waits until the prewarm is done, as earlier versions always did. | With `0` the game starts at once and the prewarm goes on behind it; a pipeline a draw needs before it is built gets built first (fast-linked at once where the driver has graphics pipeline libraries). `full` holds the start for as long as the prewarm takes: 62 s on a player's hard disk with an RX 6600 (26 s of shader reads, 31 s of pipeline builds), 0.5 to 15 s on an NVMe drive with an RTX 3060 depending on the driver's own cache. The `boot: guest starts` line gives the wait and whether the prewarm went on behind the game. | Caution |
| `reexec` | Counter of address-space retries that the runtime sets itself when it starts the process again. | The counter is cleared when the window is recreated; a value of 20 or more stops the start with a fatal error. | Risky |
| `replay_chunks` (default `0`) | Most chunks a submission is cut into when `replay_workers` is set; `0` uses twice the number of recording threads. | Fewer chunks leave threads idle at the end of a submission, more add scan and submit work per chunk; no effect while `replay_workers` is `0`. | Caution |
| `replay_chunk_draws` (default `128`) | Least draws a chunk of the chunked replay holds before it may end at a pass. | Small values make many short chunks, large ones leave big submissions on few threads; no effect while `replay_workers` is `0`. | Caution |
| `replay_state_check` (default `0`) | `1` compares the replay state after the last chunk of a chunked submission with the state of the scan and logs differences. | Adds a comparison per submission and log lines on a mismatch; no effect while `replay_workers` is `0`. | Caution |
| `replay_skew_us` (default `0`) | Debug: the even chunks of a chunked submission wait this many microseconds before they record, so the chunk after each one records first. | Shows a recording that depends on which thread gets to a chunk first in every submission; slows the replay; no effect with fewer than 2 `replay_workers`. | Caution |
| `restart_wait` | Set by the runtime when it restarts itself; names the old process by id and creation time so the new process waits for it to exit. | Meant for the runtime's own use; the code does not describe a manual value. | Unknown |
| `spin` (default `1`) | `0` makes a guest thread that polls memory keep its virtual core. | Polling threads can starve the thread whose item they wait for, so frames can stall. | Caution |
| `spin_handoff_us` (default `50`) | Microseconds a polling wait lasts before it gives its virtual core to a waiting thread. | A shorter value hands cores over sooner; a longer one keeps them longer. | Caution |
| `tex_block_mib` (default `64`) | Block size in MiB of the pool that holds the game's textures; images of a quarter block or more get memory of their own. `0` puts textures in the default pools with the buffers. | Larger blocks hold more memory that no texture uses; smaller ones mean more allocations from the driver. | Caution |
| `tex_released_mib` (default `256`) | MiB of textures the game released that are kept for reuse; older ones beyond it are dropped. `0` keeps up to 2048 of them, whatever their size. | A larger value keeps more device memory and process commit; a smaller one makes the game create more textures again. | Caution |
| `vma_block_mib` (default `64`) | Block size in MiB that the Vulkan memory allocator takes from the driver for buffers and other images on heaps over 1 GiB; larger allocations than half a block get memory of their own. `0` uses the allocator's 256. | Larger blocks hold more memory that nothing uses; smaller ones mean more allocations from the driver. | Caution |

## Generated-code patches

These names are code patch sets built into the generated game code (`recomp/patches-*.txt`). `rt_patches_init` reads the environment name at start, then the resolution and mouse code switches some sets itself.

| Setting | What it does | Effect of changing it | Safety |
| --- | --- | --- | --- |
| `aspect_patch` (default `0`) | Patch set that feeds the camera the output's aspect ratio when it is not 16:9. | The resolution code switches this set on or off itself at start, so a value here is replaced; the code does not settle what a manual value does. | Unknown |
| `fixed_res_patch` (default `0`) | Patch set that holds the game's dynamic resolution at 100 percent (the scene tier). | The resolution code switches this set from `dynamic_res` (on at `0`); a manual value is replaced. | Unknown |
| `fov_patch` (default `0`) | Patch set that scales the camera's vertical view angle by the field-of-view setting. | The resolution code switches this set itself whenever the field of view is not 100 percent; a manual value is replaced. | Unknown |
| `mouse_direct_patch` (default `1`) | Patch set of the hooks in the game's camera code behind `mouse_mode = direct` on game 1.4.3. | `0` stops `mouse_mode = direct` from taking effect; other mouse modes are unaffected. | Safe |
| `patch_output` (default `0`) | Patch set that makes the game use the configured output size for its swapchain and display configuration. | The resolution code switches this set whenever the output differs from the base size; a manual value is replaced. | Unknown |
| `patch_scene` (default `0`) | Patch set that makes the game use the configured scene render size. | The resolution code switches this set from the render size setting; a manual value is replaced. | Unknown |

## Legacy names

The runtime's code names its variables by their older names: `TOTK_<NAME>` for the settings it took from the upstream
(*totk-pc-recomp*), `SPLAT3_<NAME>` for the launcher's and the automatic match's variables of the Splatoon 3 target. Every
variable is documented, and set by the launcher, as `KLM_<NAME>`; the older name is still read. When both are set,
`KLM_<NAME>` wins (`runtime/cfg.c`, `cfg_env`, the one function that reads them; `cfg_setenv` writes them). A key of the
settings file may carry either prefix (`klm_fov = 90`, `totk_fov = 90`, `fov = 90`). The variables of the code
generator (`TOTK_LIFT_JOBS`, `docs/building.md`) keep their names: the generator is not the runtime.
`tests/env_names_check.py` fails when a page names a variable by an older name, or when this table and the documented
variables differ.

| Name | Older name, still read |
|---|---|
| `KLM_AB` | `TOTK_AB` |
| `KLM_ASSUME_FOCUS` | `TOTK_ASSUME_FOCUS` |
| `KLM_ASTC` | `TOTK_ASTC` |
| `KLM_ASYNC_PREEMPT` | `TOTK_ASYNC_PREEMPT` |
| `KLM_BACKGROUND_WINDOW` | `TOTK_BACKGROUND_WINDOW` |
| `KLM_BIND_` | `TOTK_BIND_` |
| `KLM_CORE_DIAG` | `TOTK_CORE_DIAG` |
| `KLM_CRIT` | `TOTK_CRIT` |
| `KLM_CRIT_OUT` | `TOTK_CRIT_OUT` |
| `KLM_DISCORD_IPC` | `SPLAT3_DISCORD_IPC` |
| `KLM_DISCORD_PRESENCE` | `SPLAT3_DISCORD_PRESENCE` |
| `KLM_DLAA_TIMES` | `TOTK_DLAA_TIMES` |
| `KLM_DUMP` | `TOTK_DUMP` |
| `KLM_DUMP_BUF` | `TOTK_DUMP_BUF` |
| `KLM_DUMP_DIR` | `TOTK_DUMP_DIR` |
| `KLM_FEEDBACK` | `TOTK_FEEDBACK` |
| `KLM_FLIGHT` | `TOTK_FLIGHT` |
| `KLM_FLIGHT_AT` | `TOTK_FLIGHT_AT` |
| `KLM_FLIGHT_SPIKE_MS` | `TOTK_FLIGHT_SPIKE_MS` |
| `KLM_FN_CENSUS` | `TOTK_FN_CENSUS` |
| `KLM_FN_TRACE` | `TOTK_FN_TRACE` |
| `KLM_FOV` | `TOTK_FOV` |
| `KLM_FPS_LIMIT` | `TOTK_FPS_LIMIT` |
| `KLM_FREE_CORES` | `TOTK_FREE_CORES` |
| `KLM_FULLSCREEN` | `TOTK_FULLSCREEN` |
| `KLM_GATHER_BIAS` | `TOTK_GATHER_BIAS` |
| `KLM_GPU_INDEX` | `TOTK_GPU_INDEX` |
| `KLM_GPU_NAME` | `TOTK_GPU_NAME` |
| `KLM_INPUT` | `TOTK_INPUT` |
| `KLM_INPUT_LIVE` | `TOTK_INPUT_LIVE` |
| `KLM_INPUT_RECORD` | `TOTK_INPUT_RECORD` |
| `KLM_INPUT_SCRIPT` | `TOTK_INPUT_SCRIPT` |
| `KLM_LAN` | `SPLAT3_LAN` |
| `KLM_LAN_BIND` | `SPLAT3_LAN_BIND` |
| `KLM_LAN_NAME` | `SPLAT3_LAN_NAME` |
| `KLM_LAN_ROOM` | `SPLAT3_LAN_ROOM` |
| `KLM_LIST_LANG` | `SPLAT3_LIST_LANG` |
| `KLM_LSTICK_DEADZONE` | `TOTK_LSTICK_DEADZONE` |
| `KLM_MATCH` | `SPLAT3_MATCH` |
| `KLM_MATCH_CMD` | `SPLAT3_MATCH_CMD` |
| `KLM_MATCH_DIAG` | `SPLAT3_MATCH_DIAG` |
| `KLM_MATCH_EXIT` | `SPLAT3_MATCH_EXIT` |
| `KLM_MATCH_HOST` | `SPLAT3_MATCH_HOST` |
| `KLM_MATCH_ID` | `SPLAT3_MATCH_ID` |
| `KLM_MATCH_LOADOUT` | `SPLAT3_MATCH_LOADOUT` |
| `KLM_MATCH_ON_ERROR` | `SPLAT3_MATCH_ON_ERROR` |
| `KLM_MATCH_PLAY` | `SPLAT3_MATCH_PLAY` |
| `KLM_MATCH_RESULT` | `SPLAT3_MATCH_RESULT` |
| `KLM_MATCH_REVEAL` | `SPLAT3_MATCH_REVEAL` |
| `KLM_MATCH_RULES` | `SPLAT3_MATCH_RULES` |
| `KLM_MATCH_SHOTS` | `SPLAT3_MATCH_SHOTS` |
| `KLM_MATCH_SILENT` | `SPLAT3_MATCH_SILENT` |
| `KLM_MATCH_STAGES` | `SPLAT3_MATCH_STAGES` |
| `KLM_MATCH_STATUS` | `SPLAT3_MATCH_STATUS` |
| `KLM_MATCH_TIMEOUTS` | `SPLAT3_MATCH_TIMEOUTS` |
| `KLM_MATCH_WAIT` | `SPLAT3_MATCH_WAIT` |
| `KLM_MENU_CURSOR` | `TOTK_MENU_CURSOR` |
| `KLM_MENU_CURSOR_LOG` | `TOTK_MENU_CURSOR_LOG` |
| `KLM_MOUSE_SENS` | `TOTK_MOUSE_SENS` |
| `KLM_MV_CHECK` | `TOTK_MV_CHECK` |
| `KLM_NET_LOG` | `SPLAT3_NET_LOG` |
| `KLM_NICKNAME` | `SPLAT3_NICKNAME` |
| `KLM_PAD_LAYOUT` | `TOTK_PAD_LAYOUT` |
| `KLM_PASS_CRUMBS` | `TOTK_PASS_CRUMBS` |
| `KLM_PEER_HOLD` | `SPLAT3_PEER_HOLD` |
| `KLM_PEER_SECONDS` | `SPLAT3_PEER_SECONDS` |
| `KLM_PIPELINE_CACHE` | `TOTK_PIPELINE_CACHE` |
| `KLM_PIPE_SKIP` | `TOTK_PIPE_SKIP` |
| `KLM_PIPE_WAIT` | `TOTK_PIPE_WAIT` |
| `KLM_PLAYER_NAME` | `TOTK_PLAYER_NAME` |
| `KLM_PROFILE` | `TOTK_PROFILE` |
| `KLM_PROFILE_DIR` | `TOTK_PROFILE_DIR` |
| `KLM_PROFILE_FRAMES` | `TOTK_PROFILE_FRAMES` |
| `KLM_PROFILE_RENDER` | `TOTK_PROFILE_RENDER` |
| `KLM_PROMPTS` | `TOTK_PROMPTS` |
| `KLM_RDOC` | `TOTK_RDOC` |
| `KLM_RDOC_DIR` | `TOTK_RDOC_DIR` |
| `KLM_REC_END` | `TOTK_REC_END` |
| `KLM_REC_PUB` | `TOTK_REC_PUB` |
| `KLM_REPLAY_ASYNC` | `TOTK_REPLAY_ASYNC` |
| `KLM_REPLAY_STATE_CHECK` | `TOTK_REPLAY_STATE_CHECK` |
| `KLM_REPLAY_WORKERS` | `TOTK_REPLAY_WORKERS` |
| `KLM_ROMFS` | `TOTK_ROMFS` |
| `KLM_ROMFS_PROOF_DIR` | `TOTK_ROMFS_PROOF_DIR` |
| `KLM_RSTICK_DEADZONE` | `TOTK_RSTICK_DEADZONE` |
| `KLM_RUMBLE_CURVE` | `TOTK_RUMBLE_CURVE` |
| `KLM_RUMBLE_STRENGTH` | `TOTK_RUMBLE_STRENGTH` |
| `KLM_RUMBLE_TRIGGERS` | `TOTK_RUMBLE_TRIGGERS` |
| `KLM_SAVE_PROFILE` | `TOTK_SAVE_PROFILE` |
| `KLM_SHADER_CACHE` | `TOTK_SHADER_CACHE` |
| `KLM_SHADOW` | `TOTK_SHADOW` |
| `KLM_SHADOW_DEFER` | `TOTK_SHADOW_DEFER` |
| `KLM_SHARED_FONTS` | `TOTK_SHARED_FONTS` |
| `KLM_SHDBG` | `TOTK_SHDBG` |
| `KLM_SHDBG_AT` | `TOTK_SHDBG_AT` |
| `KLM_SHDBG_FRAMES` | `TOTK_SHDBG_FRAMES` |
| `KLM_SHDBG_RECT` | `TOTK_SHDBG_RECT` |
| `KLM_SHDBG_STEP` | `TOTK_SHDBG_STEP` |
| `KLM_SHOTS` | `TOTK_SHOTS` |
| `KLM_SHOTS_DIR` | `TOTK_SHOTS_DIR` |
| `KLM_SPIN` | `TOTK_SPIN` |
| `KLM_SPIN_HANDOFF_US` | `TOTK_SPIN_HANDOFF_US` |
| `KLM_START` | `SPLAT3_START` |
| `KLM_START_SCENE` | `TOTK_START_SCENE` |
| `KLM_STICK_DEADZONE` | `TOTK_STICK_DEADZONE` |
| `KLM_SYNC_AHEAD` | `TOTK_SYNC_AHEAD` |
| `KLM_TEST_PAD` | `TOTK_TEST_PAD` |
| `KLM_TRACE` | `TOTK_TRACE` |
| `KLM_TXD_TRANSPOSE` | `TOTK_TXD_TRANSPOSE` |
| `KLM_UBO_SNAPSHOT` | `TOTK_UBO_SNAPSHOT` |
| `KLM_UB_OVERLAY` | `TOTK_UB_OVERLAY` |
| `KLM_VK_NO_FLOAT64` | `TOTK_VK_NO_FLOAT64` |
| `KLM_VK_RECORDER` | `TOTK_VK_RECORDER` |
| `KLM_VK_SSBO_ALIGN` | `TOTK_VK_SSBO_ALIGN` |
| `KLM_VK_SUBGROUP_PIN` | `TOTK_VK_SUBGROUP_PIN` |
| `KLM_VK_VALIDATION` | `TOTK_VK_VALIDATION` |
| `KLM_VRAM_LIMIT_MIB` | `TOTK_VRAM_LIMIT_MIB` |
| `KLM_WINDOW_TITLE` | `SPLAT3_WINDOW_TITLE` |
| `KLM_XLATE` | `TOTK_XLATE` |
| `KLM_ZERO_LOCALS` | `TOTK_ZERO_LOCALS` |
| `KLM_ZSTD_NATIVE` | `TOTK_ZSTD_NATIVE` |
