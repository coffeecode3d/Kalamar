# Releasing Kalamar, and how it updates itself

Kalamar updates itself from the **GitHub Releases** of one repository, and only from releases **signed with the
maintainers' key**. A binary is never trusted because it comes from GitHub: a release is installed only if its manifest is
signed by a key compiled into the launcher, names that repository, a version higher than the running one, and the
exact size and SHA-256 of the file downloaded.

A build that names no repository (the default, every development build) has no update feature at all: nothing in the
interface, no network access.

- Code: `src-tauri/src/update/` (`channel.rs` configuration, `check.rs` the API, `net.rs` hosts and downloads,
  `manifest.rs` signature and manifest, `apply.rs` replacement and roll-back), `src/update/` (`store.ts` the
  state, `prompt.ts` which prompt shows when, `UpdateBar.tsx` the bar, the dialog and the badge,
  `LauncherUpdate.tsx` Settings › About).
- Tools: `src-tauri/examples/release_sign.rs` (manifest, check), the official `minisign` tool (keys, signature),
  `scripts/make-release.ps1` (the whole release folder).
- Public keys: `src-tauri/update-keys.txt`. **Secret keys never enter this repository.**

## 1. How a launcher updates

1. **Check.** 20 s after every start of the launcher, then every 6 hours while it stays open, if "Check automatically"
   is on (Settings › About, on by default; the switch turns all of it off), and when the player presses "Check for
   updates". The check at each start costs almost nothing when nothing changed: the ETag makes GitHub answer 304 with no
   body. One request to `api.github.com`
   (`/repos/<repo>/releases/latest`, or the last 10 releases on the beta channel), with `If-None-Match` (GitHub's ETag)
   and no token, cookie or identifier (`User-Agent: Kalamar-Launcher`). Only the tag name and the pre-release/draft
   flags of the answer are read. For a tag above the running version, the launcher downloads
   `kalamar-release.json` and `kalamar-release.json.minisig` from that release and checks them; nothing else. A
   failure (network, rate limit, refused manifest) shows nothing on the bar; Settings › About says it.
2. **Offer.** Everywhere, so that a player who lives on the Community pages or in the Locker sees it
   (`src/update/UpdateBar.tsx`, rules in `src/update/prompt.ts`, tested in `prompt.test.mjs`):
   - **The bar**, at the top of the content area of every page (Play, Locker, Community and its sub-pages, Settings,
     the install wizard), above the page so it never covers it. It names the version, says the state and carries the
     state's main action: "Kalamar update available: 0.2.0" (**Update** / **Later**); "Downloading Kalamar 0.2.0… 42 %"
     (**Cancel**); "Kalamar 0.2.0 is ready." (**Restart to finish**, or **Start the installation**, or **Choose a
     folder…**); the same with the button off and "Possible once the game is closed." (or "…the game installation is
     over." / "…the relay started by Kalamar is stopped.") while a game, an installation or a relay runs; the failure
     of a download or an apply, with **Try again**; the result of the last update ("Kalamar was updated: version 0.2.0.",
     "Version 0.2.0 did not start properly: Kalamar 0.1.0 was put back. Nothing is lost."). It uses the commands of
     Settings › About; there is no second state machine.
   - **Later** (bar or dialog) puts the update off **until the next start of the launcher** (kept in memory, not on
     disk; a newer version found meanwhile shows again). The bar goes away and the **Settings** entry of the rail gets a
     dot that leads straight to Settings › About; the update stays one click away there. A download in progress, a
     ready update, a failure and a result are never hidden by "Later".
   - **The dialog**, once per run of the launcher, when an update waits for an answer: "Kalamar 0.2.0 is available",
     what is new (the signed manifest's notes as plain text, never HTML), **Update now** (starts the download; the
     bar then shows it) and **Later** (Escape too). It never opens over a match (a search, a found or running match, a
     private match connecting, a launch from the command line), an installation, a running game or another dialog, nor
     in the install wizard; it waits until the launcher has been idle for 1.5 s. The bar is compact (one line, no state
     text) while a match is on.
   - **The game update** ("Game update available" with its class and duration, "Game update needed: Splatoon 3 11.3.0",
     buttons **Update** / **Later**, the existing flow) uses the same bar on every page but Play, which carries it in
     full. The launcher's update comes first: the new launcher usually carries the new engine code and sometimes the new
     game version, so what the game update would do is only known once the launcher is current. "Later" on a game update
     is also kept until the next start, with a dot on Settings that leads to Settings › Installed game.
   Nothing is downloaded before the click.
3. **Download.** In the background into `<data>\update\download\<tag>\` (`KALAMAR_DATA` respected), with
   progress, resumed with a `Range` request after a cut (three retries), stopped as soon as it passes the signed size;
   then the SHA-256 is compared with the manifest. A mismatch deletes the file.
4. **Apply**, on a second click, and only when no game, game installation or relay runs:
   - **Portable copy** (`Kalamar.exe` in a folder it can write): "Restart to finish". The file is checked again,
     copied beside the program as `Kalamar.exe.new`, checked again; `Kalamar.exe` is renamed `Kalamar.exe.old` (Windows
     allows renaming a running program, not overwriting it) and `Kalamar.exe.new` becomes `Kalamar.exe`; the new
     version starts with `--update-trial` while the old one hides its window and waits up to 90 s for the new one's
     **witness** (`<data>\update\witness.json`, written once its page is up and has its data). Witness: the old process
     quits and the new one deletes `Kalamar.exe.old`. No witness (crash, early exit, hang): the old process stops the
     new one, puts `Kalamar.exe.old` back and shows its window with "Version 0.2.0 did not start properly: Kalamar
     0.1.0 was put back". Every step is in `<data>\update\journal.json`; after a power cut or a killed process, the
     next start of `Kalamar.exe` finishes or undoes the update before anything else (a new version that already had
     its trial without a witness puts the old one back and starts it).
   - **Installed copy** (Tauri's NSIS installer put `uninstall.exe` beside it, for example under Program Files):
     "Start the installation" runs the release's installer, checked again right before, through the shell (it asks for
     elevation itself), and Kalamar quits. The next start says whether the version changed.
   - **Copy that cannot replace itself** (run from a temporary folder, typically straight from an archive, or from a
     folder it cannot write): "Choose a folder…" saves the checked new `Kalamar.exe` there (never overwriting a file:
     `Kalamar-<version>.exe` if the name is taken) and says to start it from there.
5. **Clean-up.** The next start removes `Kalamar.exe.old`, `.new`, `.failed` left beside the program, and the
   download once the version is installed.

Also handled: another Kalamar running from the same file ("close it first"), two launchers downloading at once (a lock
file), an antivirus holding a file (renames retried for 6 s, then "try again later", nothing changed), a full disk,
a release withdrawn from GitHub (no longer offered), going back from the beta to the stable channel (never an older
version).

**The game update.** The runtime source the install wizard compiles is embedded in `Kalamar.exe`; after an update the
launcher uses its own embedded copy (an installed copy's `resources\` folder of an older version is ignored), so the
Play page's "Game update available" appears as usual when the new launcher carries other engine code.

## 2. What is signed

A release carries, besides its programs, **`kalamar-release.json`** (at most 64 KiB, UTF-8 JSON, unknown fields
refused):

```json
{
  "format": 1,
  "product": "kalamar",
  "repo": "owner/kalamar",
  "version": "0.2.0",
  "channel": "stable",
  "published": "2026-11-12T10:00:00Z",
  "notes": "Kalamar 0.2.0\n- what changed, plain text",
  "files": [
    { "name": "Kalamar.exe", "kind": "portable", "size": 13271552, "sha256": "<64 lower-case hex>" },
    { "name": "Kalamar_0.2.0_x64-setup.exe", "kind": "installer", "size": 6932206, "sha256": "<64 lower-case hex>" }
  ]
}
```

and **`kalamar-release.json.minisig`**, a [minisign](https://jedisct1.github.io/minisign/) signature of the exact
bytes of the manifest: Ed25519 over their BLAKE2b-512 hash (minisign's default "prehashed" mode; legacy signatures are
refused), plus minisign's global signature over the trusted comment. Keys and signatures are standard minisign files:
the official `minisign` tool can make and check them too (`minisign -Vm kalamar-release.json -p kalamar.pub`).

The launcher refuses the release when: the signature is not from a key of `update-keys.txt`; the manifest is not
format 1, product `kalamar`, the configured repository; its version is not the release tag's (`v0.2.0` or `0.2.0`);
its version is not higher than the running one (no going back, ever); it is a `beta` manifest and the player is on
the stable channel; a file name holds anything but letters, digits, `.`, `_`, `-` or does not end in `.exe`; a size is
0 or above 512 MiB; a hash is not 64 lower-case hex digits. The notes are shown as text (control, zero-width and
bidirectional-override characters removed, 8000 characters at most); HTML and Markdown are shown as typed, links are
not clickable.

## 3. Setting up the release repository (once)

1. **Create the repository** on GitHub, public (the launcher reads its releases without a token). Turn on two-factor
   authentication on the account; whoever can publish releases there can make players download files (they still
   need your signature to have them installed).
2. **Make two key pairs** with the official [minisign](https://jedisct1.github.io/minisign/) tool (0.10 or later, kept
   outside any git working tree), on a computer you trust, each written to removable media kept offline:

   ```powershell
   E:\tools\minisign.exe -G -p E:\kalamar-keys\kalamar.pub -s E:\kalamar-keys\kalamar.key
   E:\tools\minisign.exe -G -p F:\kalamar-keys-backup\kalamar.pub -s F:\kalamar-keys-backup\kalamar.key -c "Kalamar backup key"
   ```

   Each asks for a password (long; the secret key file is encrypted with it, scrypt; there is no recovery). The
   **public key line** (56 characters starting with `RW`) is the second line of `kalamar.pub`. The first pair is the
   **current key**, the second the **backup key**: keep them on different media, in different places, with their
   passwords in a password manager. No program built from this repository reads a secret key: the example
   `release_sign` links the whole launcher and its dependencies, and one compromised dependency in a process that
   decrypts the key could steal it.
3. **Put both public key lines** in `src-tauri/update-keys.txt`, current first, backup second, and commit. Public keys
   are not secret.
4. **Build the first release with the updater** (section 4). A launcher built without an update repository has no
   updater: they download this first release by hand; every later one comes by itself.

Never: commit a `.key` file, put a secret key or its password in a CI variable or a script, paste them in an issue, a
chat or a log, sign on a shared or untrusted machine.

## 4. Publishing a release

1. Set the same version in `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` and `package.json` (`0.2.0`; a beta is
   `0.3.0-beta.1`). Commit.
2. Write the notes in a plain-text file (outside the repository or not, as you like).
3. Plug in the key medium and run:

   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\make-release.ps1 -Repo owner/kalamar -Version 0.2.0 `
       -Notes D:\notes\0.2.0.txt -Key E:\kalamar-keys\kalamar.key -Minisign E:\tools\minisign.exe   # -Channel beta for a beta
   ```

   It checks the versions, the clean working tree, the key's and minisign's places and `update-keys.txt`; builds with
   `KALAMAR_UPDATE_REPO=owner/kalamar` (`npm run tauri:release`: `scripts\release-build.ps1` remaps the paths of this
   machine, the user's profile with Cargo's registry first, out of the program); checks that the program carries the
   repository and the public keys, and no trace of this machine (user name, `Users\<name>`, the repository's path); writes `release\0.2.0\` with `Kalamar.exe`, `Kalamar_0.2.0_x64-setup.exe`, `RELEASE-NOTES.txt`,
   `kalamar-release.json`; has minisign sign it (minisign asks for the key's password; `kalamar-release.json.minisig`); runs the launcher's
   own check on the folder; writes `SHA256SUMS`. Nothing is uploaded. Unplug the key medium. With
   `-DiscordClientId <application id>` the release also has the opt-in Discord presence (the id of your Discord
   application: [discord-presence.md](discord-presence.md)); without it, none.
4. Tag and push the tag: `git tag v0.2.0 <commit>` then `git push origin v0.2.0`.
5. On GitHub, **Releases › Draft a new release**: tag `v0.2.0`, title `Kalamar 0.2.0`, the notes as text; attach
   **every file** of `release\0.2.0\` (the launcher needs `kalamar-release.json` and `kalamar-release.json.minisig`;
   players who download by hand use `SHA256SUMS`); for a beta tick **Set as a pre-release**, else **Set as the latest
   release**; **Publish**.
6. Check from an older launcher: Settings › About › Check for updates.

Never replace a file of a published release: launchers that already have the manifest would refuse the new bytes. To
fix a bad release, publish a higher version. Deleting a release makes launchers stop offering it at their next check.

**Beta.** Players who choose the beta channel see pre-releases (and stable releases), always only higher versions. A
pre-release version must be signed with `-Channel beta`; a stable launcher refuses a beta manifest even if the
release is not marked as a pre-release.

## 5. A lost or leaked key: rotation

Launchers accept a manifest signed by **any** key of their `update-keys.txt`; that is what the backup key is for.

- **Current key lost (not leaked).** Make a new backup key pair (section 3, step 2). Put in `update-keys.txt` the old
  backup key first (it becomes the current one) and the new backup key second. Publish the next release signed with
  the old backup key: every launcher in the field accepts it, and from then on carries the new pair of keys.
- **Current key leaked** (or its medium stolen with its password): the same, at once, and also check the GitHub
  account (password, two-factor, tokens, collaborators, releases published since). The release made with the backup
  key no longer lists the leaked key, so updated launchers refuse it. Launchers not yet updated still accept it, but
  an attacker would also need to publish in your repository, and could not make them go back to an older version.
- **Both keys lost or leaked.** No launcher in the field will accept a new release. Make a new pair of key pairs, build
  a release with them and announce in the repository that players must download it by hand (and why).
- **Repository renamed.** The manifest names the repository compiled into the launchers that read it. Releases meant
  for launchers built with the old name must keep the old name in `"repo"` (sign them with `release_sign manifest
  --repo <old name>`); GitHub's redirects for renamed repositories stay on `api.github.com` and `github.com`, which the
  launcher follows.

## 6. Tests

- `cargo test` (in `src-tauri`): versions, the manifest and its signature (good, altered, another key, a forged key
  id, older or equal versions, a mismatched tag, another repository, a beta manifest on the stable channel, malformed
  fields), hosts and redirects, and end to end against a local release server on `127.0.0.1` with a key the tests
  generate: check with ETag, a download cut and resumed, the swap of a copy of the test executable in a temporary
  folder, its restart and witness, the roll-back of a new version that crashes or hangs, an altered or longer file,
  a redirect off the list, a rate limit, the installer and "save elsewhere" cases. Nothing reaches GitHub.
- `cargo test` also covers the cadence (`auto_wait`: 20 s, then 6 h; a check at each start still sends the ETag and
  fetches no manifest again) and "Later" kept for one run only. `npm run test:ui` runs `src/update/prompt.test.mjs`:
  which prompt shows for each state, page, busy condition (match, installation, game) and "Later".
- `npm run dev:mock` then `?lup=available` (or `downloading`, `ready`, `blocked&game=running`, `relay`, `installer`,
  `elsewhere`, `readonly`, `saved`, `integrity`, `updated`, `rolledback`, `later`, `uptodate`, `error`, `untrusted`,
  `beta`) and `&page=profile`, `&server=service&page=community`, `&page=settings&section=about`: every state of the
  bar, the badge and the page's line in About. The dialog opens 1.5 s after the page is up; `&dlg=0` says it was
  already answered (the bar alone). `&upd=short|long|full` or `&ver=old` with another page than Play shows the game
  update on the bar. The screenshots are the `lup-*` and `gup-*` entries of `scripts/ui-shots.list.mjs`.
- `cargo run --example release_sign -- verify --dir <release folder> --repo owner/kalamar` repeats the launcher's check
  on a release folder at any time.
- A trial run of the release tools without your key: `KALAMAR_TRIAL_DIR=<folder outside the repository>`,
  `KALAMAR_TRIAL_STEP=keys`, `cargo test --lib release_trial_helper -- --ignored` writes a throwaway key pair there
  (password `trial`); put its `pub.txt` line in `update-keys.txt` for the trial only (never commit it), run
  `make-release.ps1 ... -Key <folder>\kalamar.key -Minisign <minisign.exe> -Out <folder>\out -AllowDirty`, then remove
  the line.

## 7. A release built by CI (optional, off by default)

The procedure above builds and signs on the maintainer's machine. The repository also carries a workflow,
`.github/workflows/release.yml`, that does the build on GitHub's runners. It is **off** by default: it runs on a `v*`
tag only when the repository variable `KALAMAR_RELEASE_CI` is `enabled`. What it does:

1. checks that the tag (`v0.2.0`) matches the versions of `package.json`, `src-tauri/tauri.conf.json` and
   `src-tauri/Cargo.toml`, and takes the release notes from the tag's message (`git tag -a v0.2.0 -m "..."`);
2. builds with `npm run tauri:release` and `KALAMAR_UPDATE_REPO` set to the repository (and
   `KALAMAR_DISCORD_CLIENT_ID` from the repository variable of that name, when set: the id is public);
3. writes `kalamar-release.json` (`release_sign manifest`), `RELEASE-NOTES.txt` and `SHA256SUMS`, keeps them as a
   workflow artifact, and creates a **draft** release with them.

It never signs. Two ways to sign a release built this way, with their risks:

**A. Signed by the maintainer, offline (the workflow as written).** Download `kalamar-release.json` from the draft,
sign it on the trusted machine with the key medium and the official minisign tool (`minisign -S -s <key> -m
kalamar-release.json`; it asks for the key's password), attach `kalamar-release.json.minisig` to the draft, run
`release_sign verify` on a folder with every file of the draft, then publish. The key never leaves the offline medium. The risk moves to the build: the
signature now vouches for programs built by GitHub's runners and the actions the workflow uses (pinned by commit), so
a compromised runner, action or dependency would be signed too. Mitigations: compare the draft's `SHA256SUMS` with a
build of your own when the build is reproducible, review every change to the workflow (CODEOWNERS), keep the actions
pinned.

**B. Signed in CI with a key held as a secret.** Not implemented: no program built from this repository reads a
secret key (section 3), so it would need minisign itself in the workflow with the key's password fed to it from a
secret, and a **separate CI key** listed in `update-keys.txt` beside the offline ones. Anyone who can change the workflow on a branch that tags releases, or who obtains the secret, could
then sign a release that every launcher installs; the offline backup key (section 5) is the only way back. This goes
against the rule of section 3 ("never put a secret key or its password in a CI variable"), and is not recommended
unless releases must be made without the maintainer's machine.

Whichever is chosen, `SHA256SUMS` written by the workflow does not list the `.minisig`, which is checked on its own.
