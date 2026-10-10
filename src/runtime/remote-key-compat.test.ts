import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "bun:test";
import { installRemoteKeyCompatibility } from "./incodex-remote-key-compat.cts";

test("remote key compatibility never loads native code for private windows or Windows", () => {
  let calls = 0;
  const load = () => { calls++; throw new Error("must not load"); };
  expect(installRemoteKeyCompatibility({ platform: "win32", incognito: false, load })).toBe(false);
  expect(installRemoteKeyCompatibility({ platform: "darwin", incognito: true, load })).toBe(false);
  expect(calls).toBe(0);
});

test("native compatibility failure never prevents official startup", () => {
  expect(installRemoteKeyCompatibility({ platform: "darwin", incognito: false, directory: "/missing" })).toBe(false);
});

// This owns a disposable keychain; it never uses the user's login keychain.
test.skipIf(process.platform !== "darwin")("native policy preserves signing and blocks private-key export", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-remote-policy-"));
  try {
    const binary = join(directory, "policy-test");
    const source = join(import.meta.dir, "../../native/macos/remote-key-compat.test.m");
    const compiled = spawnSync("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-framework", "Foundation", "-framework", "Security", source, "-o", binary]);
    expect(compiled.status).toBe(0);
    const result = spawnSync(binary, [], { encoding: "utf8" });
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("real keychain contracts passed");
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test.skipIf(process.platform !== "darwin")("successful native retry clears the released original error", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-remote-error-"));
  try {
    const binary = join(directory, "error-test");
    const root = join(import.meta.dir, "../../native/macos");
    const compiled = spawnSync("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-undefined", "dynamic_lookup", "-framework", "Foundation", "-framework", "Security", join(root, "remote-key-error.test.m"), join(root, "vendor/fishhook/fishhook.c"), "-o", binary]);
    expect(compiled.status).toBe(0);
    const result = spawnSync(binary, [], { encoding: "utf8" });
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("retry error ownership passed");
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
