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
