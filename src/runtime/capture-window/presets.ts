/**
 * [INPUT]: 依赖 model.ts 的 CapturePresetId，依赖构建期注入或 preview 静态路由提供壁纸图片
 * [OUTPUT]: 对外提供按 Gradients 与 Wallpapers 分层的预设目录、纯色目录、颜色采样与资源解析
 * [POS]: capture-window 的背景预设真相源，函数生成渐变，只让壁纸进入栅格资源管线
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import type { CaptureBackground, CapturePresetId } from "./model.ts";

export type CaptureBackgroundSectionId =
  | "none"
  | "gradients"
  | "wallpapers"
  | "plain-color";

export type CapturePreset = {
  colors: readonly [string, string, string];
  direction?: CaptureGradientDirection;
  id: CapturePresetId;
  section: "gradients" | "wallpapers";
};

export type CaptureGradientDirection = "bottom" | "bottom-right" | "right" | "top-right";

export const capturePresets: readonly CapturePreset[] = [
  { colors: ["#d7eee8", "#71b6ae", "#2f6870"], id: "sea", section: "wallpapers" },
  { colors: ["#f4cfaa", "#c67b5c", "#65443e"], id: "canyon", section: "wallpapers" },
  { colors: ["#eef2f1", "#aab9b6", "#74817e"], id: "mist", section: "wallpapers" },
  { colors: ["#d8d7b6", "#779175", "#3f5c59"], id: "highland", section: "wallpapers" },
  { colors: ["#b8e8e8", "#4d9bb1", "#24526f"], id: "ocean", section: "wallpapers" },
  { colors: ["#ff8db8", "#d64ac7", "#6540c8"], direction: "bottom-right", id: "rose", section: "gradients" },
  { colors: ["#243f96", "#784fd2", "#ef85c0"], direction: "bottom-right", id: "ultraviolet", section: "gradients" },
  { colors: ["#08275f", "#087fc4", "#26d7df"], direction: "bottom-right", id: "lagoon", section: "gradients" },
  { colors: ["#54bda9", "#b9e6c3", "#fff0c2"], direction: "bottom-right", id: "mint", section: "gradients" },
  { colors: ["#ffca72", "#f56f73", "#bd3c7d"], direction: "bottom-right", id: "sunset", section: "gradients" },
  { colors: ["#35c6dc", "#a276e8", "#cb45dd"], direction: "top-right", id: "silver", section: "gradients" },
  { colors: ["#c8d0d5", "#f2c7cf", "#b8c6c3"], direction: "bottom", id: "azure", section: "gradients" },
  { colors: ["#8ebdb8", "#e9c4a4", "#c98574"], direction: "bottom-right", id: "indigo", section: "gradients" },
  { colors: ["#7256d8", "#a77ce8", "#5cc8e2"], direction: "top-right", id: "ember", section: "gradients" },
  { colors: ["#29113f", "#43145d", "#120a26"], direction: "bottom-right", id: "graphite", section: "gradients" },
  { colors: ["#17436b", "#4e83a5", "#dfa85d"], direction: "bottom", id: "prism", section: "gradients" },
  { colors: ["#5e91cc", "#a68bc2", "#e9a3ad"], direction: "bottom-right", id: "blossom", section: "gradients" },
  { colors: ["#5537c2", "#d176d6", "#47b9d0"], direction: "top-right", id: "coral", section: "gradients" },
  { colors: ["#5b1223", "#a82439", "#d5464a"], direction: "right", id: "aurora", section: "gradients" },
  { colors: ["#244b9b", "#501a54", "#b51e36"], direction: "bottom-right", id: "dusk", section: "gradients" },
  { colors: ["#b9a1ee", "#957cbd", "#e6a1c0"], direction: "bottom-right", id: "horizon", section: "gradients" },
  { colors: ["#ff7c25", "#e73a43", "#ffe08b"], direction: "bottom-right", id: "twilight", section: "gradients" },
  { colors: ["#6c39bb", "#f05f7a", "#ffbc55"], direction: "bottom-right", id: "flare", section: "gradients" },
  { colors: ["#f04a2f", "#f56d62", "#5d1b6e"], direction: "bottom-right", id: "spectrum", section: "gradients" },
  { colors: ["#3156a4", "#6a3592", "#c74783"], direction: "bottom-right", id: "nocturne", section: "gradients" },
] as const;

export const capturePlainColors = [
  "#121212",
  "#ffffff",
  "#e33345",
  "#f78521",
  "#f2a81a",
  "#188f51",
  "#0c8ce8",
  "#8536ec",
  "#383838",
  "#ebebeb",
  "#fabdb5",
  "#ffc570",
  "#fade8f",
  "#a0e8bb",
  "#a9d6f9",
  "#cfaff0",
] as const;

export const captureRasterPresetIds: readonly CapturePresetId[] = capturePresets
  .filter((preset) => preset.section === "wallpapers")
  .map((preset) => preset.id);

export type CapturePresetSection = {
  id: "gradients" | "wallpapers";
  presets: readonly CapturePreset[];
};

export const capturePresetSections: readonly CapturePresetSection[] = [
  {
    id: "gradients",
    presets: capturePresets.filter((preset) => preset.section === "gradients"),
  },
  {
    id: "wallpapers",
    presets: capturePresets.filter((preset) => preset.section === "wallpapers"),
  },
] as const;

export function capturePresetSection(
  id: CapturePresetSection["id"],
): CapturePresetSection {
  const section = capturePresetSections.find((candidate) => candidate.id === id);
  if (!section) throw new Error(`Unknown capture preset section: ${id}`);
  return section;
}

export function captureBackgroundSection(
  background: CaptureBackground,
): CaptureBackgroundSectionId {
  if (background.kind === "transparent") return "none";
  if (background.kind === "color") return "plain-color";
  if (background.kind === "wallpaper") return "wallpapers";
  const preset = capturePresets.find((candidate) => candidate.id === background.id);
  if (!preset) throw new Error(`Unknown capture background preset: ${background.id}`);
  return preset.section;
}

let embeddedCapturePresetAssets: Partial<Record<CapturePresetId, string>> = {};

export function configureCapturePresetAssets(
  assets: Partial<Record<CapturePresetId, string>>,
): void {
  embeddedCapturePresetAssets = { ...assets };
}

export function capturePresetColors(
  presetId: CapturePresetId,
): readonly [string, string, string] {
  return capturePresets.find((preset) => preset.id === presetId)?.colors ??
    capturePresets.find((preset) => preset.id === "graphite")!.colors;
}

export function capturePresetDirection(presetId: CapturePresetId): CaptureGradientDirection {
  return capturePresets.find((preset) => preset.id === presetId)?.direction ?? "bottom-right";
}

export function capturePresetAssetUrl(presetId: CapturePresetId): string | null {
  const preset = capturePresets.find((candidate) => candidate.id === presetId);
  if (preset?.section !== "wallpapers") return null;
  return embeddedCapturePresetAssets[presetId] ?? `/capture-backgrounds/${presetId}.jpg`;
}

export function capturePresetSwatch(preset: CapturePreset): string {
  if (preset.section === "gradients") {
    const [start, middle, end] = preset.colors;
    const direction = preset.direction?.replace("-", " ") ?? "bottom right";
    return `linear-gradient(to ${direction}, ${start} 0%, ${middle} 52%, ${end} 100%)`;
  }
  return `url('${capturePresetAssetUrl(preset.id)}') center / cover no-repeat`;
}

export function isCapturePlainColor(color: string): boolean {
  return capturePlainColors.some((candidate) => candidate.toLowerCase() === color.toLowerCase());
}
