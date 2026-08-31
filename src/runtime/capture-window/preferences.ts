import {
  CAPTURE_MAX_PADDING,
  CAPTURE_MIN_PADDING,
  type CaptureBackground,
  type CapturePresetId,
  type CaptureWindowCommand,
  type CaptureWindowState,
} from "./model.ts";

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
  | { kind: "wallpaper" };

export type CaptureWindowPreferences = {
  background: CapturePreferenceBackground;
  padding: number;
  privacyEnabled: boolean;
  shadow: boolean;
};

const DEFAULT_CAPTURE_PREFERENCES: CaptureWindowPreferences = {
  background: { id: "sea", kind: "preset" },
  padding: 64,
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
    ? { kind: "wallpaper" as const }
    : state.background;
  const preferences: CaptureWindowPreferences = {
    background,
    padding: state.padding,
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
    ...state,
    background,
    padding: preferences.padding,
    privacyEnabled: preferences.privacyEnabled,
    shadow: preferences.shadow,
  };
}

export function isCapturePreferenceCommand(command: CaptureWindowCommand): boolean {
  return command.kind === "set-background" ||
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
    normalized.padding = Math.min(
      CAPTURE_MAX_PADDING,
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
