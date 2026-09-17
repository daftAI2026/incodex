import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

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
