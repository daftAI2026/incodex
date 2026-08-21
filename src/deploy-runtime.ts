import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "dist");
const home = process.env.HOME;
if (!home) {
  console.error("HOME is unset; refusing to write a relative .incodex path");
  process.exit(1);
}

const files = [
  "incodex-inject.js",
  "incodex-loader.cjs",
  "incodex-main.cjs",
  "incodex-preload.cjs",
  "incodex-safe-home.cjs",
  "incodex-ipc-guard.cjs",
  "incodex-owner-core.cjs",
  "incodex-owner-recovery.cjs",
  "incodex-instance.cjs",
  "incodex-runtime-load.cjs",
  "incodex-window-kind.cjs",
];

const targetsDir = join(home, ".incodex", "targets");
if (!existsSync(targetsDir)) {
  console.log("no ~/.incodex/targets yet; start Codex once, then rerun deploy:runtime");
  process.exit(0);
}

let copied = 0;
for (const name of readdirSync(targetsDir)) {
  const dest = join(targetsDir, name);
  mkdirSync(dest, { recursive: true, mode: 0o700 });
  for (const file of files) {
    const src = join(outDir, file);
    if (!existsSync(src)) continue;
    writeFileSync(join(dest, file), readFileSync(src));
  }
  copied += 1;
  console.log("deployed runtime to", dest);
}

if (copied === 0) {
  console.log("no target directories under ~/.incodex/targets");
}
console.log("set INCODEX_DEV_HOT=1 when launching Codex to load these files");
