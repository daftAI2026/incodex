/**
 * [INPUT]: 检查器滚动边界算法与真实视图/CSS结构。
 * [OUTPUT]: 验证首尾渐隐、不溢出清晰、固定标题和隐藏滚动条的契约。
 * [POS]: capture-window 滚动表现回归，不把遮罩当主题背景色。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { inspectorScrollEdges } from "./inspector-scroll.ts";
import { captureWindowTemplate } from "./view.ts";
import { captureWindowCopy } from "./copy.ts";
import { createCaptureWindowState } from "./model.ts";

test("fades only edges which hide overflowing content", () => {
  expect(inspectorScrollEdges(0, 100, 80)).toEqual({ overflowing: false, atStart: true, atEnd: true });
  expect(inspectorScrollEdges(0, 100, 300)).toEqual({ overflowing: true, atStart: true, atEnd: false });
  expect(inspectorScrollEdges(100, 100, 300)).toEqual({ overflowing: true, atStart: false, atEnd: false });
  expect(inspectorScrollEdges(200, 100, 300)).toEqual({ overflowing: true, atStart: false, atEnd: true });
  expect(inspectorScrollEdges(199.5, 100, 300).atEnd).toBe(true);
  expect(inspectorScrollEdges(-8, 100, 300).atStart).toBe(true);
});

test("keeps the background title outside the keyboard-scrollable viewport", () => {
  const markup = captureWindowTemplate(createCaptureWindowState({ width: 800, height: 600, scaleFactor: 1 }), captureWindowCopy("en-US"));
  expect(markup).toMatch(/<h2[^>]*id="incodex-capture-background-title"[^>]*>Background<\/h2>\s*<div class="incodex-capture-inspector-scroll"/);
  expect(markup).toContain('tabindex="0"');
  const css = readFileSync(new URL("./capture-window.css", import.meta.url), "utf8");
  expect(css).toMatch(/\.incodex-capture-inspector-scroll\s*\{[^}]*overflow-y: auto/);
  expect(css).toMatch(/\.incodex-capture-inspector-scroll\s*\{[^}]*scrollbar-width: none/);
  expect(css).toContain(".incodex-capture-inspector-scroll::-webkit-scrollbar");
  expect(css).toContain('data-at-start="true"');
  expect(css).toContain('data-at-end="true"');
  expect(css).toContain("mask-image: linear-gradient");
});
