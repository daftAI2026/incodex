import { expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { RUNTIME_EXTERNAL_ARTIFACT_NAMES } from "../src/runtime-manifest.ts";

const root = join(import.meta.dir, "..");
const build = readFileSync(join(root, "scripts/build-permission-native.ts"), "utf8");
const host = readFileSync(join(root, "native/macos/permission-host.swift"), "utf8");
const developmentCatalog = readFileSync(join(root, "src/native-runtime-artifacts.ts"), "utf8");
const rustCatalog = readFileSync(join(root, "crates/incodex-runtime-bundle/src/native.rs"), "utf8");

test("the formal CLI host is built from the aligned SwiftUI presenter and flight", () => {
  for (const source of [
    "permission-host.swift",
    "permission-host-presenter.swift",
    "permission-host-settings.swift",
    "permission-host-flight.swift",
  ]) {
    expect(build).toContain(`join(nativeRoot, "${source}")`);
  }
  expect(build).not.toContain("permission-host-osa.swift");
  expect(build).not.toContain("PermissionHostOSASource");
  expect(build).not.toContain("permission-host-runtime.js");
  expect(build).not.toContain("permission-host-bridge.m");
  expect(build).not.toContain("incodex-permission-ui.cjs");
  expect(host).not.toContain("OSAScript");
  expect(host).toContain("PermissionHostPresenter(");
  expect(build).toContain("sourcePath");
  expect(build).toContain("buildSlice(swiftc, sdkPath, slice.architecture, slice.path)");
});

test("both native source-hash catalogs bind the Swift host implementation", () => {
  for (const source of [
    "permission-host.swift",
    "permission-host-presenter.swift",
    "permission-host-settings.swift",
    "permission-host-flight.swift",
  ]) {
    expect(developmentCatalog).toContain(source);
    expect(rustCatalog).toContain(source);
  }
  for (const obsoleteSource of [
    "permission-host-osa.swift",
    "permission-host-runtime.js",
    "permission-host-objc.js",
    "permission-host-bridge.m",
    "incodex-permission-ui.cjs",
    "incodex-dock-menu.cjs",
  ]) {
    expect(developmentCatalog).not.toContain(obsoleteSource);
    expect(rustCatalog).not.toContain(obsoleteSource);
  }
});

test("the Electron Runtime keeps its shared TypeScript UI and compatibility dylib", () => {
  const runtimeManifest = JSON.parse(readFileSync(join(root, "dist/runtime-manifest.json"), "utf8"));
  expect(RUNTIME_EXTERNAL_ARTIFACT_NAMES).toContain("incodex-permission-ui.cjs");
  expect(runtimeManifest.files["incodex-permission-ui.cjs"]).toMatch(/^[a-f0-9]{64}$/);
  expect(existsSync(join(root, "native/macos/dist/incodex-permission-ui.dylib"))).toBe(true);
});
