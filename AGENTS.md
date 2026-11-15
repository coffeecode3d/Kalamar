# Instructions for coding assistants

For AI coding assistants (Claude Code reads this file through [CLAUDE.md](CLAUDE.md); Codex, Cursor, Copilot and
others read it directly) and for the people who drive them: what the code does not tell you. The full rules are in
[CONTRIBUTING.md](CONTRIBUTING.md), which wins where the two differ. In `engine/`, also read
[engine/AGENTS.md](engine/AGENTS.md).

## Map

Kalamar builds a native Windows version of *Splatoon 3* 11.3.0 from the player's own dump, on the player's PC.

| Path | What |
|---|---|
| `launcher/src/` | the launcher's interface (React + TypeScript); its text in `locales/<en,fr,es,ja>/`; made-up preview data in `dev/mock/` |
| `launcher/src-tauri/src/` | its back end (Rust, Tauri 2): install wizard (`installer/`), launching, settings, community client (`proto/`), updates |
| `launcher/src-tauri/resources/` | what the launcher embeds: the engine's source and two prebuilt tools, as generated archives |
| `engine/recomp/` | the static recompiler (Python): the game's ARM64 code to C |
| `engine/runtime/` | the native runtime (C, some C++): system services, Vulkan renderer, input, audio, LAN play, in-game menu |
| `engine/tools/`, `engine/tests/` | build, extraction and analysis tools; tests |
| `docs/`, `scripts/` | guides (index: [docs/README.md](docs/README.md)); the repository checks CI runs |

The protocol specification, the reference server and the relay live in the server repository, `kalamar-server`.
How the pieces talk: [docs/architecture.md](docs/architecture.md). The launcher in depth:
[launcher/README.md](launcher/README.md). Reporting a vulnerability: [SECURITY.md](SECURITY.md).

## Checks before a pull request

CI (`.github/workflows/`) runs these; run the ones that match your change:

```powershell
# from the repository root
python scripts/check-english.py
python scripts/check-content.py
python scripts/check-links.py --skip "engine/tools/vendor/**" --skip "engine/runtime/third_party/**"
python scripts/third-party-inventory.py --check      # needs cargo; without --check it regenerates the inventory
# from launcher/ (Node.js 24; Rust 1.97.1, MSVC toolchain, with clippy)
npm ci
npx tsc --noEmit
npm run build                # the Rust side embeds dist/: build it before cargo
npm run test:ui
cd src-tauri; cargo test --locked; cargo clippy --locked --all-targets -- -D warnings
# from engine/ (Python 3.12)
python tools/check_content.py
python tests/settings_doc_check.py
python tests/env_names_check.py
```

`.github/workflows/engine.yml` also runs the engine's packaging tests, and, with `capstone`, `unicorn`, `lz4` and
`numpy` at the versions of `engine/tools/requirements.txt` and MSYS2 UCRT64 Clang on `PATH`, its tool tests and the CPU
differential tests against Unicorn (`tests/diff/simd.py` takes minutes).

**Needs the game, so CI never runs it:** building the game ([engine/docs/building.md](engine/docs/building.md)); any
mode of the built `splat3.exe`, even `--selftest` (nothing of the runtime compiles without the game's module layout);
`engine/tests/diff/lift.py`, `func.py` and `tests/jumptables_check.py`; the install wizard end to end;
`cargo test --test game_data -- --ignored`; real matches. Say in the pull request which of these you ran, on what
hardware, and which you could not.

## Rules that are not negotiable

1. **Nothing derived from the game or a console, ever:** no game files or parts of them, extracted or decrypted data,
   generated C, builds, shaders or caches, saves, screenshots or recordings of the game, sounds, fonts, tables copied
   out of it (names of items, stages, weapons, messages, symbols), keys, certificates, firmware, or addresses of the
   publisher's servers; not in code, comments, tests, issues or pull requests. Why: the project exists only because it
   ships nothing of the game. Facts needed to run the player's own copy (function addresses, offsets, build hashes)
   are allowed: [docs/LEGAL-NOTES.md](docs/LEGAL-NOTES.md), section 1. `scripts/check-content.py` and
   `engine/tools/check_content.py` catch file types, names, key lines and the generator's marker, not everything.
2. **The game contacts no one by itself.** With LAN play off (no `KLM_LAN`) it opens no socket and loads no network
   library; with it on, it reaches only the local subnet or the relay the launcher gave it, by address. It never
   resolves a host name and never contacts the publisher. The launcher has no telemetry, crash reporting or remote
   configuration; every access it makes is listed in [launcher/docs/network-accesses.md](launcher/docs/network-accesses.md); TLS certificate checks
   are never turned off, and plain HTTP is for loopback only. Why: offline must mean offline, and no server or match
   may point a game anywhere else. A new network access of any kind needs an issue first, and updates that list and
   [docs/servers.md](docs/servers.md).
3. **A community server sends data, never code:** plain text and typed data, no HTML, markdown, script or image, and
   links that open only on a click (the server repository's `docs/protocol/extending.md`, section 5). Everything that
   comes from a server, a relay, a match code or another player is untrusted: bound it, validate it, clean it before
   showing it. Why: anyone can run a server. Keep what
   [launcher/docs/security-model.md](launcher/docs/security-model.md) promises true.
4. **English only** in code, comments, docs, commit messages and developer logs (American spelling), outside the
   translations listed in `scripts/english-only.json`. **Every interface string exists in `en`, `fr`, `es` and `ja`**
   (`launcher/src/locales/`: `npx tsc --noEmit` refuses a missing key); engine menu text goes into every language
   `engine/runtime/ui_text.c` keeps in full (its menu probe fails otherwise). Why: one working language for
   contributors, and the player's own language on screen.
5. **Compatibility.** A protocol revision only adds (optional members, open enums, nothing removed) and is written in
   the server repository's `docs/protocol/` (`api.md`, `openapi.yaml`, the changes table of `overview.md`) with a
   conformance scenario. Older names keep working: the engine reads `KLM_*` and still the older `TOTK_*`/`SPLAT3_*`
   names (`engine/tests/env_names_check.py`), `kalamar.ini` and still `totk.ini`; the launcher still reads its
   `SPLAT3_*` variables and also sets the older engine names. Why: installed games, settings files, scripts and other
   launchers depend on them.
6. **No code from projects with an incompatible license.** Copy only MIT, BSD, ISC, zlib or Apache-2.0 code, keep its
   header, and add a row to the part's `THIRD_PARTY_NOTICES.md`. GPL and other copyleft projects (several console
   emulators among them) may be read for behavior, never copied or reproduced from memory. Why: Kalamar is MIT and must
   stay redistributable.
7. **Never run the launcher on real data.** Before `npm run tauri dev` or any built launcher, set `KALAMAR_APPDATA` to
   a throwaway folder (it stands for `%USERPROFILE%\AppData`, the WebView's profile included), or at least
   `KALAMAR_DATA`. Why: otherwise it reads and writes `%APPDATA%\dev.kalamar.launcher`, finds a real installation, and
   moves the data of the earlier `dev.splat3recomp.launcher` at its first start. An assistant starts neither the
   launcher nor the game unless its user asked for it.
8. **Open an issue before changing these:**
   - `engine/runtime/cpu.h`, `simd.h`, `simd_fp.h`: every generated C file includes them, so a change recompiles all of
     the generated code on every player's PC at their next update (`launcher/src-tauri/src/installer/incremental.rs`,
     `a_header_of_the_generated_code_recompiles_it_all`); they also need the CPU differential tests.
   - `engine/recomp/` and `engine/tools/check_content.py`: the code generator's inputs (`LIFT_INPUTS`,
     `installer/bundle.rs`); a change re-runs the code generation at every player's next update. Generated C is never
     committed and never edited by hand: change the generator.
   - `engine/tools/xlate/`, `engine/tools/vendor/ryujinx/`: the shader translator; a change makes the prebuilt
     translator the launcher ships out of date and can change shader hashes.
   - Third-party code kept byte for byte: `engine/tools/vendor/`, `engine/runtime/third_party/`,
     `engine/runtime/shaders/ffx/`.
   - Generated files, never edited by hand: `launcher/src-tauri/resources/runtime-src.*` and `kalamar-tools.*`
     (`launcher/scripts/bundle-runtime.ps1` and the engine's tool builds; maintainers remake them in commits of their
     own), and `docs/third-party-dependencies.md` (`scripts/third-party-inventory.py`).
   - `launcher/src-tauri/update-keys.txt` and `.github/workflows/release.yml` (release signing); the exceptions in
     `scripts/content-rules.json` and `scripts/english-only.json` (each needs a reason a maintainer reviewed); the
     engine's interface for launchers ([engine/docs/launcher-interface.md](engine/docs/launcher-interface.md)).

## Conventions

- **Commits:** a plain English sentence in the imperative that says what the change does, without `feat:` or `fix:`
  (`Load run-time libraries from the executable's folder and System32 only`); the body says why; a commit about an
  issue ends its subject with `(#123)`. Each commit is signed off (`git commit -s`, Developer Certificate of Origin).
- **Pull requests:** one topic, as small as it can be, from and against `main`, rebased before review, the template
  filled in, CI green. Anything big (a feature, a dependency, a network access, a new setting) starts as an issue.
- **Tests and docs come with the change:** a test that fails without it (Rust unit tests, `*.test.mjs` listed in
  `test:ui`, the engine's tests); the guide, the settings references, the network list and every language updated.
- **No mass reformatting.** The launcher's Rust is not `rustfmt`-formatted: format new files only and leave the lines
  you do not otherwise change. In the engine, follow the surrounding code.
- **Screenshots** show only the launcher's interface in its browser preview on made-up data: `npm run dev:mock`, then
  `npm run shots -- --out <a folder outside the repository> --lang en,fr`. Never the game, never the real launcher's
  window (it shows pictures read from the player's game). An image enters the repository only on purpose, after review,
  pinned in `scripts/content-rules.json`.

## AI-assisted contributions

Welcome, under every rule above. The person who opens the pull request is its author and answers for every line:

- Read the whole diff and understand it. Run the checks yourself, and write in the pull request, in your own words,
  what was run (commands and results) and what was not (game files or hardware you lack). A template ticked by a tool
  is not a test report.
- An assistant never invents or extrapolates a test result, a measurement, a log line or a screenshot. A check that
  did not run is reported as not run.
- No game content "for reference": no decompiled or disassembled game code, extracted data, game screenshots or key
  values in code, comments, tests, commit messages, issues or pull requests. Tools take the path of the keys file;
  its values never appear in a prompt, a log or a file.
- No code reproduced from a copyleft project, from memory or otherwise (rule 6).
- `Co-Authored-By:` lines naming an assistant are welcome and never required. `Signed-off-by:` is the human author's
  own statement under the DCO: the human adds it.
