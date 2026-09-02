import { describe, expect, test } from "bun:test";
import {
  anchoredPanForZoom,
  captureContainScale,
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

  test("keeps the source point under the pointer while zooming", () => {
    expect(
      anchoredPanForZoom(
        { x: 10, y: -20 },
        { x: 300, y: 180 },
        { x: 200, y: 150 },
        1,
        2,
      ),
    ).toEqual({ x: -80, y: -70 });
  });

  test("fits the composed canvas against whichever preview edge is tighter", () => {
    expect(
      captureContainScale(
        { height: 900, width: 1600 },
        { height: 700, width: 800 },
      ),
    ).toBe(0.5);
    expect(
      captureContainScale(
        { height: 1200, width: 600 },
        { height: 600, width: 800 },
      ),
    ).toBe(0.5);
  });

  test("lets a small composed canvas grow to its fitted preview baseline", () => {
    expect(
      captureContainScale(
        { height: 100, width: 200 },
        { height: 600, width: 800 },
      ),
    ).toBe(4);
  });
});
