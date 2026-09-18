import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { RUNTIME_EXTERNAL_ARTIFACT_NAMES } from "../runtime-manifest.ts";

test("the one-shot permission host is a verified Runtime artifact", () => {
  expect(RUNTIME_EXTERNAL_ARTIFACT_NAMES).toContain("incodex-permission-host.cjs");
  const bytes = readFileSync(new URL("../../dist/incodex-permission-host.cjs", import.meta.url));
  const manifest = JSON.parse(readFileSync(new URL("../../dist/runtime-manifest.json", import.meta.url), "utf8"));
  expect(manifest.files["incodex-permission-host.cjs"]).toBe(createHash("sha256").update(bytes).digest("hex"));
  const source = bytes.toString();
  expect(source).not.toContain("__INCODEX_ACCESSIBILITY_WINDOW__");
  expect(source).not.toContain("__INCODEX_ACCESSIBILITY_COPY__");
  expect(source).not.toContain("__INCODEX_ACCESSIBILITY_LOCALE__");
  expect(source).toContain("createNativeAccessibilitySetupWindow");
  expect(source).toContain("runNativePermissionHandoff");
});

test("the one-shot host references the shared sibling permission UI asset", () => {
  const source = readFileSync(new URL("../../dist/incodex-permission-host.cjs", import.meta.url), "utf8");
  expect(source).toContain('require("./incodex-permission-ui.cjs")');
  expect(source).not.toContain("Native permission SwiftUI guide classes are unavailable");
  expect(source).not.toContain("Native permission SwiftUI flight class unavailable");
});
