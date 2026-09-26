import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { RUNTIME_EXTERNAL_ARTIFACT_NAMES } from "../src/runtime-manifest.ts";

const root = join(import.meta.dir, "..");
const sha = (bytes: Buffer | string) => createHash("sha256").update(bytes).digest("hex");

test("the shared TypeScript permission UI remains a published Electron Runtime artifact", () => {
  const uiPath = join(root, "dist/incodex-permission-ui.cjs");
  const ui = readFileSync(uiPath);
  const manifest = JSON.parse(readFileSync(join(root, "dist/runtime-manifest.json"), "utf8"));
  expect(RUNTIME_EXTERNAL_ARTIFACT_NAMES).toContain("incodex-permission-ui.cjs");
  expect(manifest.files["incodex-permission-ui.cjs"]).toBe(sha(ui));
  expect(ui.toString("utf8")).toContain("createNativeAccessibilitySetupWindow");
});

test("the compatibility native library remains a separately published artifact", () => {
  const manifest = JSON.parse(readFileSync(join(root, "native/macos/dist/runtime-native-manifest.json"), "utf8"));
  const dylib = readFileSync(join(root, "native/macos/dist/incodex-permission-ui.dylib"));
  expect(manifest.files["incodex-permission-ui.dylib"]).toBe(sha(dylib));
});
