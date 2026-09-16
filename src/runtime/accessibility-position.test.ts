import { expect, test } from "bun:test";
import { positionPermissionGuide } from "./incodex-accessibility-window.cts";

test("positions beside Settings without covering its window", () => {
  expect(positionPermissionGuide(
    { x: 500, y: 100, width: 700, height: 700 },
    { width: 380, height: 430 }, { x: 0, y: 25, width: 1728, height: 1092 },
  )).toEqual({ x: 1216, y: 235, direction: "left" });
});

test("uses available left space and preserves negative display coordinates", () => {
  expect(positionPermissionGuide(
    { x: -1000, y: 100, width: 700, height: 700 },
    { width: 380, height: 430 }, { x: -1728, y: 25, width: 1728, height: 1092 },
  )).toEqual({ x: -1396, y: 235, direction: "right" });
});

test("on a narrow screen stays visible and hides an arrow that cannot point honestly", () => {
  const result = positionPermissionGuide(
    { x: 30, y: 25, width: 700, height: 600 },
    { width: 380, height: 430 }, { x: 0, y: 25, width: 800, height: 650 },
  );
  expect(result.direction).toBe("none");
  expect(result.x).toBeGreaterThanOrEqual(0);
  expect(result.x + 380).toBeLessThanOrEqual(800);
  expect(result.y).toBeGreaterThanOrEqual(25);
  expect(result.y + 430).toBeLessThanOrEqual(675);
});
