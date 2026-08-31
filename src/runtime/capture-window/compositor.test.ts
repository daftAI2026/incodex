import { describe, expect, test } from "bun:test";
import { captureOutputSize, createCaptureRenderPlan } from "./compositor.ts";
import { createCaptureWindowState } from "./model.ts";

describe("capture window compositor plan", () => {
  test("adds logical padding on every side", () => {
    expect(captureOutputSize({ width: 1200, height: 801 }, 64)).toEqual({
      width: 1328,
      height: 929,
    });
  });

  test("keeps background, window, automatic masks, and manual masks in stable order", () => {
    const state = createCaptureWindowState({ width: 1200, height: 801, scaleFactor: 2 });
    const plan = createCaptureRenderPlan(
      {
        ...state,
        regions: [{ x: 40, y: 50, width: 80, height: 60 }],
      },
      [{ x: 10, y: 20, width: 30, height: 24 }],
    );

    expect(plan.map((operation) => operation.kind)).toEqual([
      "background",
      "window",
      "redaction",
      "redaction",
    ]);
    expect(plan.at(-1)).toMatchObject({
      kind: "redaction",
      rect: { x: 104, y: 114, width: 80, height: 60 },
      style: "mosaic",
    });
  });
});
