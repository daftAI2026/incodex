/**
 * [INPUT]: 依赖截图状态、合成或偏好模块的公开契约。
 * [OUTPUT]: 验证短边百分比及旧逻辑像素偏好的兼容边界。
 * [POS]: capture-window 百分比迁移回归，保护预览与导出的一致性。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import {
  captureOutputSize,
  captureGradientVector,
  captureWindowCornerRadius,
  captureWindowShadow,
  createCaptureRenderPlan,
  redactionSampling,
} from "./compositor.ts";
import { createCaptureWindowState } from "./model.ts";

describe("capture window compositor plan", () => {
  test("uses the same named gradient directions as the CSS swatches", () => {
    const size = { height: 300, width: 500 };
    expect(captureGradientVector("bottom-right", size)).toEqual([0, 0, 500, 300]);
    expect(captureGradientVector("top-right", size)).toEqual([0, 300, 500, 0]);
    expect(captureGradientVector("right", size)).toEqual([0, 150, 500, 150]);
    expect(captureGradientVector("bottom", size)).toEqual([250, 0, 250, 300]);
  });
  test("uses short-side percent without multiplying physical source pixels by DPR again", () => {
    expect(captureOutputSize({ width: 1200, height: 801, scaleFactor: 2 }, 10)).toEqual({
      width: 1360,
      height: 961,
    });
  });

  test("uses the observed platform window radius independently of padding", () => {
    expect(captureWindowCornerRadius(true, 2)).toBe(52);
    expect(captureWindowCornerRadius(false, 2)).toBe(24);
  });

  test("matches the reference window shadow falloff", () => {
    expect(captureWindowShadow(64, 2)).toEqual({
      blur: 90,
      color: "rgba(15, 18, 26, 0.38)",
      offsetY: 32,
    });
    expect(captureWindowShadow(0, 2)).toEqual({
      blur: 0,
      color: "rgba(15, 18, 26, 0.38)",
      offsetY: 0,
    });
  });

  test("places a solid light-gray window base beneath translucent Codex pixels", () => {
    const state = createCaptureWindowState({ width: 1200, height: 801, scaleFactor: 2 });

    const plan = createCaptureRenderPlan(state);

    expect(plan.slice(0, 3)).toEqual([
      expect.objectContaining({ kind: "background" }),
      {
        color: "#f4f4f4",
        kind: "window-underlay",
        rect: { x: 64, y: 64, width: 1200, height: 801 },
      },
      expect.objectContaining({ kind: "window", shadow: true }),
    ]);
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
          style: "mosaic",
        },
        {
          id: "manual-1",
          rect: { x: 40, y: 50, width: 80, height: 60 },
          source: "manual",
          style: "blur",
        },
      ],
    });

    expect(plan.map((operation) => operation.kind)).toEqual([
      "background",
      "window-underlay",
      "window",
      "redaction",
      "redaction",
    ]);
    expect(plan.at(-1)).toMatchObject({
      kind: "redaction",
      rect: { x: 104, y: 114, width: 80, height: 60 },
      style: "blur",
    });
    expect(plan[2]).toMatchObject({
      kind: "window",
      rect: { x: 64, y: 64, width: 1200, height: 801 },
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

  test("keeps editor redactions independent from the pre-capture privacy switch", () => {
    const state = createCaptureWindowState({ width: 1200, height: 801, scaleFactor: 2 });
    const plan = createCaptureRenderPlan({
      ...state,
      privacyEnabled: false,
      regions: [
        {
          id: "automatic-1",
          rect: { x: 10, y: 20, width: 30, height: 24 },
          source: "automatic",
          style: "mosaic",
        },
      ],
    });

    expect(plan.map((operation) => operation.kind)).toEqual([
      "background",
      "window-underlay",
      "window",
      "redaction",
    ]);
  });
});

test("percent padding is orientation symmetric and scales with the image", () => {
  expect(captureOutputSize({ width: 800, height: 1200, scaleFactor: 1 }, 10)).toEqual({ width: 960, height: 1360 });
  expect(captureOutputSize({ width: 2400, height: 1600, scaleFactor: 2 }, 10)).toEqual({ width: 2720, height: 1920 });
  expect(captureOutputSize({ width: 1200, height: 800, scaleFactor: 2 }, 0)).toEqual({ width: 1200, height: 800 });
});
