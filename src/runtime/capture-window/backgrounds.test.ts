import { describe, expect, test } from "bun:test";
import { captureBackgroundImageUrl } from "./backgrounds.ts";

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
  });
});
