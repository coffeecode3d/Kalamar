# Kalamar engine: instructions for coding assistants

The rules of the whole repository (game content, network, languages, licenses, compatibility, files not to change
without an issue, AI-assisted contributions) and the checks CI runs are in the top-level [AGENTS.md](../AGENTS.md):
read it first. This page adds what is particular to the engine. Design: [docs/architecture.md](docs/architecture.md).
Toolchain, builds and checks: [docs/building.md](docs/building.md). What a pull request carries (checks by changed
area, proof, performance runs): [CONTRIBUTING.md](CONTRIBUTING.md).

## What the engine is

A fork of totk-pc-recomp (MIT, [NOTICE](NOTICE)) that targets *Splatoon 3* 11.3.0: target `splatoon3-v1130`,
executable `splat3.exe` (the 11.2.0 target `splatoon3-v1120` still builds). The upstream's *Tears of the Kingdom* targets (`v121`, `v143`, executable `totk.exe`) are
still in the code, and many pages and tools were written for them: [docs/agents/](docs/agents/) and
[docs/pr-review-kit/](docs/pr-review-kit/) come from the upstream and say at their top what was checked on this target.
Variables are documented as `KLM_*`; the code still names them `TOTK_*` (or `SPLAT3_*`), which the runtime reads after
the `KLM_` name ([docs/settings.md](docs/settings.md), "Legacy names"). The settings file is `kalamar.ini`; `totk.ini`
is read when there is no `kalamar.ini`.

## Game content boundary

- Everything extracted or generated stays out of the repository: under the ignored `build/` and `generated/`, or a
  work folder outside the checkout. Every file the code generator emits carries `GENERATED_MARKER` of
  `tools/check_content.py`, which refuses it.
- Pass the keys file's path to the tools; they read it themselves. Key values stay out of your output, logs, commands
  and files.
- `config.local.toml` (ignored; copy `config.example.toml`) holds the paths of the upstream's TotK pipeline. The
  Splatoon 3 target is built with explicit paths: [docs/building.md](docs/building.md), "Building Splatoon 3".
- Before a commit, from `engine/`: `python tools/check_content.py`, as CI runs it, plus the top-level
  `python scripts/check-content.py`. `--staged` run from `engine/` gets paths relative to the repository's root and
  skips the file checks: use the full check.

## Reusing code

- `refs/` holds read-only checkouts of the projects this engine builds on, at pinned commits (`sh tools/fetch_refs.sh`
  recreates it; ignored).
- Copy only MIT, BSD, ISC, zlib or Apache-2.0 code, keep its header, and add a row with the source commit to
  [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
- yuzu, suyu, OpenSmash-Ultimate and other GPL code: read for behavior, write our own. Unicorn (GPL-2.0) is used only as
  a separate test tool, never linked into the game.
- `tools/vendor/` stays byte-identical to its upstream; Windows changes go in `tools/extract.py`.

## Code

- Build C with MSYS2 UCRT64 Clang (`C:\msys64\ucrt64\bin`). Generated C uses GNU extensions, so `cl.exe` is out. Call
  Ninja from that directory by full path or put it first on `PATH`.
- Two Python environments: `.venv` (3.12, `tools/requirements.txt`) for the lifter and the build, `.venv-py314` for the
  extraction and shader scripts, which need `compression.zstd`.
- Generated C is never edited by hand: change the generator (`recomp/`). `recomp/layout.py` alone decides module bases.
- Every change to the lifter or to `runtime/cpu.h`, `simd.h`, `simd_fp.h` passes the differential tests against Unicorn
  ([docs/building.md](docs/building.md), "Checks"); the ones without game files are also in CI. `tests/diff/lift.py`,
  `func.py` and `tests/jumptables_check.py` read extracted modules: they default to the upstream's TotK paths, so give
  them `--exefs` and `--modules main,sdk` for Splatoon 3 (no `subsdk`). A change to those headers also recompiles every
  player's generated code: the top-level [AGENTS.md](../AGENTS.md), rule 8.
- Code that knows TotK's addresses or data runs for TotK only (`rt_target`, `runtime/host.h`); Splatoon 3's patch
  tables hold spin waits and, for 11.3.0 only, the `mouse_native` and `input_first` sets
  (`recomp/patches-splatoon3-v1130.txt`, `-v1120.txt`); a new game version is ported with `tools/port_offsets.py`.
- Graphics bugs: [docs/maxwell.md](docs/maxwell.md) lists the Maxwell behavior the Vulkan path must match.
- Comments say what the code does and why, in words that stand on their own. Cite this project's code by file and
  function (a line number may follow as a hint) and other projects' code by project, file and function: `refs/` moves,
  so its line numbers rot. Commit hashes belong to the attribution of copied code.

## Running and debugging

- Nothing of the runtime builds or runs without your own game files. Never start the game or the launcher unless your
  user asked; stop only a process you started, by its PID.
- Running the game unattended, screenshots, frame dumps, timings: [docs/agents/running.md](docs/agents/running.md).
- A bug, glitch or crash: search [docs/agents/symptoms.md](docs/agents/symptoms.md) by the player's words first, then
  follow [docs/agents/debugging.md](docs/agents/debugging.md). Read the code before running the game.

## Worktrees

A build needs the checkout's ignored data (`build/`, `generated/`, `refs/`, `toolchain/`, the two `.venv*`). If you
link those folders into a git worktree as directory junctions, delete the junctions before you remove the worktree:
`git worktree remove`, with or without `--force`, follows them and deletes the main checkout's data (seen with Git for
Windows 2.52).

## Pull requests

Target `main`. The checks by changed area, the proof and the performance runs a pull request carries are in
[CONTRIBUTING.md](CONTRIBUTING.md), "Pull requests"; how maintainers review it:
[docs/pr-review-kit/](docs/pr-review-kit/).
Code, comments, docs and commit messages are in English; commit messages are plain sentences without type prefixes.
