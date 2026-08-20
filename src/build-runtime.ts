import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { writeRuntimeManifest } from "./runtime-manifest";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "dist");
mkdirSync(outDir, { recursive: true });

const hatGlassesSvg = readFileSync(join(root, "assets/hat-glasses.svg"), "utf8").trim();
const circleXSvg = readFileSync(join(root, "assets/circle-x.svg"), "utf8").trim();
function embedSvg(svg: string): string {
  return svg.replace(/`/g, "\\`").replace(/\$\{/g, "\\${");
}
const injectSrc = readFileSync(join(root, "src/runtime/inject.ts"), "utf8").replace(
  "{{HAT_GLASSES_SVG}}",
  embedSvg(hatGlassesSvg),
).replace(
  "{{CIRCLE_X_SVG}}",
  embedSvg(circleXSvg),
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
  cmd: ["bun", "build", injectTmp, "--outfile", injectOut, "--target", "browser"],
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

const emitted = spawnSync(join(root, "node_modules/typescript/bin/tsc"), ["-p", "tsconfig.runtime-emit.json"], {
  cwd: root,
  encoding: "utf8",
  stdio: "inherit",
});
if (emitted.status !== 0) process.exit(emitted.status ?? 1);

const emitDir = join(root, ".runtime-cjs");
const copies: Array<[string, string]> = [
  [join(emitDir, "incodex-loader.cjs"), loaderOut],
  [join(emitDir, "incodex-main.cjs"), mainOut],
  [join(emitDir, "incodex-preload.cjs"), preloadOut],
  [join(emitDir, "incodex-safe-home.cjs"), safeHomeOut],
  [join(emitDir, "incodex-ipc-guard.cjs"), ipcGuardOut],
  [join(emitDir, "incodex-instance.cjs"), instanceOut],
  [join(emitDir, "incodex-runtime-load.cjs"), runtimeLoadOut],
  [join(emitDir, "incodex-window-kind.cjs"), windowKindOut],
];
for (const [from, to] of copies) {
  const text = readFileSync(from, "utf8").replace(
    /require\("\.\/(incodex-[a-z-]+)\.cts"\)/g,
    'require("./$1.cjs")',
  );
  writeFileSync(to, text);
}
assertPortableCjs(copies.map(([, to]) => to));

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

function assertPortableCjs(files: string[]): void {
  const banned = /\/Users\/|\/home\/|file:\/\/|C:\\\\Users\\/;
  for (const file of files) {
    const text = readFileSync(file, "utf8");
    if (banned.test(text)) {
      throw new Error(`${file} contains a machine-specific path; runtime CJS must use __dirname`);
    }
  }
}
