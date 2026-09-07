/**
 * [INPUT]: 依赖系统壁纸 bridge 的有界请求/响应契约
 * [OUTPUT]: 验证目录、原图请求隔离与不可信资源拒绝
 * [POS]: capture-window 的主机传输回归，不依赖真实系统目录
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { createSystemWallpaperBridge } from "./system-wallpaper-bridge.ts";

test("system wallpaper list and load use typed requests without file paths", async () => {
  let sequence = 0;
  const bridge = createSystemWallpaperBridge(() => `wallpaper-${++sequence}`);
  const listing = bridge.list();
  expect(bridge.takeRequest()).toEqual({ id: "wallpaper-1", kind: "list" });
  const entries = [{ id: "system-0", name: "Sonoma", thumbnail: "data:image/png;base64,cG5n" }];
  bridge.resolve({ id: "wallpaper-1", ok: true, entries });
  expect(await listing).toEqual(entries);
  const loading = bridge.load("system-0");
  expect(bridge.takeRequest()).toEqual({ id: "wallpaper-2", kind: "load", wallpaperId: "system-0" });
  bridge.resolve({ id: "wallpaper-2", ok: true, dataUrl: "data:image/png;base64,cG5n" });
  expect(await loading).toBe("data:image/png;base64,cG5n");
  expect(bridge.takeRequest()).toBeNull();
});

test("system wallpaper bridge rejects remote thumbnails and wrong response shapes", async () => {
  const bridge = createSystemWallpaperBridge(() => "wallpaper-1");
  const listing = bridge.list();
  bridge.takeRequest();
  expect(bridge.resolve({ id: "unknown", ok: true, entries: [] })).toBe(false);
  bridge.resolve({ id: "wallpaper-1", ok: true, entries: [
    { id: "system-0", name: "Remote", thumbnail: "https://example.com/image.png" },
  ] });
  await expect(listing).rejects.toThrow("invalid system wallpaper payload");
});

test("system wallpaper requests time out and late replies cannot revive them", async () => {
  const bridge = createSystemWallpaperBridge(() => "timeout", 5);
  const listing = bridge.list();
  await expect(listing).rejects.toThrow("system wallpaper request timed out");
  expect(bridge.takeRequest()).toBeNull();
  expect(bridge.resolve({ id: "timeout", ok: true, entries: [] })).toBe(false);
});

test("system wallpaper bridge refuses paths before they enter the host queue", async () => {
  const bridge = createSystemWallpaperBridge(() => "path", 5);
  await expect(bridge.load("/Users/private/image.png")).rejects.toThrow("invalid system wallpaper id");
  expect(bridge.takeRequest()).toBeNull();
});

test("system wallpaper originals accept JPEG without forcing photo assets into huge PNGs", async () => {
  const bridge = createSystemWallpaperBridge(() => "jpeg");
  const pending = bridge.load("system-0");
  bridge.takeRequest();
  bridge.resolve({ id: "jpeg", ok: true, dataUrl: "data:image/jpeg;base64,/9j/" });
  expect(await pending).toBe("data:image/jpeg;base64,/9j/");
});
