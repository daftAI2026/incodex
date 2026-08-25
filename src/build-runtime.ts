import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { RUNTIME_ARTIFACT_NAMES, writeRuntimeManifest } from "./runtime-manifest.ts";

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

const injectTmp = join(root, "src/runtime/_inject.src.ts");
writeFileSync(injectTmp, injectSrc);

const injectOut = join(outDir, "incodex-inject.js");

const inject = Bun.spawnSync({
  cmd: ["bun", "build", injectTmp, "--outfile", injectOut, "--target", "browser"],
  cwd: root,
  stdout: "inherit",
  stderr: "inherit",
});
removeTemporaryFile(injectTmp);
if (inject.exitCode !== 0) {
  process.exit(inject.exitCode ?? 1);
}

const emitted = spawnSync(join(root, "node_modules/typescript/bin/tsc"), ["-p", "tsconfig.runtime-emit.json"], {
  cwd: root,
  encoding: "utf8",
  stdio: "inherit",
});
if (emitted.status !== 0) process.exit(emitted.status ?? 1);

const emitDir = join(root, ".runtime-cjs");
const cjsNames = RUNTIME_ARTIFACT_NAMES.filter((name) => name.endsWith(".cjs"));
for (const name of cjsNames) {
  const outputPath = join(outDir, name);
  const text = readFileSync(join(emitDir, name), "utf8").replace(
    /require\("\.\/(incodex-[a-z-]+)\.cts"\)/g,
    'require("./$1.cjs")',
  );
  writeFileSync(outputPath, text);
}
assertPortableCjs(cjsNames.map((name) => join(outDir, name)));

const artifactPaths = RUNTIME_ARTIFACT_NAMES.map((name) => join(outDir, name));
const files = Object.fromEntries(
  artifactPaths.map((file) => [
    file.slice(outDir.length + 1),
    createHash("sha256").update(readFileSync(file)).digest("hex"),
  ]),
);
writeRuntimeManifest(outDir, {
  runtimeVersion: packageVersion(),
  sourceCommit: process.env.SOURCE_COMMIT || "",
  files,
});

for (const file of artifactPaths) {
  console.log("wrote", file);
}

function packageVersion(): string {
  const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as {
    version?: string;
  };
  return packageJson.version || "0.0.0";
}

function removeTemporaryFile(file: string): void {
  try {
    unlinkSync(file);
  } catch {
    /* 临时文件清理只做 best effort。 */
  }
}

function assertPortableCjs(files: string[]): void {
  const banned = /\/Users\/|\/home\/|file:\/\/|C:\\\\Users\\/;
  for (const file of files) {
    const text = readFileSync(file, "utf8");
    if (banned.test(text)) {
      throw new Error(`${file} contains a machine-specific path; runtime CJS must use __dirname`);
    }
  }
}
