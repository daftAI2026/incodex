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
const injectTmp = join(outDir, "_inject.src.ts");
writeFileSync(injectTmp, injectSrc);

const injectOut = join(outDir, "incodex-inject.js");
const loaderOut = join(outDir, "incodex-loader.cjs");

const inject = Bun.spawnSync({
  cmd: ["bun", "build", injectTmp, "--outfile", injectOut, "--target", "browser", "--minify"],
  cwd: root,
  stdout: "inherit",
  stderr: "inherit",
});
if (inject.exitCode !== 0) process.exit(inject.exitCode ?? 1);

writeFileSync(loaderOut, readFileSync(join(root, "src/runtime/incodex-loader.cjs")));

console.log("wrote", injectOut);
console.log("wrote", loaderOut);
