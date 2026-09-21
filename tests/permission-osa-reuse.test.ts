import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
test("shipped permission host reuses the original JS UI instead of Swift window orchestration", () => {
  const build = readFileSync(join(root, "scripts/build-permission-native.ts"), "utf8");
  expect(build).toContain("permission-host-osa.swift");
  expect(build).toContain("permission-host-runtime.js");
  expect(build).not.toContain('join(nativeRoot, "permission-host-flight.swift")');
  expect(build).not.toContain('join(nativeRoot, "permission-host-presenter.swift")');
});
test("OSA runtime uses the original loader and does not inject an unchecked native library", () => {
  const runtime = readFileSync(join(root, "native/macos/permission-host-runtime.js"), "utf8");
  expect(runtime).toContain("__INCODEX_ORIGINAL_UI_JSON__");
  expect(runtime).toContain("createNativeAccessibilitySetupWindow");
  expect(runtime).not.toContain("nativeLibrary:");
  expect(runtime).not.toContain("startForScreenHandler");
  expect(runtime).not.toContain("x-apple.systempreferences");
});
