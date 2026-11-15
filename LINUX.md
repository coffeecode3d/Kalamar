# Kalamar on Linux

This fork ([coffeecode3d/Kalamar](https://github.com/coffeecode3d/Kalamar)) ports the launcher to Linux x86-64 and
publishes it as an AppImage, the equivalent of the Windows release's `Kalamar.exe`. It is not affiliated with the
upstream project; the engine, the wizard's steps and the game itself are the upstream work.

**Nothing of the game is in this repository or in the AppImage.** You need your own copy of Splatoon 3 and its 11.3.0
update, dumped from your own console, with your console's keys — see the upstream
[README](README.md) and [docs/getting-started.md](docs/getting-started.md). The launcher builds the game on your PC
from those files.

## Branches, and staying level with upstream

| Branch | What |
| --- | --- |
| `main` | A mirror of `CyriacFR/Kalamar`'s `main`. Nothing is committed here by hand. |
| `linux` | `main` plus the Linux port. The AppImage is built from it. |

`.github/workflows/sync-upstream.yml` fast-forwards `main` to upstream and merges it into `linux` every Monday (and on
demand from the Actions tab). When the merge conflicts, it pushes nothing and opens an issue naming the files. To do
the same by hand:

```sh
git remote add upstream https://github.com/CyriacFR/Kalamar.git   # once
git fetch upstream
git checkout main && git merge --ff-only upstream/main && git push origin main
git checkout linux && git merge main && git push origin linux
```

## The AppImage

Every push to `linux` builds `Kalamar-<version>-x86_64.AppImage` (Actions → *Linux AppImage* → Artifacts); a `v*` tag
publishes it as a GitHub release with its `.sha256`. The AppImage contains the launcher only: the game is built on
first install, in the install folder you choose, exactly as on Windows.

```sh
chmod +x Kalamar-<version>-x86_64.AppImage
./Kalamar-<version>-x86_64.AppImage
```

The launcher keeps its data in `$XDG_DATA_HOME/dev.kalamar.launcher` (`~/.local/share/dev.kalamar.launcher`), or in
`KALAMAR_DATA` when that is set. For a throwaway profile, set `KALAMAR_APPDATA` as the contributors' guide says.

## What the port changes

- **Build tools on Linux are the system's**, as the engine documents in
  [engine/docs/building.md](engine/docs/building.md#linux-in-progress): `clang`, `lld`, `llvm-ar`, `ninja`, `make`,
  `python3` (3.14+, with `venv`; the engine tools use the standard library's `compression.zstd`), `git`, the .NET 10 SDK, and the SDL3, Opus, libvpx, Zstandard and Vulkan development
  packages. The launcher installs none of them: the plan lists what is missing with the package names to install, and
  the wizard stops with that list rather than changing your system.
- **Python** is a virtual environment in the install folder (`work/venv`), from the system's `python3`, with the pinned
  packages of `tools/requirements-installer.txt` from PyPI.
- **hactool and the shader translator** are built from the engine sources on first install (`build_hactool.py`,
  `build_xlate.py`), since the prebuilt pair the Windows launcher carries is a Windows binary. This is why Git and the
  .NET 10 SDK are required. The release workflow can bundle Linux prebuilts later without changing the wizard.
- **Paths and names**: `build/splat3` and `xlate/totk_xlate` (no `.exe`), POSIX steps, data under XDG directories.
- **Base game files**: the wizard accepts an NSP or an XCI cartridge image for the base game (the secure partition's
  NCAs); the 11.3.0 update is an NSP (an 11.2.0 install is offered the 11.3.0 update by the launcher).
- **System integration**: process groups (cancelling the wizard kills the whole build tree), `/proc` scans for "is the
  game running" and "another launcher instance", `xdg-open` for folders, `nice` for low-priority builds.
- **Self-update** stays off in this build (no update repository is named): update by downloading the new AppImage.
  The fork's `linux` branch tracks upstream for you.

The Windows behaviour is unchanged: the same code is gated on `cfg(windows)` where the two platforms differ.

## Requirements (Arch, as an example)

```sh
sudo pacman -S --needed clang lld llvm ninja make python git dotnet-sdk \
    sdl3 opus libvpx zstd vulkan-headers vulkan-icd-loader pkgconf
```

On Debian/Ubuntu the equivalents are `clang lld llvm ninja-build make python3 python3-venv git dotnet-sdk-10.0
libsdl3-dev libopus-dev libvpx-dev libzstd-dev libvulkan-dev pkg-config`.

The AppImage is built on Ubuntu 24.04: it needs glibc 2.39 or newer and the system's WebKitGTK 4.1
(`webkit2gtk-4.1`, `libwebkit2gtk-4.1-0` on Debian/Ubuntu), which is not bundled because every distribution ships it.

The game needs Linux 6.7 or later (its write watch uses `userfaultfd` write protection with `PAGEMAP_SCAN`), a Vulkan
1.3 driver, and about 15 GB free during the first install.

## Building the launcher and the AppImage

```sh
cd launcher
npm ci                      # or pnpm install
npx tauri build --bundles appimage
# the file is in src-tauri/target/release/bundle/appimage/
```

To run the wizard without its window (useful on a headless machine, or to keep the full log):

```sh
cd launcher/src-tauri
cargo run --example install_headless -- --plan \
  --dir "$HOME/Kalamar" --base <base.nsp> --update <11.3.0 update.nsp> --keys <prod.keys>
# then without --plan to install, adding --jobs N or --stop-before compile if wanted
```

The keys file is only passed to the tools by path; the example never opens it.

For development:

```sh
cd launcher
npm run dev:mock            # the interface alone, on made-up data
KALAMAR_APPDATA=/tmp/kalamar-dev npx tauri dev
cd src-tauri && cargo test --lib
```

## Frame rate

Splatoon 3 ships at 30 fps in the hub and menus and 60 in matches. The engine has a `fps60` patch set for 11.3.0 that
switches the game's own 60 fps mode on for those screens as well — normal speed, verified at 60.0 fps in the hub — and
the `fps_limit` setting (`kalamar.ini`, or Esc › Display in the game) chooses it: **60** enables the set, **30** turns
it off. Above 60 the engine needs the real-time frame step, which this game has no patch for yet, so higher limits are
reduced to 60 and the log says so.

The patch comes from the public Switch patch set by theboy181 (switch-ptchtxt-mods) and was re-verified here against
11.3.0. Treat frame-rate patches as offline features: a server cannot see a client's render rate, but a modified game
is outside what community servers promise.

## Status

The back end compiles and its tests pass on x86-64 Linux; the wizard's Linux steps follow the engine's documented
Linux build. Verified in this fork:

- `cargo test --lib` on Linux: all tests pass (the Windows-path tests are gated on `cfg(windows)`).
- `tsc`, the interface build and the interface tests.
- The AppImage build (`tauri build --bundles appimage`, 119 MB), its structure, and a launch with a throwaway profile:
  the window opens and the data folders are created under `KALAMAR_APPDATA`.

Not verified here, and worth reporting:

- A full install from real game files on Linux (Arch, 16 cores, 31 GB): an XCI base and the 11.2.0 update NSP (the
  version then current; the same steps now build 11.3.0), in about 7 minutes, ending with the wizard's self-checks (`--selftest`, `--probe-profile`) passing and `build/splat3`
  linked.
- Playing the built game: started offline with `KLM_XLATE` and the profile, 1043 frames in 21.7 s (mean 48 fps on the
  first run, while shaders are translated; the frame times settle as the cache fills). No missing shared libraries.
- A base NSP is complete when it is about 4.7 GB: `merge_failed` with `NCA section out of bounds` in
  `logs/romfs-merge.log` means the base file is truncated. An XCI of the cartridge works (the secure partition); the
  update is an NSP.
- The Steam Deck (SteamOS) and distributions other than the one used for the build.
- Distributions whose `python3` is older than 3.14 or without `venv`.

The engine's own Linux status (tested on a few AMD and NVIDIA cards, not on Intel or the Deck) is in
[engine/docs/building.md](engine/docs/building.md#linux-in-progress).
