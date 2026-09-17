import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const permissionViewsSource = readFileSync(
  join(import.meta.dir, "..", "native/macos/permission-views.swift"),
  "utf8",
);

function rootSource(name: string): string {
  const start = permissionViewsSource.indexOf(`private struct ${name}`);
  expect(start, `Missing SwiftUI root ${name}`).toBeGreaterThanOrEqual(0);
  if (start < 0) return "";

  const nextRoot = permissionViewsSource.indexOf("\nprivate struct ", start + name.length);
  return permissionViewsSource.slice(start, nextRoot < 0 ? permissionViewsSource.length : nextRoot);
}

function appIconBranch(name: string): string {
  const root = rootSource(name);
  const match = root.match(/if let image = state\.appIcon \{([\s\S]*?)\n\s*\} else \{/);
  expect(match, `${name} must keep an explicit appIcon branch`).not.toBeNull();
  return match?.[1] ?? "";
}

// Structure guards after an independent offscreen reproduction of aliasing.
// These tests do not measure pixels or claim parity with the original icon.

test("Initial app icon requests high interpolation before SwiftUI resizes it", () => {
  expect(appIconBranch("PermissionInitialRoot")).toMatch(
    /Image\(nsImage: image\)\s*\.interpolation\(\.high\)\s*\.resizable\(\)/,
  );
});

test("Helper row uses the original intrinsic NSImage and primary text in a 4pt HStack", () => {
  // ApplicationRowView conformance 0x10108AF08: resilient body witness at
  // +0x40 resolves to 0x100EBCE98; builder 0x100EBCD4C is Image + Text.primary.
  const branch = appIconBranch("PermissionHelperAppRowRoot");
  expect(branch).toContain("Image(nsImage: image)");
  expect(branch).not.toContain(".resizable()");
  expect(branch).not.toContain(".aspectRatio(");
  const root = rootSource("PermissionHelperAppRowRoot");
  expect(root).toContain("HStack(alignment: .center, spacing: 4)");
  expect(root).toContain(".foregroundStyle(.primary)");
  expect(root).not.toContain(".font(.system(size: 13))");
});
