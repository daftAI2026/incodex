/**
 * [INPUT]: 依赖 model 的短边百分比状态、presets 的背景以及 Canvas 绘制能力。
 * [OUTPUT]: 提供合成尺寸、绘制计划与预览/导出共享渲染；统一将百分比转为物理像素。
 * [POS]: capture-window 的像素几何真源，regions 与 editor 复用同一 padding 换算。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import type {
  CaptureBackground,
  CapturePresetId,
  CaptureRect,
  CaptureRedactionStyle,
  CaptureSize,
  CaptureSource,
  CaptureWindowState,
} from "./model.ts";
import {
  type CaptureGradientDirection,
  capturePresetColors,
  capturePresetDirection,
} from "./presets.ts";

const CAPTURE_WINDOW_UNDERLAY_COLOR = "#f4f4f4";

export type CaptureRenderOperation =
  | { background: CaptureBackground; kind: "background"; size: CaptureSize }
  | { color: typeof CAPTURE_WINDOW_UNDERLAY_COLOR; kind: "window-underlay"; rect: CaptureRect }
  | { kind: "window"; rect: CaptureRect; shadow: boolean }
  | {
      color: string;
      kind: "redaction";
      rect: CaptureRect;
      style: CaptureRedactionStyle;
    };

export type CaptureRenderOptions = {
  backgroundImage?: CanvasImageSource | null;
  isMacOS: boolean;
};

export type RedactionSampling = {
  blockSize: number;
  smoothing: boolean;
};

export type CaptureWindowShadow = {
  blur: number;
  color: "rgba(15, 18, 26, 0.38)";
  offsetY: number;
};

export function redactionSampling(
  style: CaptureRedactionStyle,
  scaleFactor: number,
): RedactionSampling | null {
  if (style === "solid") return null;
  return {
    blockSize: 9 * Math.max(1, scaleFactor),
    smoothing: style === "blur",
  };
}

export function capturePhysicalPadding(source: CaptureSource, padding: number): number {
  return Math.round(Math.max(1, Math.min(source.width, source.height)) * padding / 100);
}

export function captureOutputSize(source: CaptureSource, padding: number): CaptureSize {
  const physicalPadding = capturePhysicalPadding(source, padding);
  return {
    height: source.height + physicalPadding * 2,
    width: source.width + physicalPadding * 2,
  };
}

export function captureWindowCornerRadius(
  isMacOS: boolean,
  scaleFactor: number,
): number {
  return (isMacOS ? 26 : 12) * Math.max(1, scaleFactor);
}

export function captureWindowShadow(padding: number, scaleFactor: number): CaptureWindowShadow {
  const scale = Math.max(1, scaleFactor);
  return {
    blur: Math.round(Math.min(56, padding * 0.7) * scale),
    color: "rgba(15, 18, 26, 0.38)",
    offsetY: Math.round(Math.min(20, padding * 0.25) * scale),
  };
}

export function createCaptureRenderPlan(
  state: CaptureWindowState,
): CaptureRenderOperation[] {
  const size = captureOutputSize(state.source, state.padding);
  const physicalPadding = capturePhysicalPadding(state.source, state.padding);
  const windowRect = {
    height: state.source.height,
    width: state.source.width,
    x: physicalPadding,
    y: physicalPadding,
  };
  const operations: CaptureRenderOperation[] = [
    { background: state.background, kind: "background", size },
    { color: CAPTURE_WINDOW_UNDERLAY_COLOR, kind: "window-underlay", rect: windowRect },
    {
      kind: "window",
      rect: windowRect,
      shadow: state.shadow,
    },
  ];
  const regions = activeCaptureRegions(state);
  for (const region of regions) {
    const { rect } = region;
    operations.push({
      color: region.color ?? state.solidColor,
      kind: "redaction",
      rect: {
        ...rect,
        x: rect.x + physicalPadding,
        y: rect.y + physicalPadding,
      },
      style: region.style,
    });
  }
  return operations;
}

export function renderCaptureToCanvas(
  source: CanvasImageSource,
  state: CaptureWindowState,
  options: CaptureRenderOptions,
): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  const size = captureOutputSize(state.source, state.padding);
  canvas.width = Math.round(size.width);
  canvas.height = Math.round(size.height);
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Canvas 2D is unavailable");

  drawBackground(context, state.background, size, options.backgroundImage ?? null);
  drawWindow(context, source, state, options.isMacOS);

  const regions = activeCaptureRegions(state);
  for (const region of regions) {
    drawRedaction(context, source, region, state);
  }
  return canvas;
}

function activeCaptureRegions(state: CaptureWindowState): CaptureWindowState["regions"] {
  return state.regions;
}

function drawBackground(
  context: CanvasRenderingContext2D,
  background: CaptureBackground,
  size: CaptureSize,
  backgroundImage: CanvasImageSource | null,
): void {
  context.clearRect(0, 0, size.width, size.height);
  if (background.kind === "transparent") return;
  if ((background.kind === "preset" || background.kind === "wallpaper") && backgroundImage) {
    drawCoverImage(context, backgroundImage, size);
    return;
  }
  if (background.kind === "color") {
    context.fillStyle = background.color;
    context.fillRect(0, 0, size.width, size.height);
    return;
  }
  const presetId = background.kind === "preset" ? background.id : "graphite";
  drawPreset(context, presetId, size);
}

function drawPreset(
  context: CanvasRenderingContext2D,
  presetId: CapturePresetId,
  size: CaptureSize,
): void {
  const colors = capturePresetColors(presetId);
  const gradient = context.createLinearGradient(
    ...captureGradientVector(capturePresetDirection(presetId), size),
  );
  gradient.addColorStop(0, colors[0]);
  gradient.addColorStop(0.52, colors[1]);
  gradient.addColorStop(1, colors[2]);
  context.fillStyle = gradient;
  context.fillRect(0, 0, size.width, size.height);

  const glow = context.createRadialGradient(
    size.width * 0.72,
    size.height * 0.18,
    0,
    size.width * 0.72,
    size.height * 0.18,
    Math.max(size.width, size.height) * 0.62,
  );
  glow.addColorStop(0, "rgba(255,255,255,.22)");
  glow.addColorStop(1, "rgba(255,255,255,0)");
  context.fillStyle = glow;
  context.fillRect(0, 0, size.width, size.height);
}

export function captureGradientVector(
  direction: CaptureGradientDirection,
  size: CaptureSize,
): [number, number, number, number] {
  switch (direction) {
    case "bottom":
      return [size.width / 2, 0, size.width / 2, size.height];
    case "right":
      return [0, size.height / 2, size.width, size.height / 2];
    case "top-right":
      return [0, size.height, size.width, 0];
    case "bottom-right":
      return [0, 0, size.width, size.height];
  }
}

function drawCoverImage(
  context: CanvasRenderingContext2D,
  image: CanvasImageSource,
  size: CaptureSize,
): void {
  const dimensions = sourceDimensions(image);
  const scale = Math.max(size.width / dimensions.width, size.height / dimensions.height);
  const width = dimensions.width * scale;
  const height = dimensions.height * scale;
  context.drawImage(image, (size.width - width) / 2, (size.height - height) / 2, width, height);
}

function drawWindow(
  context: CanvasRenderingContext2D,
  source: CanvasImageSource,
  state: CaptureWindowState,
  isMacOS: boolean,
): void {
  const padding = capturePhysicalPadding(state.source, state.padding);
  const scaleFactor = Math.max(1, state.source.scaleFactor);
  const cornerRadius = captureWindowCornerRadius(isMacOS, scaleFactor);
  context.save();
  roundedRectPath(
    context,
    padding,
    padding,
    state.source.width,
    state.source.height,
    cornerRadius,
  );
  if (state.shadow) {
    const shadow = captureWindowShadow(padding / scaleFactor, scaleFactor);
    context.shadowColor = shadow.color;
    context.shadowBlur = shadow.blur;
    context.shadowOffsetY = shadow.offsetY;
  }
  context.fillStyle = CAPTURE_WINDOW_UNDERLAY_COLOR;
  context.fill();
  context.shadowColor = "transparent";
  context.shadowBlur = 0;
  context.shadowOffsetY = 0;
  context.clip();
  context.drawImage(source, padding, padding, state.source.width, state.source.height);
  context.restore();
}

function drawRedaction(
  context: CanvasRenderingContext2D,
  source: CanvasImageSource,
  region: CaptureWindowState["regions"][number],
  state: CaptureWindowState,
): void {
  const { rect } = region;
  const scaleFactor = Math.max(1, state.source.scaleFactor);
  const padding = capturePhysicalPadding(state.source, state.padding);
  const target = {
    height: rect.height,
    width: rect.width,
    x: rect.x + padding,
    y: rect.y + padding,
  };
  context.save();
  roundedRectPath(context, target.x, target.y, target.width, target.height, 5 * scaleFactor);
  context.clip();

  if (region.style === "solid") {
    context.fillStyle = region.color ?? state.solidColor;
    context.fillRect(target.x, target.y, target.width, target.height);
    context.restore();
    return;
  }

  const sampling = redactionSampling(region.style, scaleFactor);
  if (!sampling) {
    context.restore();
    return;
  }
  const small = document.createElement("canvas");
  small.width = Math.max(1, Math.ceil(rect.width / sampling.blockSize));
  small.height = Math.max(1, Math.ceil(rect.height / sampling.blockSize));
  const smallContext = small.getContext("2d");
  if (smallContext) {
    smallContext.imageSmoothingEnabled = sampling.smoothing;
    if (sampling.smoothing) smallContext.imageSmoothingQuality = "high";
    smallContext.drawImage(
      source,
      rect.x,
      rect.y,
      rect.width,
      rect.height,
      0,
      0,
      small.width,
      small.height,
    );
    context.imageSmoothingEnabled = sampling.smoothing;
    if (sampling.smoothing) context.imageSmoothingQuality = "high";
    context.drawImage(small, target.x, target.y, target.width, target.height);
  }
  context.restore();
}

function sourceDimensions(source: CanvasImageSource): CaptureSize {
  if (source instanceof HTMLCanvasElement || source instanceof HTMLImageElement) {
    return { height: source.height, width: source.width };
  }
  if (source instanceof ImageBitmap) {
    return { height: source.height, width: source.width };
  }
  return { height: 1, width: 1 };
}

function roundedRectPath(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
): void {
  context.beginPath();
  context.roundRect(x, y, width, height, Math.max(0, radius));
}
