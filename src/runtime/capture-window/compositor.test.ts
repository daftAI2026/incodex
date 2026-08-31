import { describe, expect, test } from "bun:test";
import {
  captureOutputSize,
  createCaptureRenderPlan,
  redactionSampling,
} from "./compositor.ts";
import { createCaptureWindowState } from "./model.ts";

describe("capture window compositor plan", () => {
  test("converts logical padding to physical output pixels", () => {
    expect(captureOutputSize({ width: 1200, height: 801, scaleFactor: 2 }, 64)).toEqual({
      width: 1456,
      height: 1057,
    });
  });

  test("exports only confirmed automatic and manual masks in stable order", () => {
    const state = createCaptureWindowState({ width: 1200, height: 801, scaleFactor: 2 });
    const plan = createCaptureRenderPlan({
      ...state,
      regions: [
        {
          id: "automatic-1",
          rect: { x: 10, y: 20, width: 30, height: 24 },
          source: "automatic",
        },
        {
          id: "manual-1",
          rect: { x: 40, y: 50, width: 80, height: 60 },
          source: "manual",
        },
      ],
    });

    expect(plan.map((operation) => operation.kind)).toEqual([
      "background",
      "window",
      "redaction",
      "redaction",
    ]);
    expect(plan.at(-1)).toMatchObject({
      kind: "redaction",
      rect: { x: 168, y: 178, width: 80, height: 60 },
      style: "mosaic",
    });
    expect(plan[1]).toMatchObject({
      kind: "window",
      rect: { x: 128, y: 128, width: 1200, height: 801 },
    });
  });

  test("matches the observed mosaic and blur-like sampling behavior", () => {
    expect(redactionSampling("mosaic", 2)).toEqual({
      blockSize: 18,
      smoothing: false,
    });
    expect(redactionSampling("blur", 2)).toEqual({
      blockSize: 18,
      smoothing: true,
    });
    expect(redactionSampling("solid", 2)).toBeNull();
  });
});
