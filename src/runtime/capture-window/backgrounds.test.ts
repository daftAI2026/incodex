/**
 * [INPUT]: 依赖 backgrounds.ts 的背景 URL 与共享图像存储契约
 * [OUTPUT]: 证明可渲染背景筛选及共享解码 Promise 的回归测试
 * [POS]: capture-window 图像管线测试，约束系统壁纸复用同一解码缓存而不自建副本
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { createCaptureBackgroundImageStore, captureBackgroundImageUrl } from "./backgrounds.ts";

describe("capture background image resolution", () => {
  test("resolves only raster-backed backgrounds", () => {
    expect(captureBackgroundImageUrl({ id: "sea", kind: "preset" })).toBe(
      "/capture-backgrounds/sea.jpg",
    );
    expect(
      captureBackgroundImageUrl({ dataUrl: "data:image/png;base64,wallpaper", kind: "wallpaper" }),
    ).toBe("data:image/png;base64,wallpaper");
    expect(captureBackgroundImageUrl({ color: "#2B3440", kind: "color" })).toBeNull();
    expect(captureBackgroundImageUrl({ kind: "transparent" })).toBeNull();
    expect(captureBackgroundImageUrl({ id: "silver", kind: "preset" })).toBeNull();
  });

  test("shares concurrent image decoding and keeps the resolved image", async () => {
    let loads = 0;
    const image = {} as HTMLImageElement;
    const store = createCaptureBackgroundImageStore(async () => {
      loads += 1;
      await Promise.resolve();
      return image;
    });
    const background = {
      dataUrl: "data:image/jpeg;base64,wallpaper",
      kind: "wallpaper" as const,
      systemId: "system:sonoma-1",
    };

    const [first, second] = await Promise.all([
      store.resolve(background),
      store.resolve(background),
    ]);

    expect(first).toBe(image);
    expect(second).toBe(image);
    expect(await store.resolve(background)).toBe(image);
    expect(loads).toBe(1);
  });
});
