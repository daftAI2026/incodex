import type {
  CaptureBackground,
  CapturePresetId,
  CaptureRect,
  CaptureRedactionStyle,
  CaptureSize,
  CaptureSource,
  CaptureWindowState,
} from "./model.ts";
import { capturePresetColors } from "./presets.ts";

export type CaptureRenderOperation =
  | { background: CaptureBackground; kind: "background"; size: CaptureSize }
  | { kind: "window"; rect: CaptureRect; shadow: boolean }
  | {
      color: string;
      kind: "redaction";
      rect: CaptureRect;
      style: CaptureRedactionStyle;
    };

export type CaptureRenderOptions = {
  automaticRegions?: CaptureRect[];
  backgroundImage?: CanvasImageSource | null;
};

export function capturePhysicalPadding(source: CaptureSource, padding: number): number {
  return padding * Math.max(1, source.scaleFactor);
}

export function captureOutputSize(source: CaptureSource, padding: number): CaptureSize {
  const physicalPadding = capturePhysicalPadding(source, padding);
  return {
    height: source.height + physicalPadding * 2,
    width: source.width + physicalPadding * 2,
  };
}

export function createCaptureRenderPlan(
  state: CaptureWindowState,
  automaticRegions: CaptureRect[] = [],
): CaptureRenderOperation[] {
  const size = captureOutputSize(state.source, state.padding);
  const physicalPadding = capturePhysicalPadding(state.source, state.padding);
  const operations: CaptureRenderOperation[] = [
    { background: state.background, kind: "background", size },
    {
      kind: "window",
      rect: {
        height: state.source.height,
        width: state.source.width,
        x: physicalPadding,
        y: physicalPadding,
      },
      shadow: state.shadow,
    },
  ];
  const regions = state.privacyEnabled ? [...automaticRegions, ...state.regions] : state.regions;
  for (const rect of regions) {
    operations.push({
      color: state.solidColor,
      kind: "redaction",
      rect: {
        ...rect,
        x: rect.x + physicalPadding,
        y: rect.y + physicalPadding,
      },
      style: state.redactionStyle,
    });
  }
  return operations;
}

export function renderCaptureToCanvas(
  source: CanvasImageSource,
  state: CaptureWindowState,
  options: CaptureRenderOptions = {},
): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  const size = captureOutputSize(state.source, state.padding);
  canvas.width = Math.round(size.width);
  canvas.height = Math.round(size.height);
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Canvas 2D is unavailable");

  drawBackground(context, state.background, size, options.backgroundImage ?? null);
  drawWindow(context, source, state);

  const automaticRegions = options.automaticRegions ?? [];
  const regions = state.privacyEnabled
    ? [...automaticRegions, ...state.regions]
    : state.regions;
  for (const region of regions) {
    drawRedaction(context, source, region, state);
  }
  return canvas;
}

function drawBackground(
  context: CanvasRenderingContext2D,
  background: CaptureBackground,
  size: CaptureSize,
  backgroundImage: CanvasImageSource | null,
): void {
  context.clearRect(0, 0, size.width, size.height);
  if (background.kind === "transparent") return;
  if (background.kind === "wallpaper" && backgroundImage) {
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
  const gradient = context.createLinearGradient(0, 0, size.width, size.height);
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
): void {
  const padding = capturePhysicalPadding(state.source, state.padding);
  const scaleFactor = Math.max(1, state.source.scaleFactor);
  if (state.shadow && padding > 0) {
    context.save();
    context.shadowColor = "rgba(15,18,26,.34)";
    context.shadowBlur = Math.min(56, state.padding * 0.7) * scaleFactor;
    context.shadowOffsetY = Math.min(20, state.padding * 0.25) * scaleFactor;
    context.fillStyle = "rgba(15,18,26,.18)";
    roundedRectPath(
      context,
      padding,
      padding,
      state.source.width,
      state.source.height,
      Math.min(18, state.padding) * scaleFactor,
    );
    context.fill();
    context.restore();
  }
  context.save();
  roundedRectPath(
    context,
    padding,
    padding,
    state.source.width,
    state.source.height,
    Math.min(18, state.padding) * scaleFactor,
  );
  context.clip();
  context.drawImage(source, padding, padding, state.source.width, state.source.height);
  context.restore();
}

function drawRedaction(
  context: CanvasRenderingContext2D,
  source: CanvasImageSource,
  region: CaptureRect,
  state: CaptureWindowState,
): void {
  const scaleFactor = Math.max(1, state.source.scaleFactor);
  const padding = capturePhysicalPadding(state.source, state.padding);
  const target = {
    height: region.height,
    width: region.width,
    x: region.x + padding,
    y: region.y + padding,
  };
  context.save();
  roundedRectPath(context, target.x, target.y, target.width, target.height, 5 * scaleFactor);
  context.clip();

  if (state.redactionStyle === "solid") {
    context.fillStyle = state.solidColor;
    context.fillRect(target.x, target.y, target.width, target.height);
    context.restore();
    return;
  }

  if (state.redactionStyle === "blur") {
    const bleed = 8 * scaleFactor;
    context.filter = `blur(${10 * scaleFactor}px)`;
    context.drawImage(
      source,
      region.x,
      region.y,
      region.width,
      region.height,
      target.x - bleed,
      target.y - bleed,
      target.width + bleed * 2,
      target.height + bleed * 2,
    );
    context.restore();
    return;
  }

  const blockSize = 9 * scaleFactor;
  const small = document.createElement("canvas");
  small.width = Math.max(1, Math.ceil(region.width / blockSize));
  small.height = Math.max(1, Math.ceil(region.height / blockSize));
  const smallContext = small.getContext("2d");
  if (smallContext) {
    smallContext.imageSmoothingEnabled = false;
    smallContext.drawImage(
      source,
      region.x,
      region.y,
      region.width,
      region.height,
      0,
      0,
      small.width,
      small.height,
    );
    context.imageSmoothingEnabled = false;
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
