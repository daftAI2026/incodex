/**
 * [INPUT]: 依赖 presets.ts 暴露的背景预设、分组与资源解析契约
 * [OUTPUT]: 为背景顺序、CleanShot 语义分层与本地图片资产提供回归证明
 * [POS]: capture-window 的背景模型合同测试，阻止展示层把渐变与壁纸重新压平成同类选项
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  capturePresetAssetUrl,
  capturePresetSections,
  capturePresets,
  capturePresetSwatch,
} from "./presets.ts";

const root = join(import.meta.dir, "../../..");

describe("capture background presets", () => {
  test("keeps the observed photo and color-field order", () => {
    expect(capturePresets.map((preset) => preset.id)).toEqual([
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
    ]);
  });

  test("exposes gradients and wallpapers as separate CleanShot-style sections", () => {
    expect(
      capturePresetSections.map((section) => ({
        id: section.id,
        presets: section.presets.map((preset) => preset.id),
      })),
    ).toEqual([
      {
        id: "gradients",
        presets: ["silver", "azure", "indigo", "ember", "graphite"],
      },
      {
        id: "wallpapers",
        presets: ["sea", "canyon", "mist", "highland", "ocean"],
      },
    ]);
  });

  test("uses local 2560 by 1600 raster assets instead of synthetic gradients", () => {
    for (const preset of capturePresets) {
      const assetUrl = capturePresetAssetUrl(preset.id);
      const bytes = readFileSync(join(root, "assets", assetUrl.replace(/^\//, "")));

      expect(jpegSize(bytes)).toEqual({ height: 1600, width: 2560 });
      expect(capturePresetSwatch(preset)).toBe(
        `url('/${assetUrl.replace(/^\//, "")}') center / cover no-repeat`,
      );
    }
  });
});

function jpegSize(bytes: Buffer): { height: number; width: number } {
  if (bytes[0] !== 0xff || bytes[1] !== 0xd8) throw new Error("Expected a JPEG image");
  let offset = 2;
  while (offset + 8 < bytes.length) {
    if (bytes[offset] !== 0xff) {
      offset += 1;
      continue;
    }
    const marker = bytes[offset + 1];
    const length = bytes.readUInt16BE(offset + 2);
    if (marker >= 0xc0 && marker <= 0xc3) {
      return {
        height: bytes.readUInt16BE(offset + 5),
        width: bytes.readUInt16BE(offset + 7),
      };
    }
    offset += 2 + length;
  }
  throw new Error("JPEG dimensions were not found");
}
