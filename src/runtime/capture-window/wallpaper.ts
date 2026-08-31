const MAX_WALLPAPER_BYTES = 32 * 1024 * 1024;
const WALLPAPER_TYPES = new Set(["image/jpeg", "image/png", "image/webp"]);

export function isCaptureWallpaperFile(file: File): boolean {
  return WALLPAPER_TYPES.has(file.type) && file.size <= MAX_WALLPAPER_BYTES;
}

export function readCaptureWallpaperFile(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => resolve(String(reader.result)));
    reader.addEventListener("error", () => reject(reader.error));
    reader.readAsDataURL(file);
  });
}

export function loadCaptureImage(source: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.addEventListener("load", () => resolve(image));
    image.addEventListener("error", () => reject(new Error("Unable to load image")));
    image.src = source;
  });
}
