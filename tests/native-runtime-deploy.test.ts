import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { macOSNativeRuntimeFiles } from "../src/native-runtime-artifacts.ts";

const sha = (bytes: Buffer | string) => createHash("sha256").update(bytes).digest("hex");

test("development Runtime includes native bytes in the same manifest, never on Windows", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-native-deploy-"));
  try {
    const source = "SwiftUI source";
    const bytes = Buffer.from([0xca, 0xfe, 0xba, 0xbe, 0xff]);
    mkdirSync(join(directory, "dist"));
    writeFileSync(join(directory, "permission-views.swift"), source);
    writeFileSync(join(directory, "dist/incodex-permission-ui.dylib"), bytes);
    const manifest = JSON.stringify({ schemaVersion: 1, platform: "macos", abiVersion: 1,
      minimumMacOS: "12.0", architectures: ["arm64", "x86_64"], sourceSha256: sha(source),
      files: { "incodex-permission-ui.dylib": sha(bytes) } });
    writeFileSync(join(directory, "dist/runtime-native-manifest.json"), manifest);
    const shared = { runtimeVersion: "1.0.1", sourceCommit: "", files: { "incodex-main.cjs": "a".repeat(64) } };
    const files = macOSNativeRuntimeFiles(directory, shared, "darwin");
    expect(files["incodex-permission-ui.dylib"]).toEqual(bytes);
    expect(files["runtime-native-manifest.json"].toString()).toBe(manifest);
    expect(JSON.parse(files["runtime-manifest.json"].toString()).files).toEqual({
      ...shared.files, "incodex-permission-ui.dylib": sha(bytes), "runtime-native-manifest.json": sha(manifest),
    });
    expect(macOSNativeRuntimeFiles("/missing-native-assets", shared, "win32")).toEqual({});
    writeFileSync(join(directory, "dist/incodex-permission-ui.dylib"), "broken");
    expect(() => macOSNativeRuntimeFiles(directory, shared, "darwin")).toThrow();
  } finally { rmSync(directory, { recursive: true, force: true }); }
});
