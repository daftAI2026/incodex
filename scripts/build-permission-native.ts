import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
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
const distRoot = join(nativeRoot, "dist");
const dylibName = "incodex-permission-ui.dylib";
const dylibPath = join(distRoot, dylibName);
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

function assertUniversalBinary(path: string): void {
  const lipoInfo = run("lipo", ["-info", path]);
  for (const architecture of architectures) {
    run("lipo", [path, "-verify_arch", architecture]);
    if (!lipoInfo.includes(architecture)) {
      throw new Error(`Universal permission library is missing ${architecture}: ${lipoInfo}`);
    }
  }

  const loadCommands = run("otool", ["-l", path]);
  const minimumVersionMatches = loadCommands.match(new RegExp(`minos\\s+${minimumMacOS.replace(".", "\\.")}`, "g")) ?? [];
  if (minimumVersionMatches.length < architectures.length) {
    throw new Error(`Permission library must target macOS ${minimumMacOS} in every slice`);
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
  const universalPath = join(temporaryRoot, dylibName);

  try {
    for (const slice of slices) buildSlice(swiftc, sdkPath, slice.architecture, slice.path);
    run("lipo", ["-create", ...slices.map((slice) => slice.path), "-output", universalPath]);
    run("codesign", ["--force", "--sign", "-", "--timestamp=none", universalPath]);
    run("codesign", ["--verify", "--strict", "--", universalPath]);
    assertUniversalBinary(universalPath);
    assertExports(universalPath);

    mkdirSync(distRoot, { recursive: true });
    copyFileSync(universalPath, dylibPath);
    const manifest: NativeManifest = {
      schemaVersion: 1,
      platform: "macos",
      abiVersion: 1,
      minimumMacOS,
      architectures: [...architectures],
      sourceSha256: sha256(sourcePath),
      files: { [dylibName]: sha256(dylibPath) },
    };
    writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    process.stdout.write(`wrote ${dylibPath} (${readFileSync(dylibPath).byteLength} bytes)\n`);
    process.stdout.write(`wrote ${manifestPath}\n`);
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

if (import.meta.main) buildPermissionNative();
