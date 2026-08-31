import { describe, expect, test } from "bun:test";
import {
  clampCaptureRect,
  normalizeCaptureRect,
  viewportRectToSource,
} from "./geometry.ts";

describe("capture window geometry", () => {
  test("normalizes reverse drags", () => {
    expect(normalizeCaptureRect({ x: 80, y: 60, width: -30, height: -20 })).toEqual({
      x: 50,
      y: 40,
      width: 30,
      height: 20,
    });
  });

  test("clamps a rectangle to source bounds", () => {
    expect(
      clampCaptureRect(
        { x: -10, y: 90, width: 40, height: 30 },
        { width: 100, height: 100 },
      ),
    ).toEqual({ x: 0, y: 90, width: 30, height: 10 });
  });

  test("maps a preview gesture back to source pixels", () => {
    expect(
      viewportRectToSource(
        { x: 210, y: 140, width: 120, height: 60 },
        { x: 10, y: 20, scale: 0.5 },
        { width: 1200, height: 800 },
      ),
    ).toEqual({ x: 400, y: 240, width: 240, height: 120 });
  });
});
