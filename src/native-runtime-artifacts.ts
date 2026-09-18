import { createHash } from "node:crypto";
import { chmodSync, lstatSync, readFileSync, writeFileSync } from "node:fs";
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
    "permission-host-settings.swift",
    "permission-host-flight.swift",
    "permission-host-presenter.swift",
    "permission-host.swift",
  ].map((file) => join(nativeRoot, file));
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
  const files = {
    ...shared.files,
    [name]: sha(bytes),
    ...(hostBytes ? { [hostName]: sha(hostBytes) } : {}),
    [manifestName]: sha(manifestBytes),
  };
  return {
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
