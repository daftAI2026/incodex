/**
 * [INPUT]: 依赖编辑器状态机与既有浏览器偏好存储。
 * [OUTPUT]: 保存非敏感编辑偏好及当前桌面的来源 ID，不保存图片或本地路径；旧像素记录在取得源尺寸后迁移为百分比。
 * [POS]: capture-window 的偏好边界；来源可用性由主机恢复，背景选择由此处语义恢复。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import {
  applyCaptureCommand,
  CAPTURE_DEFAULT_PADDING,
  CAPTURE_MAX_PADDING,
  CAPTURE_MIN_PADDING,
  type CaptureBackground,
  type CapturePresetId,
  type CaptureWindowCommand,
  type CaptureWindowState,
} from "./model.ts";

const LEGACY_MAX_PADDING = 160;

export const CAPTURE_PREFERENCES_KEY = "incodex-window-capture-prefs";

const CAPTURE_PRESET_IDS: readonly CapturePresetId[] = [
  "sea",
  "canyon",
  "mist",
  "highland",
  "ocean",
  "silver",
  "azure",
  "indigo",
  "ember",
  "graphite",
];

export type CapturePreferenceStorage = {
  getItem: (key: string) => string | null;
  setItem: (key: string, value: string) => void;
};

export type CapturePreferenceBackground =
  | Exclude<CaptureBackground, { kind: "wallpaper" }>
  | { kind: "wallpaper"; systemId?: "system-wallpaper-current" };

export type CaptureWindowPreferences = {
  background: CapturePreferenceBackground;
  padding: number;
  paddingUnit: "percent" | "logical-px";
  privacyEnabled: boolean;
  shadow: boolean;
};

const DEFAULT_CAPTURE_PREFERENCES: CaptureWindowPreferences = {
  background: { id: "sea", kind: "preset" },
  padding: CAPTURE_DEFAULT_PADDING,
  paddingUnit: "percent",
  privacyEnabled: true,
  shadow: true,
};

export function loadCapturePreferences(
  storage: CapturePreferenceStorage,
): CaptureWindowPreferences {
  try {
    const value = storage.getItem(CAPTURE_PREFERENCES_KEY);
    if (!value) return defaultCapturePreferences();
    return normalizeCapturePreferences(JSON.parse(value));
  } catch {
    return defaultCapturePreferences();
  }
}

export function saveCapturePreferences(
  storage: CapturePreferenceStorage,
  state: CaptureWindowState,
): void {
  const background = state.background.kind === "wallpaper"
    ? { kind: "wallpaper" as const, ...(state.background.systemId === "system-wallpaper-current" ? { systemId: "system-wallpaper-current" as const } : {}) }
    : state.background;
  const preferences: CaptureWindowPreferences = {
    background,
    padding: state.padding,
    paddingUnit: "percent",
    privacyEnabled: state.privacyEnabled,
    shadow: state.shadow,
  };
  try {
    storage.setItem(CAPTURE_PREFERENCES_KEY, JSON.stringify(preferences));
  } catch {
  }
}

export function applyCapturePreferences(
  state: CaptureWindowState,
  preferences: CaptureWindowPreferences,
  wallpaperDataUrl?: string | null,
): CaptureWindowState {
  let background: CaptureBackground;
  if (preferences.background.kind === "wallpaper") {
    background = wallpaperDataUrl
      ? { dataUrl: wallpaperDataUrl, kind: "wallpaper" }
      : { id: "sea", kind: "preset" };
  } else {
    background = preferences.background;
  }
  return {
    ...applyCaptureCommand(state, { background, kind: "set-background" }),
    padding: applyCaptureCommand(state, {
      kind: "set-padding",
      padding: preferences.paddingUnit === "logical-px"
        ? preferences.padding * Math.max(1, state.source.scaleFactor) * 100 / Math.max(1, Math.min(state.source.width, state.source.height))
        : preferences.padding,
    }).padding,
    privacyEnabled: preferences.privacyEnabled,
    shadow: preferences.shadow,
  };
}

export function isCapturePreferenceCommand(command: CaptureWindowCommand): boolean {
  return command.kind === "set-background" ||
    command.kind === "set-transparent-background" ||
    command.kind === "set-padding" ||
    command.kind === "set-privacy" ||
    command.kind === "set-shadow";
}

function normalizeCapturePreferences(input: unknown): CaptureWindowPreferences {
  const normalized = defaultCapturePreferences();
  if (!input || typeof input !== "object") return normalized;
  const record = input as Record<string, unknown>;
  if (typeof record.privacyEnabled === "boolean") {
    normalized.privacyEnabled = record.privacyEnabled;
  }
  if (typeof record.shadow === "boolean") normalized.shadow = record.shadow;
  if (typeof record.padding === "number" && Number.isFinite(record.padding)) {
    // 旧记录没有单位；等源图尺寸可用后再换算，不能把旧像素当百分比。
    normalized.paddingUnit = record.paddingUnit === "percent" ? "percent" : "logical-px";
    normalized.padding = Math.min(
      normalized.paddingUnit === "percent" ? CAPTURE_MAX_PADDING : LEGACY_MAX_PADDING,
      Math.max(CAPTURE_MIN_PADDING, Math.round(record.padding)),
    );
  }
  const background = normalizeBackground(record.background);
  if (background) normalized.background = background;
  return normalized;
}

function normalizeBackground(value: unknown): CapturePreferenceBackground | null {
  if (!value || typeof value !== "object") return null;
  const background = value as Record<string, unknown>;
  if (background.kind === "wallpaper" && background.systemId === "system-wallpaper-current") {
    return { kind: "wallpaper", systemId: "system-wallpaper-current" };
  }
  if (background.kind === "transparent" || background.kind === "wallpaper") {
    return { kind: background.kind };
  }
  if (
    background.kind === "preset" &&
    typeof background.id === "string" &&
    CAPTURE_PRESET_IDS.includes(background.id as CapturePresetId)
  ) {
    return { id: background.id as CapturePresetId, kind: "preset" };
  }
  if (background.kind === "color" && typeof background.color === "string") {
    return { color: background.color, kind: "color" };
  }
  return null;
}

function defaultCapturePreferences(): CaptureWindowPreferences {
  return {
    ...DEFAULT_CAPTURE_PREFERENCES,
    background: { ...DEFAULT_CAPTURE_PREFERENCES.background },
  };
}


export function shouldRestoreCurrentWallpaper(storage: CapturePreferenceStorage | null): boolean {
  try {
    if (!storage?.getItem(CAPTURE_PREFERENCES_KEY)) return true;
    const { background } = loadCapturePreferences(storage);
    return background.kind === "wallpaper" && background.systemId === "system-wallpaper-current";
  } catch {
    return false;
  }
}
