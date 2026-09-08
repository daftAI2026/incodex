/**
 * [INPUT]: 依赖受控 CDP 主机提供的目录元数据、PNG 缩略图和 PNG/JPEG 编辑图片响应
 * [OUTPUT]: 提供系统壁纸 adapter、请求队列与有界响应关联
 * [POS]: capture-window 的系统资源传输边界，不接受文件路径或远程 URL
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import type { SystemWallpaperAdapter, SystemWallpaperEntry } from "./system-wallpapers.ts";

export type SystemWallpaperRequest =
  | { id: string; kind: "list" | "restore" }
  | { id: string; kind: "load"; wallpaperId: string };
type Pending = {
  kind: "list" | "restore" | "load";
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
};
const PNG_PREFIX = "data:image/png;base64,";
const MAX_IMAGE_LENGTH = 48 * 1024 * 1024;

export function createSystemWallpaperBridge(
  createId = () => `wallpaper-${crypto.randomUUID()}`,
  timeoutMs = 240_000,
): SystemWallpaperAdapter & {
  takeRequest: () => SystemWallpaperRequest | null;
  resolve: (response: unknown) => boolean;
} {
  const pending = new Map<string, Pending>();
  const queue: SystemWallpaperRequest[] = [];
  function request(kind: "list" | "restore" | "load", wallpaperId?: string): Promise<unknown> {
    if (pending.size >= 4) return Promise.reject(new Error("system wallpaper request busy"));
    const id = createId();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error("system wallpaper request timed out"));
      }, timeoutMs);
      pending.set(id, { kind, resolve, reject, timer });
      queue.push(kind !== "load" ? { id, kind } : { id, kind, wallpaperId: wallpaperId ?? "" });
    });
  }
  return {
    list: () => request("list") as Promise<SystemWallpaperEntry[]>,
    restore: () => request("restore") as Promise<SystemWallpaperEntry[]>,
    load: (id) => /^[a-zA-Z0-9-]{1,128}$/.test(id)
      ? request("load", id) as Promise<string>
      : Promise.reject(new Error("invalid system wallpaper id")),
    takeRequest: () => {
      while (queue.length) {
        const request = queue.shift();
        if (request && pending.has(request.id)) return request;
      }
      return null;
    },
    resolve: (response) => {
      if (!isRecord(response) || typeof response.id !== "string") return false;
      const entry = pending.get(response.id);
      if (!entry) return false;
      pending.delete(response.id);
      clearTimeout(entry.timer);
      if (response.ok !== true) {
        entry.reject(new Error("system wallpaper host failed"));
      } else if (entry.kind !== "load" && validEntries(response.entries)) {
        entry.resolve(response.entries);
      } else if (entry.kind === "load" && validOriginal(response.dataUrl)) {
        entry.resolve(response.dataUrl);
      } else {
        entry.reject(new Error("invalid system wallpaper payload"));
      }
      return true;
    },
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
function validPng(value: unknown): value is string {
  return typeof value === "string" && value.length > PNG_PREFIX.length &&
    value.length <= MAX_IMAGE_LENGTH && value.startsWith(PNG_PREFIX);
}
function validOriginal(value: unknown): value is string {
  return validPng(value) || (typeof value === "string" &&
    value.length <= MAX_IMAGE_LENGTH && value.startsWith("data:image/jpeg;base64,") &&
    value.length > "data:image/jpeg;base64,".length);
}
function validEntries(value: unknown): value is SystemWallpaperEntry[] {
  if (!Array.isArray(value) || value.length > 128) return false;
  const ids = new Set<string>();
  return value.every((entry) => {
    if (!isRecord(entry) || typeof entry.id !== "string" ||
      !/^[a-zA-Z0-9-]{1,128}$/.test(entry.id) || ids.has(entry.id) ||
      typeof entry.name !== "string" || entry.name.length > 256 ||
      !(validPng(entry.thumbnail) || (entry.thumbnail === "" && entry.loadStatus === "idle" && /^system-wallpaper-(theme|landscape)$/.test(entry.id))) || entry.thumbnail.length > 256 * 1024) return false;
    ids.add(entry.id);
    return true;
  });
}
