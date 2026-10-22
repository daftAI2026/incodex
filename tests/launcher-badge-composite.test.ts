/**
 * [INPUT]: 依赖 Swift 合成工具的无窗像素自测。
 * [OUTPUT]: 固定 alpha254 漏绘、轮廓外保真与原生抗锯齿边界。
 * [POS]: macOS 开发资产回归，不启动宿主或请求权限。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

test.skipIf(process.platform !== "darwin")("badge replacement uses its own mask, not host opacity", () => {
  const result = spawnSync("swift", [join(import.meta.dir, "../scripts/composite-launcher-badge.swift"), "--self-test"], {
    encoding: "utf8",
    timeout: 60_000,
  });
  expect(result.status, result.stderr).toBe(0);
  expect(result.stdout).toContain("PASS alpha254, alpha255, outside-mask, native-AA, zero-mask, mask-orientation, rounded-corner");
}, 65_000);


test("INCODEX fits its capsule without stretching the seven glyphs", () => {
  const layout = JSON.parse(readFileSync(join(import.meta.dir, "../assets/launcher-badge-layout.json"), "utf8"));
  const svg = readFileSync(join(import.meta.dir, "../assets/launcher-wordmark.svg"), "utf8");
  expect(layout.wordmark.text).toBe("INCODEX");
  expect(svg).toContain("<title>INCODEX</title>");
  expect(svg.match(/<path /g)?.length).toBe(7);
  expect(layout.wordmark.advanceWidth).toBeLessThanOrEqual(layout.width - 96);
  expect(layout.wordmark.capHeight).toBeLessThan(layout.height);
  expect((layout.width - layout.wordmark.advanceWidth) / 2).toBeCloseTo(layout.wordmark.sideInset, 4);
});
