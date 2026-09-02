import type { CaptureRect, CaptureSize } from "./model.ts";

export type CaptureViewport = {
  scale: number;
  x: number;
  y: number;
};

export type CapturePoint = {
  x: number;
  y: number;
};

export function captureContainScale(content: CaptureSize, bounds: CaptureSize): number {
  const contentWidth = Math.max(1, content.width);
  const contentHeight = Math.max(1, content.height);
  const boundsWidth = Math.max(1, bounds.width);
  const boundsHeight = Math.max(1, bounds.height);
  return Math.min(boundsWidth / contentWidth, boundsHeight / contentHeight);
}

export function anchoredPanForZoom(
  pan: CapturePoint,
  pointer: CapturePoint,
  origin: CapturePoint,
  previousZoom: number,
  nextZoom: number,
): CapturePoint {
  const safePreviousZoom = previousZoom > 0 ? previousZoom : 1;
  const sourceX = (pointer.x - origin.x - pan.x) / safePreviousZoom;
  const sourceY = (pointer.y - origin.y - pan.y) / safePreviousZoom;
  return {
    x: pointer.x - origin.x - sourceX * nextZoom,
    y: pointer.y - origin.y - sourceY * nextZoom,
  };
}

export function normalizeCaptureRect(rect: CaptureRect): CaptureRect {
  const x = rect.width < 0 ? rect.x + rect.width : rect.x;
  const y = rect.height < 0 ? rect.y + rect.height : rect.y;
  return {
    height: Math.abs(rect.height),
    width: Math.abs(rect.width),
    x,
    y,
  };
}

export function clampCaptureRect(rect: CaptureRect, bounds: CaptureSize): CaptureRect {
  const normalized = normalizeCaptureRect(rect);
  const x = clamp(normalized.x, 0, bounds.width);
  const y = clamp(normalized.y, 0, bounds.height);
  const right = clamp(normalized.x + normalized.width, 0, bounds.width);
  const bottom = clamp(normalized.y + normalized.height, 0, bounds.height);
  return {
    height: Math.max(0, bottom - y),
    width: Math.max(0, right - x),
    x,
    y,
  };
}

export function viewportRectToSource(
  rect: CaptureRect,
  viewport: CaptureViewport,
  source: CaptureSize,
): CaptureRect {
  const scale = viewport.scale > 0 ? viewport.scale : 1;
  const sourceRect = {
    height: rect.height / scale,
    width: rect.width / scale,
    x: (rect.x - viewport.x) / scale,
    y: (rect.y - viewport.y) / scale,
  };
  return clampCaptureRect(sourceRect, source);
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
