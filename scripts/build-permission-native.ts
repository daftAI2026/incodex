import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const root = join(import.meta.dir, "..");
const nativeRoot = join(root, "native", "macos");
const sourcePath = join(nativeRoot, "permission-views.swift");
// The native process owns only protocol/lifetime and ABI adaptation. Embed
// the existing JS UI verbatim; do not compile the retired duplicate presenter.
const hostSourcePaths = [
  join(nativeRoot, "permission-host-osa.swift"),
  join(nativeRoot, "permission-host.swift"),
  join(nativeRoot, "permission-host-bridge.m"),
  join(nativeRoot, "permission-host-runtime.js"),
  join(nativeRoot, "permission-host-objc.js"),
  join(root, "dist", "incodex-permission-ui.cjs"),
  join(root, "dist", "incodex-dock-menu.cjs"),
];
const distRoot = join(nativeRoot, "dist");
const dylibName = "incodex-permission-ui.dylib";
const hostName = "incodex-permission-host";
const dylibPath = join(distRoot, dylibName);
const hostPath = join(distRoot, hostName);
const manifestPath = join(distRoot, "runtime-native-manifest.json");
const minimumMacOS = "12.0";
const architectures = ["arm64", "x86_64"] as const;

type NativeManifest = {
  schemaVersion: 1;
  platform: "macos";
  abiVersion: 1;
  minimumMacOS: typeof minimumMacOS;
  architectures: string[];
  sourceSha256: string;
  hostSourceSha256: string;
  files: Record<string, string>;
};

function run(binary: string, args: string[], cwd = root): string {
  const result = spawnSync(binary, args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.error) throw result.error;
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
  if (result.status !== 0) {
    throw new Error(`${binary} ${args.join(" ")} failed with status ${result.status ?? "unknown"}\n${output}`);
  }
  return output;
}

function sha256(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function sha256Files(paths: string[]): string {
  const digest = createHash("sha256");
  for (const path of paths) digest.update(readFileSync(path));
  return digest.digest("hex");
}

function assertUniversalBinary(path: string): void {
  const lipoInfo = run("lipo", ["-info", path]);
  for (const architecture of architectures) {
    run("lipo", [path, "-verify_arch", architecture]);
    if (!lipoInfo.includes(architecture)) {
      throw new Error(`Universal permission native artifact is missing ${architecture}: ${lipoInfo}`);
    }
  }

  const loadCommands = run("otool", ["-l", path]);
  const minimumVersionMatches = loadCommands.match(new RegExp(`minos\\s+${minimumMacOS.replace(".", "\\.")}`, "g")) ?? [];
  if (minimumVersionMatches.length < architectures.length) {
    throw new Error(`Permission native artifact must target macOS ${minimumMacOS} in every slice`);
  }
}

function assertExports(path: string): void {
  const symbols = run("nm", ["-gU", path]);
  for (const className of [
    "IncodexPermissionFlightView",
    "IncodexPermissionArrowView",
    "IncodexPermissionDisplayLink",
  ]) {
    if (!symbols.includes(className)) {
      throw new Error(`Permission library does not export ${className}`);
    }
  }
  const strings = run("strings", [path]);
  for (const selector of [
    "setSourceImage:targetImage:",
    "updateProgress:cornerRadius:reduceTransparency:",
    "animateToScaleX:scaleY:",
    "resetToIdentity",
    "startForWindow:handler:",
    "startForScreen:handler:",
  ]) {
    if (!strings.includes(selector)) {
      throw new Error(`Permission library is missing ObjC selector ${selector}`);
    }
  }
}

function buildSlice(swiftc: string, sdkPath: string, architecture: string, output: string): void {
  run(swiftc, [
    "-parse-as-library",
    "-emit-library",
    "-module-name",
    "IncodexPermissionUI",
    "-disable-autolinking-runtime-compatibility",
    "-Xlinker",
    "-install_name",
    "-Xlinker",
    "@rpath/incodex-permission-ui.dylib",
    "-target",
    `${architecture}-apple-macos${minimumMacOS}`,
    "-sdk",
    sdkPath,
    sourcePath,
    "-o",
    output,
  ]);
}

function buildHostSlice(swiftc: string, sdkPath: string, architecture: string, output: string): void {
  const source = readFileSync(join(nativeRoot, "permission-host-runtime.js"), "utf8")
    .replace("// __INCODEX_OBJC_ADAPTER__", () => readFileSync(join(nativeRoot, "permission-host-objc.js"), "utf8"))
    .replace("__INCODEX_ORIGINAL_UI_JSON__", () => JSON.stringify(readFileSync(join(root, "dist", "incodex-permission-ui.cjs"), "utf8")))
    .replace("__INCODEX_ORIGINAL_LOCATOR_JSON__", () => JSON.stringify(readFileSync(join(root, "dist", "incodex-dock-menu.cjs"), "utf8")));
  let delimiter = "#";
  while (source.includes(`"""${delimiter}`) || source.includes(`\\${delimiter}(`)) delimiter += "#";
  const embedded = `${output}-source.swift`;
  writeFileSync(embedded, `enum PermissionHostOSASource { static let runtime = ${delimiter}"""\n${source}\n"""${delimiter} }\n`);
  const bridgeObject = `${output}-bridge.o`;
  run("xcrun", ["clang", "-fobjc-arc", "-target", `${architecture}-apple-macos${minimumMacOS}`,
    "-isysroot", sdkPath, "-c", join(nativeRoot, "permission-host-bridge.m"), "-o", bridgeObject]);
  run(swiftc, [
    "-parse-as-library",
    "-emit-executable",
    "-module-name",
    "IncodexPermissionHost",
    "-disable-autolinking-runtime-compatibility",
    "-target",
    `${architecture}-apple-macos${minimumMacOS}`,
    "-sdk",
    sdkPath,
    ...hostSourcePaths.filter(path => path.endsWith(".swift")),
    embedded,
    bridgeObject,
    "-Xlinker", "-export_dynamic",
    "-o",
    output,
  ]);
}

export function buildPermissionNative(): void {
  if (process.platform !== "darwin") {
    throw new Error("macOS permission native artifact must be built on macOS");
  }

  const sdkPath = run("xcrun", ["--sdk", "macosx", "--show-sdk-path"]).trim();
  const swiftc = run("xcrun", ["--sdk", "macosx", "--find", "swiftc"]).trim();
  if (!sdkPath || !swiftc) throw new Error("Unable to locate the macOS SDK or swiftc");

  const temporaryRoot = mkdtempSync(join(tmpdir(), "incodex-permission-native-"));
  const slices = architectures.map((architecture) => ({
    architecture,
    path: join(temporaryRoot, `incodex-permission-ui-${architecture}.dylib`),
  }));
  const hostSlices = architectures.map((architecture) => ({
    architecture,
    path: join(temporaryRoot, `incodex-permission-host-${architecture}`),
  }));
  const universalPath = join(temporaryRoot, dylibName);
  const universalHostPath = join(temporaryRoot, hostName);

  try {
    for (const slice of slices) buildSlice(swiftc, sdkPath, slice.architecture, slice.path);
    for (const slice of hostSlices) buildHostSlice(swiftc, sdkPath, slice.architecture, slice.path);
    run("lipo", ["-create", ...slices.map((slice) => slice.path), "-output", universalPath]);
    run("lipo", ["-create", ...hostSlices.map((slice) => slice.path), "-output", universalHostPath]);
    run("codesign", ["--force", "--sign", "-", "--timestamp=none", universalPath]);
    run("codesign", ["--force", "--sign", "-", "--timestamp=none", universalHostPath]);
    run("codesign", ["--verify", "--strict", "--", universalPath]);
    run("codesign", ["--verify", "--strict", "--", universalHostPath]);
    assertUniversalBinary(universalPath);
    assertUniversalBinary(universalHostPath);
    assertExports(universalPath);

    mkdirSync(distRoot, { recursive: true });
    copyFileSync(universalPath, dylibPath);
    copyFileSync(universalHostPath, hostPath);
    chmodSync(hostPath, 0o700);
    const manifest: NativeManifest = {
      schemaVersion: 1,
      platform: "macos",
      abiVersion: 1,
      minimumMacOS,
      architectures: [...architectures],
      sourceSha256: sha256(sourcePath),
      hostSourceSha256: sha256Files(hostSourcePaths),
      files: { [dylibName]: sha256(dylibPath), [hostName]: sha256(hostPath) },
    };
    writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    process.stdout.write(`wrote ${dylibPath} (${readFileSync(dylibPath).byteLength} bytes)\n`);
    process.stdout.write(`wrote ${hostPath} (${readFileSync(hostPath).byteLength} bytes)\n`);
    process.stdout.write(`wrote ${manifestPath}\n`);
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

if (import.meta.main) buildPermissionNative();
