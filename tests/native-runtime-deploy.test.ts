import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { macOSNativeRuntimeFiles, writeNativeRuntimeFiles } from "../src/native-runtime-artifacts.ts";

const sha = (bytes: Buffer | string) => createHash("sha256").update(bytes).digest("hex");
const repositoryRoot = join(import.meta.dir, "..");

test("the committed native manifest is accepted by the development publisher", () => {
  const shared = JSON.parse(readFileSync(join(repositoryRoot, "dist/runtime-manifest.json"), "utf8"));
  expect(() => macOSNativeRuntimeFiles(join(repositoryRoot, "native", "macos"), shared, "darwin")).not.toThrow();
});

test("development native publication repairs modes on existing files", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-native-mode-"));
  try {
    const host = join(directory, "incodex-permission-host");
    const manifest = join(directory, "runtime-native-manifest.json");
    writeFileSync(host, "old", { mode: 0o600 });
    writeFileSync(manifest, "old", { mode: 0o700 });
    writeNativeRuntimeFiles(directory, { "incodex-permission-host": Buffer.from("host"), "runtime-native-manifest.json": Buffer.from("manifest") });
    expect(statSync(host).mode & 0o777).toBe(0o700);
    expect(statSync(manifest).mode & 0o777).toBe(0o600);
    expect(readFileSync(host, "utf8")).toBe("host");
    expect(readFileSync(manifest, "utf8")).toBe("manifest");
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

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

test("development Runtime carries the Swift executable host with its source hash", () => {
  const projectRoot = mkdtempSync(join(tmpdir(), "incodex-native-host-deploy-"));
  const directory = join(projectRoot, "native", "macos");
  try {
    const source = "SwiftUI source";
    const nativeHostSources = [
      "permission-host.swift",
      "permission-host-presenter.swift",
      "permission-host-settings.swift",
      "permission-host-flight.swift",
    ];
    const bytes = Buffer.from([0xca, 0xfe, 0xba, 0xbe, 0xff]);
    const host = Buffer.from([0xca, 0xfe, 0xba, 0xbe, 0x01]);
    mkdirSync(join(directory, "dist"), { recursive: true });
    mkdirSync(join(projectRoot, "dist"), { recursive: true });
    writeFileSync(join(directory, "permission-views.swift"), source);
    for (const file of nativeHostSources) writeFileSync(join(directory, file), file);
    writeFileSync(join(directory, "dist/incodex-permission-ui.dylib"), bytes);
    // Git preserves the executable bit, not private deployment permissions.
    writeFileSync(join(directory, "dist/incodex-permission-host"), host, { mode: 0o755 });
    const hostSourceHash = sha(Buffer.concat([
      ...nativeHostSources.map((file) => readFileSync(join(directory, file))),
    ]));
    const manifest = JSON.stringify({ schemaVersion: 1, platform: "macos", abiVersion: 1,
      minimumMacOS: "12.0", architectures: ["arm64", "x86_64"], sourceSha256: sha(source),
      hostSourceSha256: hostSourceHash,
      files: {
        "incodex-permission-ui.dylib": sha(bytes),
        "incodex-permission-host": sha(host),
      } });
    writeFileSync(join(directory, "dist/runtime-native-manifest.json"), manifest);
    const shared = { runtimeVersion: "1.0.1", sourceCommit: "", files: { "incodex-main.cjs": "a".repeat(64) } };
    const files = macOSNativeRuntimeFiles(directory, shared, "darwin");
    expect(files["incodex-permission-host"]).toEqual(host);
    expect(JSON.parse(files["runtime-manifest.json"].toString()).files).toMatchObject({
      "incodex-permission-host": sha(host),
      "runtime-native-manifest.json": sha(manifest),
    });
  } finally { rmSync(projectRoot, { recursive: true, force: true }); }
});
