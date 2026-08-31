import { clampCaptureRect } from "./geometry.ts";

export const CAPTURE_MIN_REGION_EDGE = 6;
export const CAPTURE_HISTORY_LIMIT = 100;
export const CAPTURE_MIN_ZOOM = 0.4;
export const CAPTURE_MAX_ZOOM = 6;
export const CAPTURE_MIN_PADDING = 0;
export const CAPTURE_MAX_PADDING = 160;
export const CAPTURE_PADDING_STEP = 4;

export type CaptureRect = {
  height: number;
  width: number;
  x: number;
  y: number;
};

export type CaptureSize = {
  height: number;
  width: number;
};

export type CaptureSource = CaptureSize & {
  scaleFactor: number;
};

export type CaptureTool = "move" | "redact";
export type CaptureRedactionStyle = "mosaic" | "blur" | "solid";
export type CapturePresetId =
  | "sea"
  | "canyon"
  | "mist"
  | "highland"
  | "ocean"
  | "silver"
  | "azure"
  | "indigo"
  | "ember"
  | "graphite";

export type CaptureBackground =
  | { id: CapturePresetId; kind: "preset" }
  | { color: string; kind: "color" }
  | { kind: "transparent" }
  | { dataUrl: string; kind: "wallpaper" };

export type CaptureRegionHistory = {
  future: CaptureRect[][];
  past: CaptureRect[][];
};

export type CaptureWindowState = {
  background: CaptureBackground;
  history: CaptureRegionHistory;
  padding: number;
  privacyEnabled: boolean;
  redactionStyle: CaptureRedactionStyle;
  regions: CaptureRect[];
  shadow: boolean;
  solidColor: string;
  source: CaptureSource;
  sourceRevision: number;
  tool: CaptureTool;
  zoom: number;
};

export type CaptureWindowCommand =
  | { kind: "add-region"; rect: CaptureRect }
  | { kind: "clear-regions" }
  | { background: CaptureBackground; kind: "set-background" }
  | { enabled: boolean; kind: "set-privacy" }
  | { kind: "set-padding"; padding: number }
  | { kind: "set-redaction-style"; style: CaptureRedactionStyle }
  | { kind: "set-shadow"; shadow: boolean }
  | { color: string; kind: "set-solid-color" }
  | { kind: "set-tool"; tool: CaptureTool }
  | { kind: "set-zoom"; zoom: number }
  | { kind: "retake"; source: CaptureSource }
  | { kind: "redo" }
  | { kind: "undo" };

export function createCaptureWindowState(source: CaptureSource): CaptureWindowState {
  return {
    background: { id: "sea", kind: "preset" },
    history: { future: [], past: [] },
    padding: 64,
    privacyEnabled: true,
    redactionStyle: "mosaic",
    regions: [],
    shadow: true,
    solidColor: "#101114",
    source,
    sourceRevision: 0,
    tool: "move",
    zoom: 1,
  };
}

export function applyCaptureCommand(
  state: CaptureWindowState,
  command: CaptureWindowCommand,
): CaptureWindowState {
  switch (command.kind) {
    case "add-region":
      return addRegion(state, command.rect);
    case "clear-regions":
      return state.regions.length === 0 ? state : commitRegions(state, []);
    case "redo":
      return redoRegions(state);
    case "retake":
      return { ...state, source: command.source, sourceRevision: state.sourceRevision + 1 };
    case "set-background":
      return { ...state, background: command.background };
    case "set-padding":
      return { ...state, padding: normalizePadding(command.padding) };
    case "set-privacy":
      return { ...state, privacyEnabled: command.enabled };
    case "set-redaction-style":
      return { ...state, redactionStyle: command.style };
    case "set-shadow":
      return { ...state, shadow: command.shadow };
    case "set-solid-color":
      return { ...state, solidColor: command.color };
    case "set-tool":
      return { ...state, tool: command.tool };
    case "set-zoom":
      return { ...state, zoom: clamp(command.zoom, CAPTURE_MIN_ZOOM, CAPTURE_MAX_ZOOM) };
    case "undo":
      return undoRegions(state);
  }
}

function addRegion(state: CaptureWindowState, rect: CaptureRect): CaptureWindowState {
  const normalized = clampCaptureRect(rect, state.source);
  if (
    normalized.width < CAPTURE_MIN_REGION_EDGE ||
    normalized.height < CAPTURE_MIN_REGION_EDGE
  ) {
    return state;
  }
  return commitRegions(state, [...state.regions, normalized]);
}

function commitRegions(state: CaptureWindowState, regions: CaptureRect[]): CaptureWindowState {
  const past = [...state.history.past, state.regions].slice(-CAPTURE_HISTORY_LIMIT);
  return {
    ...state,
    history: { future: [], past },
    regions,
  };
}

function undoRegions(state: CaptureWindowState): CaptureWindowState {
  const previous = state.history.past.at(-1);
  if (!previous) return state;
  return {
    ...state,
    history: {
      future: [state.regions, ...state.history.future].slice(0, CAPTURE_HISTORY_LIMIT),
      past: state.history.past.slice(0, -1),
    },
    regions: previous,
  };
}

function redoRegions(state: CaptureWindowState): CaptureWindowState {
  const next = state.history.future[0];
  if (!next) return state;
  return {
    ...state,
    history: {
      future: state.history.future.slice(1),
      past: [...state.history.past, state.regions].slice(-CAPTURE_HISTORY_LIMIT),
    },
    regions: next,
  };
}

function normalizePadding(padding: number): number {
  const clamped = clamp(padding, CAPTURE_MIN_PADDING, CAPTURE_MAX_PADDING);
  return Math.round(clamped / CAPTURE_PADDING_STEP) * CAPTURE_PADDING_STEP;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
