// The interface build Tauri runs before bundling: `tsc` then Vite, through Node directly so that no particular package
// manager has to be on PATH (`npm run build` stays the developer's command). Run `npm ci` (or `pnpm install`) first.

import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const launcher = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const bin = join(launcher, "node_modules", ".bin");
const exe = process.platform === "win32" ? ".cmd" : "";

function run(name, args) {
  const r = spawnSync(join(bin, name + exe), args, { cwd: launcher, stdio: "inherit" });
  if (r.error) {
    console.error(`build-ui: cannot run ${name}: ${r.error.message} (run npm ci or pnpm install first)`);
    process.exit(1);
  }
  if (r.status !== 0) process.exit(r.status ?? 1);
}

run("tsc", []);
run("vite", ["build"]);
