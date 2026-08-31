import type { CapturePresetId } from "./model.ts";

export type CapturePreset = {
  colors: readonly [string, string, string];
  id: CapturePresetId;
};

export const capturePresets: readonly CapturePreset[] = [
  { colors: ["#d7eee8", "#71b6ae", "#2f6870"], id: "sea" },
  { colors: ["#f4cfaa", "#c67b5c", "#65443e"], id: "canyon" },
  { colors: ["#eef2f1", "#aab9b6", "#74817e"], id: "mist" },
  { colors: ["#d8d7b6", "#779175", "#3f5c59"], id: "highland" },
  { colors: ["#b8e8e8", "#4d9bb1", "#24526f"], id: "ocean" },
  { colors: ["#f0f1f3", "#c6c9ce", "#90959d"], id: "silver" },
  { colors: ["#d6ecff", "#8ab8f7", "#3268a8"], id: "azure" },
  { colors: ["#d8dcff", "#7a78cf", "#38346e"], id: "indigo" },
  { colors: ["#ffcf98", "#ea785b", "#722f45"], id: "ember" },
  { colors: ["#57606f", "#2d3440", "#15191f"], id: "graphite" },
] as const;

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
