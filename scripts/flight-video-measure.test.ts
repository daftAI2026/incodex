/**
 * [INPUT]: 依赖 Swift 离线测量工具的 self-test。
 * [OUTPUT]: 验证锚点几何、VFR 与输出边界。
 * [POS]: 离线分析合同，不触发真实权限页。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { describe, expect, test } from "bun:test";

const analyzer = resolve(import.meta.dir, "flight-video-measure.swift");

describe("offline flight video analyzer", () => {
  test("self-test checks anchor geometry, confidence bounds, VFR gaps, and anchor-only output", () => {
    const result = spawnSync("swift", [analyzer, "--self-test"], {
      encoding: "utf8",
      timeout: 120_000,
    });

    expect(result.error?.message).toBeUndefined();
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("self-test passed");
    expect(result.stdout).toContain("anchorCentroidXPx");
    expect(result.stdout).toContain("anchorWidthPx");
    expect(result.stdout).not.toContain("cardCentroid");
  }, 120_000);
});
