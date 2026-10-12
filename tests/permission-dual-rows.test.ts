/**
 * [INPUT]: 依赖权限 SwiftUI/native presenter 源码与隔离夹具。
 * [OUTPUT]: 验证双权限行、26/27 呈现、状态接口和窗口边界。
 * [POS]: 双权限授权卡的 RED/GREEN 回归；不访问宿主或系统授权。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test.skipIf(process.platform !== "darwin")("dual permission rows retain independent 518x80 cards and presenter state without showing windows", () => {
  const root = join(import.meta.dir, "..");
  const directory = mkdtempSync(join(tmpdir(), "incodex-permission-dual-rows-"));
  try {
    const executable = join(directory, "dual-rows-smoke");
    const presenterFixture = join(directory, "permission-host-presenter.swift");
    const presenterSource = readFileSync(join(root, "native/macos/permission-host-presenter.swift"), "utf8");
    // Pin only the animation preference branch so the reverse-flight regression
    // never reads or changes the test machine's Accessibility settings.
    writeFileSync(
      presenterFixture,
      presenterSource.replaceAll("NSWorkspace.shared.accessibilityDisplayShouldReduceMotion", "false"),
    );
    const sources = [
      "native/macos/permission-views.swift",
      "native/macos/permission-initial.swift",
      "native/macos/permission-host-presenter-geometry.swift",
      "native/macos/permission-host-presenter-views.swift",
      presenterFixture,
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
    expect(run.stdout).toContain("permission dual rows passed (independent cards, 98pt growth, snapshots, unshown flight fixtures)");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 120_000);

test.skipIf(process.platform !== "darwin")("platform presentation preserves macOS 26 and uses macOS 27 privacy names without changing permission actions", () => {
  const root = join(import.meta.dir, "..");
  const directory = mkdtempSync(join(tmpdir(), "incodex-permission-platform-"));
  try {
    const executable = join(directory, "platform-smoke");
    const architecture = process.arch === "arm64" ? "arm64" : "x86_64";
    const build = spawnSync("xcrun", [
      "swiftc", "-parse-as-library", "-target", `${architecture}-apple-macos12`,
      "native/macos/permission-views.swift", "native/macos/permission-initial.swift",
      "tests/native/permission-platform-smoke.swift", "-o", executable,
    ], { cwd: root, encoding: "utf8", timeout: 90_000 });
    expect(build.status, build.stderr || String(build.error ?? "platform smoke build failed")).toBe(0);
    if (build.status !== 0) return;
    const run = spawnSync(executable, [join(root, "dist/incodex-permission-copy.json")], {
      cwd: root, encoding: "utf8", timeout: 20_000,
    });
    expect(run.status, `${run.stdout ?? ""}${run.stderr ?? ""}` || String(run.error ?? "platform smoke failed")).toBe(0);
    expect(run.stdout).toContain("permission platform passed (26 unchanged; 27 names, templates, fallback, icons; no TCC)");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 120_000);
