import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ACCESSIBILITY_REGIONAL_COPY } from "../src/runtime/incognito-accessibility-copy-data.ts";

test.skipIf(process.platform !== "darwin")("localized permission card text wraps without clipping or reaching Allow", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-permission-card-text-layout-"));
  try {
    const executable = join(directory, "card-text-layout");
    const architecture = process.arch === "arm64" ? "arm64" : "x86_64";
    const build = spawnSync("xcrun", [
      "swiftc", "-parse-as-library", "-target", `${architecture}-apple-macos12`,
      "-module-name", "IncodexPermissionCardTextLayoutTest",
      "native/macos/permission-views.swift", "tests/native/permission-card-text-layout-smoke.swift",
      "-o", executable,
    ], { cwd: join(import.meta.dir, ".."), encoding: "utf8", timeout: 60_000 });
    expect(build.status, build.stderr || String(build.error ?? "permission-card text-layout compilation failed")).toBe(0);
    if (build.status !== 0) return;

    const copies = [ACCESSIBILITY_REGIONAL_COPY["bg-BG"], ACCESSIBILITY_REGIONAL_COPY["el-GR"]];
    const run = spawnSync(executable, [JSON.stringify(copies)], { encoding: "utf8", timeout: 20_000 });
    expect(run.status, `${run.stdout ?? ""}${run.stderr ?? ""}` || String(run.error ?? "permission-card text-layout smoke failed")).toBe(0);
    console.log(run.stdout.trim());
    expect(run.stdout).toContain("permission card text layout smoke passed (windowless; 2× SwiftUI foreground snapshots)");
    expect(run.stdout).toContain("bg-BG:");
    expect(run.stdout).toContain("el-GR:");
    expect(run.stdout).toContain("unbroken-word:");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 90_000);
