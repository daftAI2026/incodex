import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function permissionCardSource(): string {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const start = source.indexOf("private struct PermissionCardRoot");
  const end = source.indexOf("private struct PermissionPlaceholderLabel", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  return source.slice(start, end);
}

test("PermissionCardRoot keeps title and description line limits unconstrained by the original source contract", () => {
  const card = permissionCardSource();
  // The shipped binary's PermissionRow title/description chains have no lineLimit(1)/(2).
  // This is a source-structure contract only; it does not claim long-text or pixel parity.
  expect(card).not.toContain(".lineLimit(1)");
  expect(card).not.toContain(".lineLimit(2)");
});

test("PermissionCardRoot states the original continuous Capsule shape on Allow", () => {
  const card = permissionCardSource();
  // The binary chain is Capsule(style: .continuous), not the implicit Capsule default.
  // This asserts source structure only, not runtime pixels.
  expect(card).toMatch(/\.clipShape\(\s*Capsule\(style:\s*\.continuous\)\s*\)/);
});

test("macOS permission flight uses a public SwiftUI host with a stable bridge ABI", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-swiftui-test-"));
  try {
    const executable = join(directory, "permission-ui-test");
    const build = spawnSync("xcrun", [
      "swiftc", "-parse-as-library", "-module-name", "IncodexPermissionUITest",
      "native/macos/permission-views.swift", "tests/native/permission-views-smoke.swift",
      "-o", executable,
    ], { cwd: join(import.meta.dir, ".."), encoding: "utf8", timeout: 60_000 });
    expect(build.status, build.stderr || String(build.error ?? "Swift compilation failed")).toBe(0);
    const run = spawnSync(executable, [], { encoding: "utf8", timeout: 20_000 });
    expect(run.status, run.stderr || String(run.error ?? "Swift host smoke failed")).toBe(0);
    expect(run.stdout).toContain("permission SwiftUI host smoke passed");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 90_000);
