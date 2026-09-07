/**
 * [INPUT]: 依赖截图状态、合成或偏好模块的公开契约。
 * [OUTPUT]: 验证短边百分比及旧逻辑像素偏好的兼容边界。
 * [POS]: capture-window 百分比迁移回归，保护预览与导出的一致性。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { createCaptureWindowState } from "./model.ts";
import {
  applyCapturePreferences,
  loadCapturePreferences,
  saveCapturePreferences,
  shouldRestoreCurrentWallpaper,
} from "./preferences.ts";

class MemoryStorage {
  readonly values = new Map<string, string>();
  failReads = false;
  failWrites = false;

  getItem(key: string): string | null {
    if (this.failReads) throw new Error("read failed");
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    if (this.failWrites) throw new Error("write failed");
    this.values.set(key, value);
  }
}

const SOURCE = { height: 801, scaleFactor: 2, width: 1200 };

describe("capture window preferences", () => {
  test("loads only the four reference options and normalizes corrupt storage", () => {
    const storage = new MemoryStorage();
    storage.values.set("incodex-window-capture-prefs", JSON.stringify({
      background: { color: "#123456", kind: "color" },
      padding: 999.4,
      privacyEnabled: false,
      shadow: false,
      tool: "redact",
      zoom: 4,
    }));

    expect(loadCapturePreferences(storage)).toEqual({
      background: { color: "#123456", kind: "color" },
      padding: 160,
      paddingUnit: "logical-px",
      privacyEnabled: false,
      shadow: false,
    });

    storage.values.set("incodex-window-capture-prefs", "not json");
    expect(loadCapturePreferences(storage)).toEqual({
      background: { id: "sea", kind: "preset" },
      padding: 8,
      paddingUnit: "percent",
      privacyEnabled: true,
      shadow: true,
    });
  });

  test("applies preferences without restoring transient tools, zoom, or region history", () => {
    const state = createCaptureWindowState(SOURCE);
    const restored = applyCapturePreferences(state, {
      background: { kind: "transparent" },
      padding: 28,
      paddingUnit: "percent",
      privacyEnabled: false,
      shadow: false,
    });

    expect(restored.background).toEqual({ kind: "transparent" });
    expect(restored.padding).toBe(28);
    expect(restored.privacyEnabled).toBe(false);
    expect(restored.shadow).toBe(false);
    expect(restored.tool).toBe("move");
    expect(restored.zoom).toBe(1);
    expect(restored.regions).toEqual([]);
  });

  test("persists no wallpaper bytes and tolerates unavailable storage", () => {
    const storage = new MemoryStorage();
    const state = {
      ...createCaptureWindowState(SOURCE),
      background: { dataUrl: "data:image/png;base64,large", kind: "wallpaper" } as const,
      padding: 32,
      privacyEnabled: false,
    };

    saveCapturePreferences(storage, state);
    expect(storage.values.get("incodex-window-capture-prefs")).toBe(
      '{"background":{"kind":"wallpaper"},"padding":32,"paddingUnit":"percent","privacyEnabled":false,"shadow":true}',
    );

    storage.failReads = true;
    expect(() => loadCapturePreferences(storage)).not.toThrow();
    storage.failReads = false;
    storage.failWrites = true;
    expect(() => saveCapturePreferences(storage, state)).not.toThrow();
  });
});


test("restoration preserves an explicit different background and remembers only current source ID", () => {
  const storage = new MemoryStorage();
  expect(shouldRestoreCurrentWallpaper(storage)).toBe(true);
  const state = createCaptureWindowState(SOURCE);
  saveCapturePreferences(storage, state);
  expect(shouldRestoreCurrentWallpaper(storage)).toBe(false);
  saveCapturePreferences(storage, { ...state, background: { kind: "wallpaper", systemId: "system-wallpaper-current", dataUrl: "data:image/jpeg;base64,private" } });
  expect(shouldRestoreCurrentWallpaper(storage)).toBe(true);
  expect(storage.values.get("incodex-window-capture-prefs")).not.toContain("private");
});

test("migrates legacy logical padding only with source dimensions, then saves percent", () => {
  const storage = new MemoryStorage();
  storage.values.set("incodex-window-capture-prefs", JSON.stringify({ padding: 64, shadow: false }));
  const source = { width: 1200, height: 800, scaleFactor: 2 };
  const restored = applyCapturePreferences(createCaptureWindowState(source), loadCapturePreferences(storage));
  expect(restored.padding).toBe(16);
  expect(restored.shadow).toBe(false);
  saveCapturePreferences(storage, restored);
  const larger = applyCapturePreferences(createCaptureWindowState({ width: 2400, height: 1600, scaleFactor: 2 }), loadCapturePreferences(storage));
  expect(larger.padding).toBe(16);
});
