/**
 * [INPUT]: 依赖 geometry.ts 的区域归一化能力，接收编辑器命令与背景选择
 * [OUTPUT]: 对外提供截图状态、背景模型、命令类型与纯函数状态转移
 * [POS]: capture-window 的唯一业务状态源，padding 表示内容短边的单边整数百分比；系统壁纸只通过 wallpaper.systemId 进入这里
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { clampCaptureRect } from "./geometry.ts";

export const CAPTURE_MIN_REGION_EDGE = 6;
export const CAPTURE_HISTORY_LIMIT = 100;
export const CAPTURE_MIN_ZOOM = 0.4;
export const CAPTURE_MAX_ZOOM = 6;
export const CAPTURE_DEFAULT_PADDING = 8;
export const CAPTURE_MIN_PADDING = 0;
export const CAPTURE_MAX_PADDING = 45;
export const CAPTURE_PADDING_STEP = 1;

export type CaptureRect = {
  height: number;
  width: number;
  x: number;
  y: number;
};

export type CaptureCandidate = CaptureRect & {
  id: string;
};

export type CaptureRegion = {
  color?: string;
  id: string;
  rect: CaptureRect;
  source: "automatic" | "manual";
  style: CaptureRedactionStyle;
};

export type CaptureSize = {
  height: number;
  width: number;
};

export type CaptureSource = CaptureSize & {
  scaleFactor: number;
};

export type CaptureTool = "move" | "redact";
export type CaptureRedactionSource = "auto" | "draw";
export type CapturePointerIntent = "draw" | "ignore" | "pan" | "region";
export type CaptureRedactionStyle = "mosaic" | "blur" | "solid";
export type CaptureHistoryShortcutModifiers = {
  control: boolean;
  modifier: boolean;
  shift: boolean;
};
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
  | "graphite"
  | "rose"
  | "ultraviolet"
  | "lagoon"
  | "mint"
  | "sunset"
  | "prism"
  | "blossom"
  | "coral"
  | "aurora"
  | "dusk"
  | "horizon"
  | "twilight"
  | "flare"
  | "spectrum"
  | "nocturne";

export type CaptureBackground =
  | { id: CapturePresetId; kind: "preset" }
  | { color: string; kind: "color" }
  | { kind: "transparent" }
  | { dataUrl: string; kind: "wallpaper"; systemId?: string };

export type CaptureOpaqueBackground = Exclude<CaptureBackground, { kind: "transparent" }>;

export type CaptureRegionHistory = {
  future: CaptureRegion[][];
  past: CaptureRegion[][];
};

export type CaptureWindowState = {
  background: CaptureBackground;
  gradientsExpanded: boolean;
  history: CaptureRegionHistory;
  lastOpaqueBackground: CaptureOpaqueBackground;
  padding: number;
  privacyEnabled: boolean;
  redactionSource: CaptureRedactionSource;
  redactionStyle: CaptureRedactionStyle;
  regions: CaptureRegion[];
  shadow: boolean;
  solidColor: string;
  source: CaptureSource;
  sourceRevision: number;
  tool: CaptureTool;
  zoom: number;
};

export type CaptureWindowCommand =
  | { id: string; kind: "add-region"; rect: CaptureRect }
  | { kind: "clear-regions" }
  | { id: string; kind: "remove-region" }
  | { id: string; kind: "select-automatic-region"; rect: CaptureRect }
  | { background: CaptureBackground; kind: "set-background" }
  | { enabled: boolean; kind: "set-privacy" }
  | { kind: "set-padding"; padding: number }
  | { kind: "set-redaction-source"; source: CaptureRedactionSource }
  | { kind: "set-redaction-style"; style: CaptureRedactionStyle }
  | { kind: "set-shadow"; shadow: boolean }
  | { color: string; kind: "set-solid-color" }
  | { enabled: boolean; kind: "set-transparent-background" }
  | { kind: "set-tool"; tool: CaptureTool }
  | { kind: "set-zoom"; zoom: number }
  | { kind: "retake"; source: CaptureSource }
  | { kind: "redo" }
  | { kind: "toggle-gradients" }
  | { kind: "undo" };

export function createCaptureWindowState(source: CaptureSource): CaptureWindowState {
  return {
    background: { id: "sea", kind: "preset" },
    gradientsExpanded: false,
    history: { future: [], past: [] },
    lastOpaqueBackground: { id: "sea", kind: "preset" },
    padding: CAPTURE_DEFAULT_PADDING,
    privacyEnabled: true,
    redactionSource: "auto",
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

export function capturePointerIntent(
  tool: CaptureTool,
  button: number,
  overRegion: boolean,
): CapturePointerIntent {
  if (button === 1) return "pan";
  if (button !== 0) return "ignore";
  if (tool === "move") return "pan";
  return overRegion ? "region" : "draw";
}

export function scaleCaptureZoom(zoom: number, factor: number): number {
  return clamp(zoom * factor, CAPTURE_MIN_ZOOM, CAPTURE_MAX_ZOOM);
}

export function wheelCaptureZoom(zoom: number, deltaY: number): number {
  return scaleCaptureZoom(zoom, Math.exp(-deltaY * 0.0016));
}

export function captureHistoryShortcut(
  key: string,
  modifiers: CaptureHistoryShortcutModifiers,
): "redo" | "undo" | null {
  const normalizedKey = key.toLowerCase();
  if (modifiers.modifier && normalizedKey === "z") {
    return modifiers.shift ? "redo" : "undo";
  }
  if (modifiers.control && normalizedKey === "y") return "redo";
  return null;
}

export function applyCaptureCommand(
  state: CaptureWindowState,
  command: CaptureWindowCommand,
): CaptureWindowState {
  switch (command.kind) {
    case "add-region":
      return addRegion(state, command.id, command.rect);
    case "clear-regions":
      return state.regions.length === 0 ? state : commitRegions(state, []);
    case "remove-region":
      return removeRegion(state, command.id);
    case "redo":
      return redoRegions(state);
    case "retake":
      return { ...state, source: command.source, sourceRevision: state.sourceRevision + 1 };
    case "set-background":
      return setCaptureBackground(state, command.background);
    case "set-padding":
      return { ...state, padding: normalizePadding(command.padding), zoom: 1 };
    case "set-privacy":
      return { ...state, privacyEnabled: command.enabled };
    case "set-redaction-source":
      return { ...state, redactionSource: command.source };
    case "select-automatic-region":
      return selectAutomaticRegion(state, command.id, command.rect);
    case "set-redaction-style":
      return { ...state, redactionStyle: command.style };
    case "set-shadow":
      return { ...state, shadow: command.shadow };
    case "set-solid-color":
      return { ...state, solidColor: command.color };
    case "set-tool":
      return { ...state, tool: command.tool };
    case "set-transparent-background":
      if (command.enabled) {
        return setCaptureBackground(state, { kind: "transparent" });
      }
      return state.background.kind === "transparent"
        ? setCaptureBackground(state, state.lastOpaqueBackground)
        : state;
    case "set-zoom":
      return { ...state, zoom: clamp(command.zoom, CAPTURE_MIN_ZOOM, CAPTURE_MAX_ZOOM) };
    case "toggle-gradients":
      return { ...state, gradientsExpanded: !state.gradientsExpanded };
    case "undo":
      return undoRegions(state);
  }
}

function setCaptureBackground(
  state: CaptureWindowState,
  background: CaptureBackground,
): CaptureWindowState {
  if (background.kind === "transparent") {
    return state.background.kind === "transparent" ? state : { ...state, background };
  }
  return { ...state, background, lastOpaqueBackground: background };
}

function addRegion(
  state: CaptureWindowState,
  id: string,
  rect: CaptureRect,
): CaptureWindowState {
  const normalized = clampCaptureRect(rect, state.source);
  if (
    normalized.width < CAPTURE_MIN_REGION_EDGE ||
    normalized.height < CAPTURE_MIN_REGION_EDGE
  ) {
    return state;
  }
  return commitRegions(state, [
    ...state.regions,
    createRegion(state, id, normalized, "manual"),
  ]);
}

function selectAutomaticRegion(
  state: CaptureWindowState,
  id: string,
  rect: CaptureRect,
): CaptureWindowState {
  if (state.regions.some((region) => region.id === id)) return state;
  const normalized = clampCaptureRect(rect, state.source);
  return commitRegions(state, [
    ...state.regions,
    createRegion(state, id, normalized, "automatic"),
  ]);
}

function createRegion(
  state: CaptureWindowState,
  id: string,
  rect: CaptureRect,
  source: CaptureRegion["source"],
): CaptureRegion {
  const region: CaptureRegion = {
    id,
    rect,
    source,
    style: state.redactionStyle,
  };
  if (state.redactionStyle === "solid") region.color = state.solidColor;
  return region;
}

function removeRegion(state: CaptureWindowState, id: string): CaptureWindowState {
  const regions = state.regions.filter((region) => region.id !== id);
  return regions.length === state.regions.length ? state : commitRegions(state, regions);
}

function commitRegions(state: CaptureWindowState, regions: CaptureRegion[]): CaptureWindowState {
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
  const clamped = clamp(Number.isFinite(padding) ? padding : 0, CAPTURE_MIN_PADDING, CAPTURE_MAX_PADDING);
  return Math.round(clamped / CAPTURE_PADDING_STEP) * CAPTURE_PADDING_STEP;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
