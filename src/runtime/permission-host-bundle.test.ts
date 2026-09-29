import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { RUNTIME_EXTERNAL_ARTIFACT_NAMES } from "../runtime-manifest.ts";
import { sharedPermissionCopy } from "../permission-shared-copy.ts";
import { ACCESSIBILITY_SETUP_COPY } from "./incognito-copy.ts";

test("native and compatibility hosts share one verified permission copy catalog", () => {
  expect(RUNTIME_EXTERNAL_ARTIFACT_NAMES).not.toContain("incodex-permission-host.cjs");
  expect(RUNTIME_EXTERNAL_ARTIFACT_NAMES).toContain("incodex-permission-copy.json");
  const bytes = readFileSync(new URL("../../dist/incodex-permission-copy.json", import.meta.url));
  const manifest = JSON.parse(readFileSync(new URL("../../dist/runtime-manifest.json", import.meta.url), "utf8"));
  expect(manifest.files["incodex-permission-copy.json"]).toBe(createHash("sha256").update(bytes).digest("hex"));
  expect(JSON.parse(bytes.toString())).toEqual(sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY));
});

test("the compatibility host references shared permission UI and copy assets", () => {
  const source = readFileSync(new URL("../../dist/incodex-main.cjs", import.meta.url), "utf8");
  expect(source).toContain('require("./incodex-permission-copy.json")');
  expect(source).toContain('require("./incodex-permission-ui.cjs")');
  expect(source).not.toContain("Native permission SwiftUI guide classes are unavailable");
  expect(source).not.toContain("Native permission SwiftUI flight class is unavailable");
});

test("new permission dependencies remain covered by loaders already installed in Codex", () => {
  const source = readFileSync(new URL("../../dist/incodex-main.cjs", import.meta.url), "utf8");
  const legacyVerifiedArtifacts = new Set([
    "incodex-main.cjs",
    "incodex-preload.cjs",
    "incodex-inject.js",
    "incodex-safe-home.cjs",
    "incodex-ipc-guard.cjs",
    "incodex-owner-core.cjs",
    "incodex-owner-recovery.cjs",
    "incodex-instance.cjs",
    "incodex-window-kind.cjs",
    "incodex-window-lifecycle.cjs",
    "incodex-runtime-load.cjs",
    "incodex-codex-mode.cjs",
    "incodex-dock-menu.cjs",
  ]);
  const unchecked = [...source.matchAll(/require\(["']\.\/(incodex-[^"']+)["']\)/g)]
    .map((match) => match[1])
    .filter((name) => name !== "incodex-windows-platform.cjs" && !legacyVerifiedArtifacts.has(name));

  expect(unchecked).toEqual([]);
});
