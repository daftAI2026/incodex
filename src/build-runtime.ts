import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "dist");
mkdirSync(outDir, { recursive: true });

const svg = readFileSync(join(root, "assets/hat-glasses.svg"), "utf8").trim();
const injectSrc = readFileSync(join(root, "src/runtime/inject.ts"), "utf8").replace(
  "{{HAT_GLASSES_SVG}}",
  svg.replace(/`/g, "\\`").replace(/\$\{/g, "\\${"),
);
writeFileSync(join(outDir, "incognito-copy.ts"), readFileSync(join(root, "src/runtime/incognito-copy.ts")));
const injectTmp = join(outDir, "_inject.src.ts");
writeFileSync(injectTmp, injectSrc);

const injectOut = join(outDir, "incodex-inject.js");
const loaderOut = join(outDir, "incodex-loader.cjs");
const mainOut = join(outDir, "incodex-main.cjs");
const preloadOut = join(outDir, "incodex-preload.cjs");
const safeHomeOut = join(outDir, "incodex-safe-home.cjs");
const ipcGuardOut = join(outDir, "incodex-ipc-guard.cjs");

const inject = Bun.spawnSync({
  cmd: ["bun", "build", injectTmp, "--outfile", injectOut, "--target", "browser", "--minify"],
  cwd: root,
  stdout: "inherit",
  stderr: "inherit",
});
if (inject.exitCode !== 0) process.exit(inject.exitCode ?? 1);

writeFileSync(loaderOut, readFileSync(join(root, "src/runtime/incodex-loader.cjs")));
writeFileSync(mainOut, readFileSync(join(root, "src/runtime/incodex-main.cjs")));
writeFileSync(preloadOut, readFileSync(join(root, "src/runtime/incodex-preload.cjs")));
writeFileSync(safeHomeOut, readFileSync(join(root, "src/runtime/incodex-safe-home.cjs")));
writeFileSync(ipcGuardOut, readFileSync(join(root, "src/runtime/incodex-ipc-guard.cjs")));

const hot = join(process.env.HOME ?? "", ".incodex");
if (hot) {
  mkdirSync(hot, { recursive: true });
  writeFileSync(join(hot, "incodex-inject.js"), readFileSync(injectOut));
  writeFileSync(join(hot, "incodex-main.cjs"), readFileSync(mainOut));
  writeFileSync(join(hot, "incodex-preload.cjs"), readFileSync(preloadOut));
  writeFileSync(join(hot, "incodex-safe-home.cjs"), readFileSync(safeHomeOut));
  writeFileSync(join(hot, "incodex-ipc-guard.cjs"), readFileSync(ipcGuardOut));
}

console.log("wrote", injectOut);
console.log("wrote", loaderOut);
console.log("wrote", mainOut);
console.log("wrote", preloadOut);
console.log("wrote", safeHomeOut);
console.log("wrote", ipcGuardOut);
