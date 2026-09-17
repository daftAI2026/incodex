import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const layoutSmokeSource = join(import.meta.dir, "native", "permission-views-layout-smoke.m");

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

test.skipIf(process.platform !== "darwin")(
  "compiles the P11 real AX title-layout smoke (runtime is opt-in)",
  () => {
    const directory = mkdtempSync(join(tmpdir(), "incodex-layout-smoke-"));
    try {
      const architecture = process.arch === "arm64" ? "arm64" : process.arch === "x64" ? "x86_64" : "";
      expect(architecture).not.toBe("");
      if (!architecture) return;

      const executable = join(directory, "permission-views-layout-smoke");
      const build = spawnSync("/usr/bin/clang", [
        "-arch", architecture,
        "-fobjc-arc",
        "-framework", "Cocoa",
        "-framework", "ApplicationServices",
        layoutSmokeSource,
        "-o", executable,
      ], { cwd: join(import.meta.dir, ".."), encoding: "utf8", timeout: 60_000 });
      const buildOutput = `${build.stdout ?? ""}${build.stderr ?? ""}`;
      expect(build.status, buildOutput || String(build.error ?? "P11 Objective-C compilation failed")).toBe(0);
      if (build.status !== 0 || process.env.INCODEX_RUN_NATIVE_LAYOUT_SMOKE !== "1") return;

      const dylibPath = join(import.meta.dir, "..", "native", "macos", "dist", "incodex-permission-ui.dylib");
      const run = spawnSync(executable, [dylibPath], {
        cwd: join(import.meta.dir, ".."),
        encoding: "utf8",
        timeout: 20_000,
      });
      const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
      expect(run.status, output || String(run.error ?? "P11 real AX layout smoke failed")).toBe(0);
      expect(output).toContain("P11_CHECK");
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
  90_000,
);
