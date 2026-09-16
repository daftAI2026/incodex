import { expect, test } from "bun:test";
import { positionPermissionGuide } from "./incodex-accessibility-window.cts";

// Cavalry-i18n e76175f: helper follows Settings center at visible-screen bottom,
// with 20pt inset; the arrow points up toward the system permission window.
test("reuses Cavalry helper placement below Settings on its display", () => {
  expect(positionPermissionGuide(
    { x: 500, y: 100, width: 700, height: 700 },
    { width: 532, height: 112 }, { x: 0, y: 25, width: 1728, height: 1092 },
  )).toEqual({ x: 584, y: 985 });
});

test("retains negative display coordinates and clamps to its work area", () => {
  expect(positionPermissionGuide(
    { x: -1000, y: 100, width: 700, height: 700 },
    { width: 532, height: 112 }, { x: -1728, y: 25, width: 1728, height: 1092 },
  )).toEqual({ x: -916, y: 985 });
});

test("keeps the native helper inside a narrow display", () => {
  expect(positionPermissionGuide(
    { x: 30, y: 25, width: 700, height: 600 },
    { width: 532, height: 112 }, { x: 0, y: 25, width: 800, height: 650 },
  )).toEqual({ x: 114, y: 543 });
});
