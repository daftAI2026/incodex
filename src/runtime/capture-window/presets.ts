/**
 * [INPUT]: 依赖 model.ts 的 CapturePresetId，依赖构建期注入或 preview 静态路由提供背景图片
 * [OUTPUT]: 对外提供按 Gradients 与 Wallpapers 分层的预设目录、颜色采样与资源解析
 * [POS]: capture-window 的背景预设真相源，把展示分组与具体资源绑定隔离在编辑状态之外
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
  id: CapturePresetId;
  section: "gradients" | "wallpapers";
};

export const capturePresets: readonly CapturePreset[] = [
  { colors: ["#d7eee8", "#71b6ae", "#2f6870"], id: "sea", section: "wallpapers" },
  { colors: ["#f4cfaa", "#c67b5c", "#65443e"], id: "canyon", section: "wallpapers" },
  { colors: ["#eef2f1", "#aab9b6", "#74817e"], id: "mist", section: "wallpapers" },
  { colors: ["#d8d7b6", "#779175", "#3f5c59"], id: "highland", section: "wallpapers" },
  { colors: ["#b8e8e8", "#4d9bb1", "#24526f"], id: "ocean", section: "wallpapers" },
  { colors: ["#f0f1f3", "#c6c9ce", "#90959d"], id: "silver", section: "gradients" },
  { colors: ["#d6ecff", "#8ab8f7", "#3268a8"], id: "azure", section: "gradients" },
  { colors: ["#d8dcff", "#7a78cf", "#38346e"], id: "indigo", section: "gradients" },
  { colors: ["#ffcf98", "#ea785b", "#722f45"], id: "ember", section: "gradients" },
  { colors: ["#57606f", "#2d3440", "#15191f"], id: "graphite", section: "gradients" },
] as const;

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
  return capturePresets.find((preset) => preset.id === presetId)?.colors ?? capturePresets[9].colors;
}

export function capturePresetAssetUrl(presetId: CapturePresetId): string {
  return embeddedCapturePresetAssets[presetId] ?? `/capture-backgrounds/${presetId}.jpg`;
}

export function capturePresetSwatch(preset: CapturePreset): string {
  return `url('${capturePresetAssetUrl(preset.id)}') center / cover no-repeat`;
}
