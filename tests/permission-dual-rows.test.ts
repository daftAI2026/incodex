/**
 * [INPUT]: 依赖权限 SwiftUI/native presenter 源码与隔离无窗夹具。
 * [OUTPUT]: 验证双权限行布局、快照、状态接口和窗口边界。
 * [POS]: 双权限授权卡的 RED/GREEN 回归；不访问宿主或系统授权。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test.skipIf(process.platform !== "darwin")("dual permission rows retain independent 518x80 cards and presenter state without windows", () => {
  const root = join(import.meta.dir, "..");
  const directory = mkdtempSync(join(tmpdir(), "incodex-permission-dual-rows-"));
  try {
    const executable = join(directory, "dual-rows-smoke");
    const sources = [
      "native/macos/permission-views.swift",
      "native/macos/permission-initial.swift",
      "native/macos/permission-host-presenter.swift",
      "tests/native/permission-dual-rows-smoke.swift",
    ];
    const architecture = process.arch === "arm64" ? "arm64" : process.arch === "x64" ? "x86_64" : "";
    expect(architecture).not.toBe("");
    const build = spawnSync("xcrun", [
      "swiftc", "-parse-as-library", "-target", `${architecture}-apple-macos12`,
      ...sources, "-o", executable,
    ], { cwd: root, encoding: "utf8", timeout: 90_000 });
    const buildOutput = `${build.stdout ?? ""}${build.stderr ?? ""}`;
    expect(build.status, buildOutput || String(build.error ?? "dual-row Swift build failed")).toBe(0);
    if (build.status !== 0) return;

    const run = spawnSync(executable, [], { cwd: root, encoding: "utf8", timeout: 20_000 });
    const runOutput = `${run.stdout ?? ""}${run.stderr ?? ""}`;
    expect(run.status, runOutput || String(run.error ?? "dual-row smoke failed")).toBe(0);
    expect(run.stdout).toContain("permission dual rows passed (independent cards, 98pt growth, snapshots, windowless presenter)");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 120_000);
