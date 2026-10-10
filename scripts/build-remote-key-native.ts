import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = join(import.meta.dir, "../native/macos");
export const remoteKeySources = [
  "remote-key-compat.m", "remote-key-policy.h", "vendor/fishhook/fishhook.c", "vendor/fishhook/fishhook.h",
  "vendor/node/node_api.h", "vendor/node/node_api_types.h", "vendor/node/js_native_api.h", "vendor/node/js_native_api_types.h",
];
const sha = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");
function run(command: string, args: string[]) {
  const result = spawnSync(command, args, { encoding: "utf8" });
  if (result.error || result.status !== 0) throw result.error ?? new Error(result.stderr);
}
export function buildRemoteKeyNative() {
  if (process.platform !== "darwin") throw new Error("Remote key compatibility must build on macOS");
  const temporary = mkdtempSync(join(tmpdir(), "incodex-remote-key-build-"));
  const name = "incodex-remote-key-compat.node";
  const output = join(root, "dist", name);
  try {
    const slices = ["arm64", "x86_64"].map(arch => join(temporary, `${arch}.node`));
    for (const [i, arch] of ["arm64", "x86_64"].entries()) {
      run("xcrun", ["clang", "-fobjc-arc", "-shared", "-undefined", "dynamic_lookup", "-framework", "Foundation",
        "-framework", "Security", "-target", `${arch}-apple-macos12.0`, join(root, "remote-key-compat.m"),
        join(root, "vendor/fishhook/fishhook.c"), "-o", slices[i]]);
    }
    run("lipo", ["-create", ...slices, "-output", output]);
    run("codesign", ["--force", "--sign", "-", "--timestamp=none", output]);
    run("codesign", ["--verify", "--strict", output]);
    const manifest = {
      schemaVersion: 1, platform: "macos", abiVersion: 1, minimumMacOS: "12.0", architectures: ["arm64", "x86_64"],
      sourceSha256: sha(Buffer.concat(remoteKeySources.map(file => readFileSync(join(root, file))))),
      files: { [name]: sha(readFileSync(output)) },
    };
    writeFileSync(join(root, "dist/incodex-remote-key-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  } finally { rmSync(temporary, { recursive: true, force: true }); }
}
if (import.meta.main) buildRemoteKeyNative();
