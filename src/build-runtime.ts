/**
 * [INPUT]: 依赖唯一资产目录、共享文案与Bun/TypeScript；adapter pin只内嵌动作产物。
 * [OUTPUT]: 压缩外部脚本并生成同代dist/manifest；不把device-key adapter加入catalog。
 * [POS]: 源码到嵌入 Rust CLI 产物的唯一构建入口。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { lstatSync, mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { minify } from "terser";
import { ACCESSIBILITY_SETUP_COPY } from "./runtime/incognito-copy.ts";
import { sharedPermissionCopy } from "./permission-shared-copy.ts";
import {
  RUNTIME_ARTIFACT_NAMES,
  RUNTIME_EXTERNAL_ARTIFACT_NAMES,
  RUNTIME_LOADER_NAME,
  writeRuntimeManifest,
} from "./runtime-manifest.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "dist");
// The committed native manifest and binary are the build-time pin source.
const deviceKeyAdapterPin = readDeviceKeyAdapterPin(join(root, "native/macos/dist"));
mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, "incodex-permission-copy.json"), `${JSON.stringify(sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY))}\n`);

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
  cmd: ["bun", "build", injectTmp, "--outfile", injectOut, "--target", "browser", "--minify-whitespace"],
  cwd: root,
  stdout: "inherit",
  stderr: "inherit",
});
removeTemporaryFile(injectTmp);
if (inject.exitCode !== 0) {
  process.exit(inject.exitCode ?? 1);
}
// Content identity covers all bundled UI dependencies. Hash before replacing
// the identity token so repeated builds stay deterministic (no self hash).
const injectorBytes = await compactRuntime(readFileSync(injectOut, "utf8"));
const rendererBuildMarker = "__INCODEX_RENDERER_BUILD_ID__";
if (injectorBytes.split(rendererBuildMarker).length !== 2) {
  throw new Error("Renderer must contain exactly one generation identity marker");
}
writeFileSync(injectOut, injectorBytes.replace(
  rendererBuildMarker, createHash("sha256").update(injectorBytes).digest("hex"),
));

const emitted = spawnSync(
  process.execPath,
  [join(root, "node_modules/typescript/bin/tsc"), "-p", "tsconfig.runtime-emit.json"],
  {
    cwd: root,
    encoding: "utf8",
    stdio: "inherit",
  },
);
if (emitted.status !== 0) process.exit(emitted.status ?? 1);

const emitDir = join(root, ".runtime-cjs");
async function embeddedCjs(file: string): Promise<string> {
  const compact = await minify(readFileSync(join(emitDir, file), "utf8"), {
    module: false, compress: false, mangle: { toplevel: true }, format: { comments: false },
  });
  if (!compact.code) throw new Error(`Empty embedded Runtime module: ${file}`);
  return `(() => { const module = { exports: {} }; const exports = module.exports; ${compact.code}\nreturn module.exports; })()`;
}
const localeModule = await embeddedCjs("incodex-locale.cjs");
const placeholderModule = await embeddedCjs("incodex-permission-placeholder.cjs");
const cardModule = await embeddedCjs("incodex-permission-card.cjs");
const graphicsModule = await embeddedCjs("incodex-permission-graphics.cjs");
const motionModule = await embeddedCjs("incodex-permission-motion.cjs");
const permissionNativeModule = await embeddedCjs("incodex-permission-native.cjs");
const permissionProbeModule = await embeddedCjs("incodex-permission-probe.cjs");
const quitTraceModule = await embeddedCjs("incodex-quit-trace.cjs");
const deviceKeyHotModule = await embeddedCjs("incodex-device-key-hot.cjs");
const remoteKeySource = readFileSync(join(emitDir, "incodex-remote-key-compat.cjs"), "utf8")
  .replace('require("./incodex-permission-native.cts")', permissionNativeModule);
const remoteKeyCompact = await minify(remoteKeySource, { compress: false, mangle: { toplevel: true } });
if (!remoteKeyCompact.code) throw new Error("Remote key module compaction failed");
const remoteKeyModule = `(() => { const module = { exports: {} }; const exports = module.exports; ${remoteKeyCompact.code}\nreturn module.exports; })()`;
const nativeMotionSource = readFileSync(join(emitDir, "incodex-permission-native-motion.cjs"), "utf8")
  .replace('require("./incodex-permission-native.cts")', permissionNativeModule)
  .replace('require("./incodex-permission-motion.cts")', motionModule)
  .replace('require("./incodex-permission-graphics.cts")', graphicsModule);
const nativeMotion = await minify(nativeMotionSource, {
  module: false, compress: false, mangle: { toplevel: true }, format: { comments: false },
});
if (!nativeMotion.code) throw new Error("Native permission motion compaction produced no code");
const nativeMotionModule = `(() => { const module = { exports: {} }; const exports = module.exports; ${nativeMotion.code}\nreturn module.exports; })()`;
const cjsNames = RUNTIME_ARTIFACT_NAMES.filter((name) => name.endsWith(".cjs"));
for (const name of cjsNames) {
  const outputPath = join(outDir, name);
  let text = readFileSync(join(emitDir, name), "utf8")
    .replaceAll("\r\n", "\n")
    .replaceAll("\r", "\n")
    .replace(
      /require\("\.\/(incodex-[a-z-]+)\.cts"\)/g,
      'require("./$1.cjs")',
    );
  if (name === RUNTIME_LOADER_NAME) {
    text = embedRuntimeArtifactNames(text);
  }
  if (name === "incodex-main.cjs") {
    text = text.replace('"__INCODEX_QUIT_TRACE__"', quitTraceModule);
    text = text.replace('"__INCODEX_PERMISSION_PROBE__"', permissionProbeModule);
    text = text.replace('"__INCODEX_REMOTE_KEY_COMPAT__"', remoteKeyModule);
    text = text.replace('"__INCODEX_ACCESSIBILITY_COPY__"', 'loadVerifiedRuntimeJson("incodex-permission-copy.json")')
      .replace('"__INCODEX_ACCESSIBILITY_LOCALE__"', localeModule);
    text = text.replace('"__INCODEX_ACCESSIBILITY_WINDOW__"',
      'loadVerifiedRuntimeModule("incodex-permission-ui.cjs")');
  }
  if (name === "incodex-main-actions.cjs") {
    text = embedDeviceKeyHotFeature(text, deviceKeyHotModule, deviceKeyAdapterPin);
  }
  if (name === "incodex-permission-ui.cjs") {
    // Both entry points load one verified presenter/motion artifact. Do not
    // duplicate the native UI or require the side-effectful Electron main.
    const guideSource = readFileSync(join(emitDir, "incodex-accessibility-native.cjs"), "utf8")
      .replace('require("./incodex-permission-native.cts")', permissionNativeModule)
      .replace('require("./incodex-permission-graphics.cts")', graphicsModule)
      .replace('require("./incodex-permission-card.cts")', cardModule)
      .replace('require("./incodex-permission-placeholder.cts")', placeholderModule);
    const guide = await minify(guideSource, {
      module: false, compress: false, mangle: { toplevel: true }, format: { comments: false },
    });
    if (!guide.code) throw new Error("Permission guide compaction produced no code");
    text = text.replace('"__INCODEX_ACCESSIBILITY_WINDOW__"',
      `(() => { const module = { exports: {} }; const exports = module.exports; ${guide.code}\nreturn { ...module.exports, ...${nativeMotionModule} }; })()`);
  }
  if (name !== RUNTIME_LOADER_NAME) text = await compactRuntime(text);
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

// ---- 只缩短局部名称；保留入口、函数名、属性名和 require 路径。 ----
async function compactRuntime(source: string): Promise<string> {
  const compact = await minify(source, {
    module: false,
    compress: false,
    mangle: { toplevel: false },
    keep_fnames: true,
    format: { comments: false },
  });
  if (!compact.code) throw new Error("Runtime compaction produced no code");
  return `${compact.code}\n`;
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

function embedRuntimeArtifactNames(source: string): string {
  const marker = '"__INCODEX_RUNTIME_FILES__"';
  if (source.split(marker).length !== 2) {
    throw new Error("Runtime loader must contain exactly one artifact catalog marker");
  }
  return source.replace(marker, JSON.stringify(RUNTIME_EXTERNAL_ARTIFACT_NAMES));
}

function embedDeviceKeyHotFeature(
  source: string,
  moduleSource: string,
  pin: { manifestSha256: string; adapterSha256: string },
): string {
  const marker = '"__INCODEX_DEVICE_KEY_HOT_FEATURE__"';
  if (source.split(marker).length !== 2) {
    throw new Error("Main action factory must contain exactly one device-key feature marker");
  }
  return source.replace(marker,
    `(${moduleSource}).createDeviceKeyHotFeature(${JSON.stringify(pin)})`);
}

function readDeviceKeyAdapterPin(nativeDist: string): { manifestSha256: string; adapterSha256: string } {
  const manifestName = "incodex-device-key-adapter-manifest.json";
  const adapterName = "incodex-device-key-adapter.node";
  const manifestPath = join(nativeDist, manifestName);
  const adapterPath = join(nativeDist, adapterName);
  const manifestStat = lstatSync(manifestPath);
  const adapterStat = lstatSync(adapterPath);
  if (!manifestStat.isFile() || manifestStat.isSymbolicLink() || manifestStat.size > 64 * 1024 ||
      !adapterStat.isFile() || adapterStat.isSymbolicLink() || adapterStat.size > 1024 * 1024) {
    throw new Error("Invalid device-key adapter build input");
  }
  const manifestBytes = readFileSync(manifestPath);
  const adapterBytes = readFileSync(adapterPath);
  const manifest = JSON.parse(manifestBytes.toString("utf8"));
  const adapterSha256 = createHash("sha256").update(adapterBytes).digest("hex");
  const manifestSha256 = createHash("sha256").update(manifestBytes).digest("hex");
  if (manifest.schemaVersion !== 1 || manifest.platform !== "macos" || manifest.abiVersion !== 1 ||
      manifest.minimumMacOS !== "12.0" || JSON.stringify(manifest.architectures) !== '["arm64","x86_64"]' ||
      typeof manifest.sourceSha256 !== "string" || !/^[a-f0-9]{64}$/.test(manifest.sourceSha256) ||
      Object.keys(manifest.files || {}).length !== 1 || manifest.files[adapterName] !== adapterSha256) {
    throw new Error("Device-key adapter manifest does not match its native binary");
  }
  return { manifestSha256, adapterSha256 };
}
