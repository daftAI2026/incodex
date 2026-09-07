/**
 * [INPUT]: 依赖 model.ts 的背景模型、presets.ts 的资源 URL 及 wallpaper.ts 的图像装载能力
 * [OUTPUT]: 提供背景 URL 解析与按 URL 去重的图像存储、hydrate、read、remember、resolve 接口
 * [POS]: capture-window 的共享图像管线，统一自定义、预设与系统壁纸的解码结果
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import type { CaptureBackground } from "./model.ts";
import { capturePresetAssetUrl } from "./presets.ts";
import { loadCaptureImage } from "./wallpaper.ts";

export type CaptureBackgroundImageStore = {
  hydrate: (
    background: CaptureBackground,
    onLoad: () => void,
    onError: (error: unknown) => void,
  ) => void;
  read: (background: CaptureBackground) => HTMLImageElement | null;
  remember: (url: string, image: HTMLImageElement) => void;
  resolve: (background: CaptureBackground) => Promise<HTMLImageElement | null>;
};

export function captureBackgroundImageUrl(background: CaptureBackground): string | null {
  if (background.kind === "preset") return capturePresetAssetUrl(background.id);
  if (background.kind === "wallpaper") return background.dataUrl;
  return null;
}

export function createCaptureBackgroundImageStore(
  loadImage: (url: string) => Promise<HTMLImageElement> = loadCaptureImage,
): CaptureBackgroundImageStore {
  const images = new Map<string, HTMLImageElement>();
  const loads = new Map<string, Promise<HTMLImageElement>>();

  function read(background: CaptureBackground): HTMLImageElement | null {
    const url = captureBackgroundImageUrl(background);
    return url ? images.get(url) ?? null : null;
  }

  async function resolve(background: CaptureBackground): Promise<HTMLImageElement | null> {
    const url = captureBackgroundImageUrl(background);
    if (!url) return null;
    const cached = images.get(url);
    if (cached) return cached;

    let pending = loads.get(url);
    if (!pending) {
      pending = loadImage(url).then((image) => {
        images.set(url, image);
        loads.delete(url);
        return image;
      }, (error: unknown) => {
        loads.delete(url);
        throw error;
      });
      loads.set(url, pending);
    }
    return pending;
  }

  function hydrate(
    background: CaptureBackground,
    onLoad: () => void,
    onError: (error: unknown) => void,
  ): void {
    if (!captureBackgroundImageUrl(background) || read(background)) return;
    void resolve(background).then(onLoad, onError);
  }

  return {
    hydrate,
    read,
    remember: (url, image) => images.set(url, image),
    resolve,
  };
}
