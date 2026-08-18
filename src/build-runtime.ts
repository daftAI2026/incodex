import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { writeRuntimeManifest } from "./runtime-manifest";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "dist");
mkdirSync(outDir, { recursive: true });

const svg = readFileSync(join(root, "assets/hat-glasses.svg"), "utf8").trim();
const injectSrc = readFileSync(join(root, "src/runtime/inject.ts"), "utf8").replace(
  "{{HAT_GLASSES_SVG}}",
  svg.replace(/`/g, "\\`").replace(/\$\{/g, "\\${"),
);
writeFileSync(join(outDir, "incognito-copy.ts"), readFileSync(join(root, "src/runtime/incognito-copy.ts")));
const injectTmp = join(root, "src/runtime/_inject.src.ts");
writeFileSync(injectTmp, injectSrc);

const injectOut = join(outDir, "incodex-inject.js");
const loaderOut = join(outDir, "incodex-loader.cjs");
const mainOut = join(outDir, "incodex-main.cjs");
const preloadOut = join(outDir, "incodex-preload.cjs");
const safeHomeOut = join(outDir, "incodex-safe-home.cjs");
const ipcGuardOut = join(outDir, "incodex-ipc-guard.cjs");
const instanceOut = join(outDir, "incodex-instance.cjs");
const runtimeLoadOut = join(outDir, "incodex-runtime-load.cjs");
const windowKindOut = join(outDir, "incodex-window-kind.cjs");

const inject = Bun.spawnSync({
  cmd: ["bun", "build", injectTmp, "--outfile", injectOut, "--target", "browser", "--minify"],
  cwd: root,
  stdout: "inherit",
  stderr: "inherit",
});
if (inject.exitCode !== 0) {
  try {
    unlinkSync(injectTmp);
  } catch {
    /* ignore */
  }
  process.exit(inject.exitCode ?? 1);
}
try {
  unlinkSync(injectTmp);
} catch {
  /* ignore */
}

const copies: Array<[string, string]> = [
  [join(root, "src/runtime/incodex-loader.cjs"), loaderOut],
  [join(root, "src/runtime/incodex-main.cjs"), mainOut],
  [join(root, "src/runtime/incodex-preload.cjs"), preloadOut],
  [join(root, "src/runtime/incodex-safe-home.cjs"), safeHomeOut],
  [join(root, "src/runtime/incodex-ipc-guard.cjs"), ipcGuardOut],
  [join(root, "src/runtime/incodex-instance.cjs"), instanceOut],
  [join(root, "src/runtime/incodex-runtime-load.cjs"), runtimeLoadOut],
  [join(root, "src/runtime/incodex-window-kind.cjs"), windowKindOut],
];
for (const [from, to] of copies) writeFileSync(to, readFileSync(from));

const artifactPaths = [
  injectOut,
  loaderOut,
  mainOut,
  preloadOut,
  safeHomeOut,
  ipcGuardOut,
  instanceOut,
  runtimeLoadOut,
  windowKindOut,
];
const files: Record<string, string> = {};
for (const file of artifactPaths) {
  files[file.slice(outDir.length + 1)] = createHash("sha256").update(readFileSync(file)).digest("hex");
}
writeRuntimeManifest(outDir, {
  runtimeVersion: (
    JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as { version?: string }
  ).version || "0.0.0",
  sourceCommit: process.env.SOURCE_COMMIT || "",
  files,
});

for (const file of artifactPaths) console.log("wrote", file);
