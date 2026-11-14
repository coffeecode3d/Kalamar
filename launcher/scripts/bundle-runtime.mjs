// Build-time step, on Windows and Linux: snapshots the engine sources the install wizard compiles into
// src-tauri/resources (runtime-src.tar.gz + runtime-src.json; the manifest names the snapshot, shown by the wizard and
// the About page).
//
//   node scripts/bundle-runtime.mjs [--repo <engine repository>] [--ref <ref>] [--kit-tools <dir>]
//
// The same logic as bundle-runtime.ps1 (which keeps working): `git archive` of the engine sources (read only: nothing
// is checked out or changed there), the helper scripts of launcher/kit-tools added under kit-tools/, and the archive
// refused if any file fails the "nothing derived from the game" scan (examples/bundle_runtime.rs). `npm run tauri
// build` runs it first (tauri.conf.json beforeBuildCommand).
//
// The manifest names the snapshot by the git tree id of the engine sources: the same sources have the same id in
// every repository, whatever its history. The archive is remade only when that id, or a helper script, differs from
// what the manifest records; a fresh clone therefore builds without touching the committed archive.
//
// Where the engine sources are:
// - by default, the engine/ folder of the repository that holds this launcher (ref HEAD);
// - with --repo or KALAMAR_RUNTIME_REPO: the engine in a repository of its own (ref main unless --ref says otherwise).
// Without either (no git checkout at all, for example a source archive), the committed archive is kept.

import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const launcher = resolve(here, "..");
const parent = resolve(launcher, "..");

function option(name) {
  const i = process.argv.indexOf(name);
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : "";
}

let repo = option("--repo") || process.env.KALAMAR_RUNTIME_REPO || process.env.SPLAT3_RUNTIME_REPO || "";
let ref = option("--ref");
let kitTools = option("--kit-tools") || process.env.KALAMAR_KIT_TOOLS || process.env.SPLAT3_KIT_TOOLS || join(launcher, "kit-tools");
let subdir = "";

if (!repo && existsSync(join(parent, "engine", "recomp", "gen.py")) && existsSync(join(parent, ".git"))) {
  repo = parent;
  subdir = "engine";
}
if (!ref) ref = subdir ? "HEAD" : "main";

function git(args, { text = true } = {}) {
  const r = spawnSync("git", args, { encoding: text ? "utf8" : "buffer" });
  if (r.status !== 0) {
    console.error(`bundle-runtime: git ${args.join(" ")}: ${String(r.stderr || "").trim()}`);
    process.exit(1);
  }
  return r.stdout;
}

function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

if (!repo || !existsSync(join(repo, ".git"))) {
  console.log("bundle-runtime: no engine repository found (name one with --repo or KALAMAR_RUNTIME_REPO); keeping the committed src-tauri/resources/runtime-src.*");
  process.exit(0);
}

const commit = git(["-C", repo, "rev-parse", "--verify", `${ref}^{commit}`]).trim();
const treeSpec = subdir ? `${commit}:${subdir}` : `${commit}^{tree}`;
const tree = git(["-C", repo, "rev-parse", "--verify", treeSpec]).trim();

const manifestPath = join(launcher, "src-tauri", "resources", "runtime-src.json");
if (existsSync(manifestPath)) {
  const man = JSON.parse(readFileSync(manifestPath, "utf8"));
  const kitSame = (man.kit_tools || []).every(([path, want]) => {
    const file = join(kitTools, path.replace(/^kit-tools\//, ""));
    return existsSync(file) && sha256(file) === want;
  });
  if (man.commit === tree && kitSame) {
    console.log(`bundle-runtime: already at engine snapshot ${tree}`);
    process.exit(0);
  }
}

const args = ["run", "--quiet", "--example", "bundle_runtime", "--", "--repo", repo, "--commit", commit, "--kit-tools", kitTools];
if (subdir) args.push("--subdir", subdir);
const built = spawnSync(process.platform === "win32" ? "cargo.exe" : "cargo", args, { cwd: join(launcher, "src-tauri"), stdio: "inherit" });
if (built.status !== 0) {
  console.error(`bundle-runtime: refused or failed (exit ${built.status})`);
  process.exit(built.status ?? 1);
}

// The archive landed in resources/; a short note so a release log says what was snapshotted.
try {
  const man = JSON.parse(readFileSync(manifestPath, "utf8"));
  console.log(`bundle-runtime: engine snapshot ${man.commit} (${man.files} files, ${man.bytes} bytes)`);
} catch {
  // Nothing to add: the example already printed what it wrote.
}
