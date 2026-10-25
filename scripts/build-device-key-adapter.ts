/**
 * [INPUT]: 依赖 xcrun、lipo、codesign 与固定 N-API bridge 源。
 * [OUTPUT]: 构建双架构 adapter 和独立源/二进制摘要清单。
 * [POS]: 显式原生构建工具；不改 Runtime catalog 或宿主包。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = join(import.meta.dir, "../native/macos");
const dist = join(root, "dist");
const adapterName = "incodex-device-key-adapter.node";
export const deviceKeyAdapterSources = [
  "device-key-adapter.m",
  "device-key-bridge.h",
  "host-identity.h",
  "vendor/node/node_api.h",
  "vendor/node/node_api_types.h",
  "vendor/node/js_native_api.h",
  "vendor/node/js_native_api_types.h",
];

const sha256 = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");

function run(command: string, args: string[]) {
  const result = spawnSync(command, args, { encoding: "utf8" });
  if (result.error || result.status !== 0) throw result.error ?? new Error(result.stderr);
}

export function buildDeviceKeyAdapter() {
  if (process.platform !== "darwin") throw new Error("Device-key adapter must build on macOS");
  const temporary = mkdtempSync(join(tmpdir(), "incodex-device-key-adapter-build-"));
  const output = join(dist, adapterName);
  try {
    const slices = ["arm64", "x86_64"].map(arch => join(temporary, `${arch}.node`));
    for (const [index, arch] of ["arm64", "x86_64"].entries()) {
      run("xcrun", [
        "clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-shared", "-undefined", "dynamic_lookup",
        "-framework", "Foundation", "-framework", "Security", "-target", `${arch}-apple-macos12.0`,
        join(root, "device-key-adapter.m"), "-o", slices[index],
      ]);
    }
    run("lipo", ["-create", ...slices, "-output", output]);
    run("codesign", ["--force", "--sign", "-", "--timestamp=none", output]);
    run("codesign", ["--verify", "--strict", output]);

    const manifest = {
      schemaVersion: 1,
      platform: "macos",
      abiVersion: 1,
      minimumMacOS: "12.0",
      architectures: ["arm64", "x86_64"],
      sourceSha256: sha256(Buffer.concat(deviceKeyAdapterSources.map(source => readFileSync(join(root, source))))),
      files: { [adapterName]: sha256(readFileSync(output)) },
    };
    writeFileSync(join(dist, "incodex-device-key-adapter-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}

if (import.meta.main) buildDeviceKeyAdapter();
