import { createHash } from "node:crypto";
import { lstatSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { RuntimeManifest } from "./runtime-manifest.ts";

const name = "incodex-permission-ui.dylib";
const manifestName = "runtime-native-manifest.json";
const sha = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");

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
  if (manifest.schemaVersion !== 1 || manifest.platform !== "macos" || manifest.abiVersion !== 1 ||
      manifest.minimumMacOS !== "12.0" || JSON.stringify(manifest.architectures) !== '["arm64","x86_64"]' ||
      !manifest.files || Object.keys(manifest.files).length !== 1 || manifest.files[name] !== sha(bytes) ||
      manifest.sourceSha256 !== sha(read(join(nativeRoot, "permission-views.swift")))) {
    throw new Error("Native Runtime artifact/source manifest mismatch; run build:permission-native");
  }
  const files = { ...shared.files, [name]: sha(bytes), [manifestName]: sha(manifestBytes) };
  return {
    [name]: bytes,
    [manifestName]: manifestBytes,
    "runtime-manifest.json": Buffer.from(`${JSON.stringify({ ...shared, files }, null, 2)}\n`),
  };
}
