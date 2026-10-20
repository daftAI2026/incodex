/**
 * [INPUT]: 依赖原生权限视图源码与无窗 SwiftUI 回归。
 * [OUTPUT]: 验证权限行独立持有呈现状态与回调。
 * [POS]: 组件解绑的红绿测试，不打开宿主或请求系统权限。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test.skipIf(process.platform !== "darwin")("permission rows reuse one view with independent state and callbacks without windows", () => {
  const root = join(import.meta.dir, "..");
  const directory = mkdtempSync(join(tmpdir(), "incodex-permission-row-component-"));
  try {
    const executable = join(directory, "row-component");
    const sources = ["permission-views.swift", "permission-initial.swift"]
      .map(name => join(root, "native", "macos", name));
    const build = spawnSync("xcrun", [
      "swiftc", "-parse-as-library", "-target", `${process.arch === "arm64" ? "arm64" : "x86_64"}-apple-macos12`,
      ...sources, "tests/native/permission-row-component-smoke.swift", "-o", executable,
    ], { cwd: root, encoding: "utf8", timeout: 60_000 });
    expect(build.status, build.stderr || String(build.error ?? "row-component build failed")).toBe(0);
    if (build.status !== 0) return;
    const run = spawnSync(executable, [], { cwd: root, encoding: "utf8", timeout: 20_000 });
    expect(run.status, `${run.stdout ?? ""}${run.stderr ?? ""}`).toBe(0);
    expect(run.stdout).toContain("permission row component passed (independent state/actions; no windows)");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 90_000);
