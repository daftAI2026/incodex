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

test("Helper app icon requests high interpolation before SwiftUI resizes it", () => {
  expect(appIconBranch("PermissionHelperAppRowRoot")).toMatch(
    /Image\(nsImage: image\)\s*\.interpolation\(\.high\)\s*\.resizable\(\)/,
  );
});
