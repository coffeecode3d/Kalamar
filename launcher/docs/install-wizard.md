# The install wizard

How the launcher builds the game from the player's own files.

People install only the launcher. When no built game is found, the wizard takes the whole window (Welcome → Your
files → What will be installed → Install → Done); later it is on the Installation page ("Reinstall / update the game
build", "Uninstall the game build") and in Settings > Launcher. It is written in Rust
(`src-tauri/src/installer/`) and installs nothing on the system: every tool lives in the install
folder (`<install>\tools`), downloaded from its official source and checked against a pinned SHA-256, or shipped with
the launcher (`installer/portable.rs`, `installer/prebuilt.rs`):

| step | what runs (no console window, each command logged to `<install>\logs\<name>.log`) | skipped when |
|---|---|---|
| checks | x64 (not ARM), x86-64-v3 CPU, the three files (container table, `cnmt.xml` title/version, key *names*), free space, RAM, Vulkan dll | never |
| prereqs | downloads llvm-mingw 20261020 (Clang/LLD 23.1.4), Ninja 1.13.2, SDL 3.4.20 and the MSYS2 packages of Opus, libvpx, Vulkan, VMA and zstd (unpacked, MSYS2 is not installed) into `<install>\tools`, **beside the next steps** until the compile needs them (no winget, no administrator, no PATH change) | toolchain marker of the same pins; or an install whose objects MSYS2 compiled, while MSYS2 is complete |
| sources | extracts the bundled runtime source into `<install>\src` (keeps an already built hactool and the save keys; unchanged files keep their dates) | same engine snapshot |
| python | python.org's embeddable Python 3.14.9 in `<install>\tools\python` (without its `._pth`: it then behaves like an installed Python), with the wheels numpy 2.5.4, lz4 4.4.5, cryptography 50.0.2, cffi 2.1.1 and pycparser 3.11 unpacked into its `Lib\site-packages` | imports work (an older install's `work\venv` is kept) |
| exefs | `s3_extract.py` on the update, then **SHA-256 of `main` against `recomp/targets.py`** (wrong update: stops here, before any long step, and deletes the extraction) | `exefs\main` present (hash still checked) |
| hactool | copies the hactool shipped with the launcher (built by the engine's `tools/build_hactool.py`, with `range.patch`) to `tools\bin` with its license texts | the shipped exe in place, or RomFS merged |
| romfs | `s3_merge.py --in-place`: hactool reads the two Program NCAs inside the NSP files (no 12 GB copy); runs **beside the lift and the compile** (the lift reads only the ExeFS; the compile compiles every object beside it and waits for it only before the RomFS index and the link); progress = bytes of its temporary folder | `romfs.bin` + `index.json` |
| lift | `recomp/gen.py --target splatoon3-v1130 --jobs N` into the existing `lift\` (N: every logical CPU within 80 % of the available memory at 0.75 GB per worker; the generator's own default assumes 1.5 GB) (it rewrites only the chunks whose text changes; progress from `emitted n/m chunks`); the game of this install may run meanwhile (below normal priority then) | complete and made by the same generator (digest of `recomp/` + `tools/check_content.py`), or the game built from this source |
| compile | waits for the toolchain; while the RomFS merge still runs, and for an update (a game executable in place), `--compile-only` first (every object; a placeholder RomFS index during the merge), then waits for the merge and for the game of this install to be closed; `tools/build_game.py … --jobs N --toolchain tools\llvm --deps tools\deps --ninja tools\bin\ninja.exe` (Ninja: only what is out of date; progress from `[n/N]`); below normal priority when the jobs outnumber the cores or the game is open; a link run stopped by a game started during it waits for that game and runs again; keeps `lift\` and `build\obj\` unless asked | game built from the same inputs (digest of every source file that can reach the executable, the target, the RomFS index, the toolchain) |
| xlate | copies the translator shipped with the launcher, built from the very sources of the bundled runtime (`kalamar-tools.json` names their digest; a test checks it); only without one, a local build (Git, .NET 10 SDK, Visual Studio C++ tools, when present) | translator built from this source |
| keys | `s3save.py keys` | file present, same `s3save.py` |
| layout | `userdata\logs`, `userdata\profile`; points the launcher at `<install>\work\splatoon3-11.3.0` (after the self-test, for an install made for the previous game version) | never |
| selftest | `splat3.exe --selftest`, `--probe-profile` (exit 0, no `FAILED`/`FATAL`) | never |
| cleanup | removes whatever intermediate is left | never |

- **Measured** on one machine (8 cores / 16 threads, 32 GB, install and dumps on a SATA SSD, about 40 MB/s download;
  `examples/wizard_run.rs`): a first install takes about 12.5 minutes. Compile 532 s with 16 jobs, lift 202 s, the
  RomFS merge 156 s beside it, the tools' download and unpacking 28 s beside the first steps. 232 MB downloaded;
  12.94 GB in the folder at the end, which is also the most it holds; nothing written outside the folder.
- **State and resume.** `<install>\kalamar-install.json` records each step (status, time, engine snapshot) and the
  paths of the player's files (paths only). Every step first tests its outputs, so the same run resumes after a cancel,
  a failure, a closed launcher or a reboot (`config.json` `install.pending` makes the next start offer "Resume"). A
  prerequisite installer that asks for a restart stops the run with outcome `reboot`. Closing the window during an
  installation asks first (stay / minimize and keep installing / stop and close); a launcher that is killed anyway leaves
  the state of the last finished step, and the next start names the step it stopped at, removes the merge tool's
  temporary folder, keeps the previous executable a stopped compile had already replaced (`build-previous.new\`
  becomes `build-previous\`) and puts back one an older launcher had moved aside.
- **Cancel** kills the current command's whole process tree (a kill-on-close job object: Python, Ninja, every clang,
  git, dotnet), and the steps running beside it (their own job objects; a download stops at its next chunk). Closing the
  launcher has the same effect. A cut download resumes where it stopped.
- **Progress.** Measured where the tools say it (compile, lift, merge bytes, downloaded bytes), else an indeterminate bar;
  a step running beside the current one has its own line. The ETA comes from the reference durations of the measured
  first install (`progress::reference_secs_portable`; `progress::reference_secs` for an install that uses MSYS2), scaled by how fast this
  PC went through the steps already done, the steps that run side by side counted once (`progress::lanes_eta`).
- **Failures** show a sentence, the last lines of that step's log, advice, "Copy details" and "Open the logs folder";
  "Try again" resumes at that step.
- **The player's files** are only read (by `s3_extract.py`, `s3_merge.py`, hactool; hactool reads the two Program NCAs
  in place inside the NSP files). `s3_merge.py` keeps, for about two minutes, a key file derived from prod.keys in
  `work\splatoon3-11.3.0\update-merge-*` (with the NCA copies too when hactool cannot read in place); the wizard
  deletes that folder whenever the step ends, canceled or failed.
  Key values are never kept, logged or shown: the check reads prod.keys once (values included, in memory, then
  dropped) and only looks at the key names.
- **Downloads**: about 232 MB on a first install, listed with their sources and sizes on the plan screen: python.org,
  pypi.org, the GitHub releases of llvm-mingw, Ninja and SDL, repo.msys2.org; https only, each file checked against the
  SHA-256 pinned in `installer/portable.rs` (a cut download resumes). hactool and the shader translator come with the
  launcher (`resources/kalamar-tools.*`, `examples/bundle_tools.rs`). Nothing of the game is downloaded.
- **Nothing on the system.** No winget, no administrator prompt, no PATH or registry change: uninstalling removes the
  tools with the rest. An install whose objects were compiled with MSYS2 (a pre-release install) keeps it while it
  is complete, so an update does not recompile everything for a new compiler; otherwise the portable tools are used.
- **Disk**: checked before every step for that step and the ones after it (`installer/space.rs`, measured sizes), about
  15 GB free at the start of a first install; about 13 GB used at the end (measured: RomFS image 10.06 GB, game 0.38 GB, Python 0.15 GB, ExeFS, source, translator, plus the generated C 1.5 GB and the objects 0.4 GB kept for the next update; an advanced option deletes those two, and the next update then rebuilds everything).
- **Updates** redo only what their inputs changed, decided from contents, never from dates or snapshot names
  (`installer/incremental.rs`). Before it starts, the plan screen says what will be redone and how long it takes, from
  the build's own Ninja records (which sources and headers each object read, how long it took) against the files of
  the bundled source that differ from the installed one: "only the engine changes: N objects, about 1 min", "the code
  generator changes: between X and Y", or "everything is recompiled because ...". The Play page shows "Game update
  available" with that sentence and a button that opens the wizard on that install's plan
  (`installer_update_check`). `cargo run --release --example wizard_run -- <install> <bundle dir> [--plan-only |
  --check]` runs the same pipeline without the window (developer tool, for measuring); `--probe <data dir>` also plays
  the page's part (the Locker's catalog built the moment the merged data is complete, settings and a profile written
  during the compile and checked at the end), `--hold <s> --hold-at start|compile` holds the game of the install open
  (`splat3.exe --selftest` started suspended) to see the link wait for it.
- **Uninstall** removes only what the wizard made inside the folder (`src`, `tools`, `work\venv`, `work\xlate`, the game's
  `exefs`, `romfs`, `build`, the logs and the state file); never a system tool, never the player's files; the game's
  `userdata` (saves) and settings file stay unless the separate box is ticked. It is offered only on a folder that has
  the wizard's state file.

**Priority and jobs.** Build commands run at normal priority; parallel compiles default to every logical CPU, at most one
per 1.25 GB of memory beyond 4 GB (adjustable in the wizard's advanced options); when they outnumber the physical cores the
compile runs below normal priority, so the PC stays usable. An advanced, off-by-default "low priority" option runs the whole tree below
normal priority (creation flag + job limit, inherited by Ninja and every clang). The only wait is for the game of THIS
install, and only before the link (its executable is replaced there); other running games do not stop the build.

### While it compiles (`src/install/WaitTasks.tsx`, `wait.ts`)

The install screen's empty half holds what can be set up during the wait, checked from the launcher's own files as it
gets done; the rail shows the installation from every other page (`InstallChip`: percentage ring, step, time left; one
click back), and the end is said where the player is.

| card | where it is kept | when |
|---|---|---|
| name and character (Locker) | `<data>\profile.json`; applied at launch (`profile::prepare_launch` → `<root>\userdata\launcher-save-profile.json`) | the name at once; the looks once the merged game data is complete (`Snapshot::game_data`, set when the RomFS step is recorded done, its outputs checked): about 3 min into a first install, about 7 min before its end; before that the card says "in about N min" (`game_data_eta`) |
| mouse (sensitivity, cm/360) and keys, display | the game's `<root>\build\kalamar.ini` | at once: during an installation the Settings page edits the file of the folder it builds (`AppState::settings_root`; that folder becomes the configured root at the layout step) |
| language | `<data>\config.json` | at once |
| community server, login | `<data>\online\` | at once (optional card) |
| the native mode in three lines | (read) | at once |

**No concurrent write.** No step writes `kalamar.ini`: `build_game.py` writes its outputs beside it (objects, `build.ninja`,
`modules.c`, the RomFS index only when it changes, the DLLs after the link), the self-check only reads it (`cfg: read …: N
keys` in its log), the game itself writes back only the keys changed in its own menu. The launcher writes it atomically
(read, change, temporary file, rename). `profile.json` and `config.json` are the launcher's only. The Locker reads the
install's RomFS only after its merge step ended and was checked, with the shared read access of `std::fs` (the merge never
rewrites a complete image: the step is then skipped). A reinstall into another folder takes the old settings file along
when the new one has none; a pre-release install's launcher data imported at the layout step keeps the language and interface
choices made during the installation.

**The end.** The `success` sound (the `error` one for a failure); when the window is not in front, its taskbar button
flashes and a Windows notification says it (`notify_attention`; `Shell_NotifyIconW` from a notification-area icon that
goes once the window is used again: no plugin, nothing registered; Settings › Display › Notifications turns it off,
`ui.notify`). The "Ready" screen offers Play and lists what is set and what is left, each with its button.

**Updates said truthfully, run when it suits.** The Play page's notice gives the update's class (short, long, full
recompilation, with the reason in one sentence: `upd.why.*`, e.g. "a file shared by all of the game's code changed:
everything is recompiled"). Over two minutes (`LONG_UPDATE_SECS`) it asks: now, when the launcher closes, or later.
During an update the installed game stays playable from the Play page (launches wait only while the new executable or
translator is put in place: `Snapshot::replacing`, `AppError::GameUpdating`); the objects compile beside the game, opened
before or during the compile, and the link waits for it to be closed. The link alone would not stop for it: measured, lld
renames the new executable over one a process holds (that process keeps its image), and the DLLs copied after it are
the ones a running game holds; hence the separate link run, after the wait (a few seconds more per update). A game
started from outside the launcher during that run makes it stop: the step waits for it and runs it again (at most three
times). "When the launcher closes": closing the window starts the update, the window goes, an icon
stays in the notification area with the progress in its tooltip (its menu: Open, Quit — Quit asks first), a notification
says when it ends; the launcher then quits after 15 s unless the window was brought back; a failure keeps the icon. The
community page says a running update plainly, and a match refused because this game is older than the room needs points to
the update.

**Installs made by pre-release tools** (a folder that holds its own copy of the runtime source next to
`work\splatoon3-11.3.0`) are found at start (their default folder, the configured root, the folders the launcher runs
from) and offered as "Update the installed game" (or "Play the installed version"). The update needs no dumps or keys:
the extracted ExeFS (its `main` hash is checked again) and the merged RomFS are reused, as are the tools, the Python
environment and hactool. The runtime source is replaced by the bundled one; the generated C is reused when the code
generator's sources (`recomp/`) did not change, else regenerated from the existing ExeFS; the compile is incremental
when the install's `build\obj` is still there (kept afterwards), else full; the shader translator is kept when its inputs
(`tools/build_xlate.py`, `tools/xlate/`, `tools/vendor/ryujinx/`) are unchanged. Before a compile the current
`splat3.exe` gets a second name in `build-previous.new\` (a hard link, with copies of the DLLs: nothing is moved, the
game stays playable); the link writes the new executable under a temporary name and renames it into place, so that
name keeps the old game, which becomes `build-previous\` once the link replaced it (nothing is kept when Ninja had
nothing to relink). If the update then fails it is put back; after success it stays as "Revert to the previous
version" (Installation page; again to go forward; the next update then only relinks).
The translator is kept the same way (`work\xlate\previous`). User data (`userdata\`: saves, profile, shader and pipeline
caches) and the settings file are never touched; the old launcher is left in place, and its data (`<install>\launcher-data`) is copied once into Kalamar's data folder when Kalamar has no profile yet.

## One game version, and the update to the next

The launcher builds ONE version of the game: 11.3.0. Everything that names it reads `src-tauri/src/installer/gamever.rs`
(`CURRENT`: the display version, the game folder `splatoon3-11.3.0`, the engine target `splatoon3-v1130`, the update's
content-meta version and Program NCA name, the merged RomFS's size; the engine's `recomp/targets.py` holds the
matching `main` hash the extraction step checks). The next game update is a change to that file, the version text of
the four interface languages (a test fails on any `11.x.y` there that is neither `CURRENT` nor `PREVIOUS`), the
engine's own target, and a re-bundle. `PREVIOUS` (11.2.0) exists only to recognize what a player still has; nothing
builds it.

**The update file.** The wizard asks for the 11.3.0 update. Any other file is named for what it is, before anything
is installed (`installer/nsp.rs`): the 11.2.0 update (the version Kalamar used before) gets "This is the 11.2.0 update,
the version Kalamar used before. Kalamar now needs the 11.3.0 update."; another version of the update gets its revision
and the version needed; a file without a content-meta is accepted, and its `main` is checked against the engine's hash
before any long step (`e.version_mismatch`).

**An install made for 11.2.0** (the game in `<install>\work\splatoon3-11.2.0`) is not rebuilt in place: the
game of another version needs another ExeFS, RomFS, generated C and executable.

- *Recognized.* `find_existing` and `update_check` see the old folder and no 11.3.0 game. The Play page shows "Game
  update needed: Splatoon 3 11.3.0" (it is the "game update" notice of the update check, with `game_from`; no new
  concept) with what the update asks, keeps and deletes, and the wizard's welcome screen has a card for it.
- *Offline play stays possible.* The launcher goes on pointing at the old game until the update is done, and it still
  starts for solo play (nothing about starting a game depends on its version). Online play does not: the check that
  blocks a game too old for the automatic match (`proto/freshness.rs`, level `too_old`) also fires for a game folder of
  another version (`GameCheck.old_version`), so the community pages refuse "ready", "start" and "accept" with the
  update's words, the runner refuses a match (`GameHost::game_outdated`), and `PUT /v1/me/game` is not sent: the
  server never learns of a build whose label would say 11.3.0 for an 11.2.0 game.
- *What is asked.* Only the 11.3.0 update file is new: the base game and the keys file are the paths remembered in
  the launcher's configuration (`install.base_nsp`, `install.prod_keys`; checked again, asked again only when the file is
  gone or fails its check), and the remembered update path, which is the old file, is cleared.
- *What is built.* `<install>\work\splatoon3-11.3.0` beside the old folder (about 13 GB more, checked before every step), with the
  tools, Python and shader translator the install already has (the translator and hactool do not depend on the game
  version; the state file's `target` field says which game version its steps were done for: a state of another one
  forgets the exefs, romfs, lift, compile and keys steps and their input digests, and keeps the tools). The save keys
  (`s3save.py keys`) are made again from the 11.3.0 `main` and RomFS, as the install log says; generated C and objects
  of the old game are not reused (a different `main`).
- *What is kept.* At the layout step, everything under the old game's `userdata*` folders (saves, profile, logs,
  the second player's `userdata-p2`) is copied into the new game folder, never overwriting a file already there and
  never following a link (a junction or symbolic link inside is left alone and counted in the log). The shader and
  pipeline caches (`gamever::DERIVED_CACHES`) are not carried: they come from the code the game runs, so the new game
  builds its own as it is played (the first sessions translate shaders again). The
  settings file (`build\kalamar.ini`) comes along when the launcher's root switches (`on_root`), as for any new folder,
  and the Locker's profile is the launcher's own (`<data>\profile.json`). The save is not rewritten: the game version
  word in its header is only shown by `s3save.py`, never read or written by anything else.
- *When the old folder goes.* In the cleanup step, which runs only after the self-check and the profile probe of the
  new game passed: the launcher is pointed at the new folder, then `gamever::remove_previous_game` deletes the old
  one (about 14 GB). It refuses unless the folder is a real folder inside the install folder; a link or junction found
  inside is removed itself and never followed; a file in use leaves the rest in place with a warning ("The old game
  folder could not be deleted completely...") and the next update tries again. A failed or cancelled install deletes
  nothing and leaves the launcher on the old game; "Resume" continues the update. "Uninstall" on an install not yet
  updated removes what the wizard made in the old game folder too, and keeps its saves and settings like the current
  one's.

## Bundled source and tools

Two archives are committed in `src-tauri/resources/` and embedded in the executable, so that the portable release is
`Kalamar.exe` alone:

- `runtime-src.tar.gz` + `runtime-src.json`: the engine sources the wizard compiles (`git archive` of `engine/`; the manifest
  names the snapshot by the git tree id of `engine/`, which is the same in every repository that holds these exact
  sources) plus the helper scripts of `kit-tools/` (`s3_extract.py`, `s3_merge.py`,
  `profile/s3save.py`). `scripts/bundle-runtime.ps1` (`npm run bundle:runtime`, also run before `npm run tauri build`)
  remakes it only when that tree id or a helper script differs from what the manifest records, through `examples/bundle_runtime.rs`, which
  refuses to write it if any file fails the "nothing derived from the game" scan (`installer::bundle::scan_entry`: the
  engine's own `check_content.py` rules plus ExeFS/RomFS/lift/save/cache names and folders, unreviewed images, key
  lines, the generator's marker, files over 1 MiB). After a change under `engine/`, run it and commit the result.
- `kalamar-tools.tar.gz` + `kalamar-tools.json`: hactool and the shader translator, prebuilt from the sources and
  patches of `engine/tools/` with their license texts (`examples/bundle_tools.rs`), so that players need no .NET SDK
  or C++ build tools. The manifest names the digest of the sources they were built from; a test checks it against the
  bundled engine sources.

`cargo test` scans the shipped archive again
(`shipped_bundle_is_clean`, which also scans the bytes compiled into the executable and fails if they differ from
`resources\`). `installer::bundle::resolve`
picks the source folder: the first valid `resources` folder (the Tauri resource folder of the NSIS installer, then
`resources\` beside the executable; development builds also the source tree's), valid meaning the archive's SHA-256
equals the manifest's; otherwise the embedded copy is written to `<data>\runtime-src\<SHA-256 of the manifest>\`
(`KALAMAR_DATA` respected), written aside and renamed into place, read back and checked, and reused only while
its archive still matches (a damaged copy is replaced). If even that fails, the wizard says "Launcher files incomplete:
download Kalamar again" at once (`WizardInfo.bundle_error`, `AppError::RuntimeMissing`). `KALAMAR_TRACE` logs
`runtime:resources` or `runtime:embedded` with the folder used. `cargo run --example wizard_probe -- <dir> <base> <update> <keys>` prints what the wizard
detects (codes only).

## Data of pre-release launchers

The identifier is `dev.kalamar.launcher`; pre-release launchers used `dev.splat3recomp.launcher`. At the first start,
before anything reads it, `datadir.rs` renames that data folder to the new name (one rename on the same volume,
journaled: a launcher stopped half-way finishes it at its next start), rewrites the paths into it that the launcher's
JSON files hold, and logs each decision in `data-folder.log`. A game installation found inside it stays where it is
(its build records hold absolute paths); both folders present: the new one is used, nothing merged; an older launcher
open, an update on trial or a file in use: the old folder serves this session and the move waits for the next start
(the table in `datadir.rs`). Older-format files are upgraded in place after a backup (`backup-pre-upgrade\`).
`migrate.rs` also copies (never moves) another data folder (a portable one) into the current one on demand from
Settings > Launcher, keeping each server's session byte for byte and unread; rejected files go to
`migration-rejected\`. Remembered UI choices are in `config.json` (`ui`). The game's settings file is `kalamar.ini`, else `totk.ini`
(`install::ini_path`).
