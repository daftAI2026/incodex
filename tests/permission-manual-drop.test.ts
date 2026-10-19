/**
 * [INPUT]: 依赖原生权限 Presenter、拖放坐标夹具与隔离 Settings Locator。
 * [OUTPUT]: 验证 Official 手动收据只收回 helper，不授予 Screen 状态。
 * [POS]: 卸载后引导回归；仅编译无窗 Presenter，不访问宿主或 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test.skipIf(process.platform !== "darwin")(
  "manual Screen drop returns only a Settings-window receipt in the isolated native presenter",
  () => {
    const root = join(import.meta.dir, "..");
    const directory = mkdtempSync(join(tmpdir(), "incodex-permission-manual-drop-"));
    try {
      const executable = join(directory, "manual-drop-smoke");
      const architecture = process.arch === "arm64" ? "arm64" : process.arch === "x64" ? "x86_64" : "";
      expect(architecture).not.toBe("");
      const sources = [
        "native/macos/permission-views.swift",
        "native/macos/permission-initial.swift",
        "native/macos/permission-host-presenter-geometry.swift",
        "native/macos/permission-host-presenter-views.swift",
        "native/macos/permission-host-presenter.swift",
        "tests/native/permission-manual-drop-smoke.swift",
      ];
      const build = spawnSync("xcrun", [
        "swiftc", "-parse-as-library", "-target", `${architecture}-apple-macos12`,
        "-module-name", "IncodexPermissionManualDropSmoke",
        ...sources,
        "-o", executable,
      ], { cwd: root, encoding: "utf8", timeout: 90_000 });
      const buildOutput = `${build.stdout ?? ""}${build.stderr ?? ""}`;
      expect(build.status, buildOutput || String(build.error ?? "manual-drop Swift build failed")).toBe(0);
      if (build.status !== 0) return;

      const run = spawnSync(executable, [], { cwd: root, encoding: "utf8", timeout: 20_000 });
      const runOutput = `${run.stdout ?? ""}${run.stderr ?? ""}`;
      expect(run.status, runOutput || String(run.error ?? "manual-drop native smoke failed")).toBe(0);
      expect(run.stdout).toContain("permission manual drop passed (manual receipt only; no TCC)");
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
  120_000,
);
