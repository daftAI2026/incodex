import { createHash } from "node:crypto";
import { chmodSync, existsSync, lstatSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { RuntimeManifest } from "./runtime-manifest.ts";

const name = "incodex-permission-ui.dylib";
const hostName = "incodex-permission-host";
const manifestName = "runtime-native-manifest.json";
const sha = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");
const validHash = (value: unknown): value is string => typeof value === "string" && /^[a-f0-9]{64}$/.test(value);

// Development copies use the same sibling files and merged manifest as the
// Rust publisher. This does not add native artifacts to the Windows catalog.
export function macOSNativeRuntimeFiles(
  nativeRoot: string,
  shared: RuntimeManifest,
  platform: string = process.platform,
): Record<string, Buffer> {
  if (platform !== "darwin") return {};
  const read = (file: string) => {
    const stat = lstatSync(file);
    if (!stat.isFile() || stat.isSymbolicLink()) throw new Error("Invalid native Runtime input");
    return readFileSync(file);
  };
  const manifestBytes = read(join(nativeRoot, "dist", manifestName));
  const manifest = JSON.parse(manifestBytes.toString("utf8"));
  const bytes = read(join(nativeRoot, "dist", name));
  const declaredFiles = manifest?.files;
  const hostDeclared = declaredFiles && Object.hasOwn(declaredFiles, hostName);
  const hostBytes = hostDeclared ? read(join(nativeRoot, "dist", hostName)) : undefined;
  const hostMode = hostDeclared ? lstatSync(join(nativeRoot, "dist", hostName)).mode : 0;
  const hostSourcePaths = [
    join(nativeRoot, "permission-host.swift"),
    join(nativeRoot, "permission-host-presenter.swift"),
    join(nativeRoot, "permission-host-settings.swift"),
    join(nativeRoot, "permission-host-flight.swift"),
  ];
  const hostSourceHash = hostDeclared
    ? sha(Buffer.concat(hostSourcePaths.map((file) => read(file))))
    : undefined;
  if (manifest.schemaVersion !== 1 || manifest.platform !== "macos" || manifest.abiVersion !== 1 ||
      manifest.minimumMacOS !== "12.0" || JSON.stringify(manifest.architectures) !== '["arm64","x86_64"]' ||
      !declaredFiles || Object.keys(declaredFiles).length !== (hostDeclared ? 2 : 1) ||
      !validHash(declaredFiles[name]) || declaredFiles[name] !== sha(bytes) ||
      (hostDeclared && (!hostBytes || !validHash(declaredFiles[hostName]) || declaredFiles[hostName] !== sha(hostBytes) ||
        (hostMode & 0o100) === 0 || (hostMode & 0o022) !== 0 ||
        !validHash(manifest.hostSourceSha256) || manifest.hostSourceSha256 !== hostSourceHash)) ||
      (!hostDeclared && manifest.hostSourceSha256 !== undefined) ||
      manifest.sourceSha256 !== sha(read(join(nativeRoot, "permission-views.swift")))) {
    throw new Error("Native Runtime artifact/source manifest mismatch; run build:permission-native");
  }
  const compatibility: Record<string, Buffer> = {};
  const compatibilityManifest = "incodex-remote-key-manifest.json";
  if (existsSync(join(nativeRoot, "remote-key-compat.m"))) {
    const declared = read(join(nativeRoot, "dist", compatibilityManifest));
    const metadata = JSON.parse(declared.toString("utf8"));
    const addonName = "incodex-remote-key-compat.node";
    const addon = read(join(nativeRoot, "dist", addonName));
    const sources = ["remote-key-compat.m", "remote-key-policy.h", "vendor/fishhook/fishhook.c", "vendor/fishhook/fishhook.h",
      "vendor/node/node_api.h", "vendor/node/node_api_types.h", "vendor/node/js_native_api.h", "vendor/node/js_native_api_types.h"];
    if (metadata.schemaVersion !== 1 || metadata.platform !== "macos" || metadata.abiVersion !== 1 ||
        metadata.minimumMacOS !== "12.0" || JSON.stringify(metadata.architectures) !== '["arm64","x86_64"]' ||
        Object.keys(metadata.files || {}).join() !== addonName || metadata.files[addonName] !== sha(addon) ||
        metadata.sourceSha256 !== sha(Buffer.concat(sources.map(file => read(join(nativeRoot, file)))))) {
      throw new Error("Remote key native artifact/source manifest mismatch; run build:remote-key-native");
    }
    compatibility[compatibilityManifest] = declared;
    compatibility[addonName] = addon;
  }
  const files = {
    ...shared.files,
    ...Object.fromEntries(Object.entries(compatibility).map(([file, content]) => [file, sha(content)])),
    [name]: sha(bytes),
    ...(hostBytes ? { [hostName]: sha(hostBytes) } : {}),
    [manifestName]: sha(manifestBytes),
  };
  return {
    ...compatibility,
    [name]: bytes,
    ...(hostBytes ? { [hostName]: hostBytes } : {}),
    [manifestName]: manifestBytes,
    "runtime-manifest.json": Buffer.from(`${JSON.stringify({ ...shared, files }, null, 2)}\n`),
  };
}

// Source checkouts retain Git's executable bit (normally 0755). The private
// development deployment, like the Rust publisher, must instead be 0700.
export function writeNativeRuntimeFiles(directory: string, files: Record<string, Buffer>): void {
  for (const [file, bytes] of Object.entries(files)) {
    const target = join(directory, file);
    const mode = file === hostName ? 0o700 : 0o600;
    writeFileSync(target, bytes, { mode });
    chmodSync(target, mode); // writeFile's mode does not repair existing files.
  }
}
