/**
 * [INPUT]: 依赖主机注入的 SystemWallpaperAdapter，本机目录只提供缩略图与 id，原图按需读取
 * [OUTPUT]: 提供系统壁纸类型、目录控制器、有界原图 Promise 缓存、异步选择编排与背景控件 DOM 同步
 * [POS]: capture-window 的系统资源边界，隔离 CDP/文件桥接与编辑器唯一状态源
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import type { CaptureWindowState } from "./model.ts";

export type SystemWallpaperEntry = {
  id: string;
  name: string;
  thumbnail: string;
};

export const SYSTEM_WALLPAPER_CURRENT_ID = "system-wallpaper-current";

export type SystemWallpaperAdapter = {
  list: () => Promise<SystemWallpaperEntry[]>;
  restore?: () => Promise<SystemWallpaperEntry[]>;
  load: (id: string) => Promise<string>;
};

export type SystemWallpaperStatus = "error" | "idle" | "loading" | "ready" | "unavailable";

export type SystemWallpaperState = {
  entries: readonly SystemWallpaperEntry[];
  status: SystemWallpaperStatus;
};

export type SystemWallpaperSelection = {
  dataUrl: string;
  id: string;
};

export type SystemWallpaperController = {
  destroy: () => void;
  restore: () => Promise<void>;
  ensureLoaded: () => Promise<readonly SystemWallpaperEntry[]>;
  getState: () => SystemWallpaperState;
  getSelectionRevision: () => number;
  invalidateSelection: () => void;
  isSelectionCurrent: (selection: SystemWallpaperSelection | string, revision?: number) => boolean;
  select: (id: string) => Promise<SystemWallpaperSelection | null>;
};

const SYSTEM_WALLPAPER_IMAGE_CACHE_LIMIT = 2;

export function createSystemWallpaperController(
  adapter: SystemWallpaperAdapter | undefined,
  onChange?: (state: SystemWallpaperState) => void,
): SystemWallpaperController {
  let state: SystemWallpaperState = {
    entries: [],
    status: adapter ? "idle" : "unavailable",
  };
  let listPromise: Promise<readonly SystemWallpaperEntry[]> | null = null;
  let selectionRevision = 0;
  // 这里只是过期异步请求的守卫，选中态始终由 CaptureWindowState.background 派生。
  let pendingSelectionId: string | null = null;
  const selectionRevisions = new WeakMap<SystemWallpaperSelection, number>();
  const originalLoads = new Map<string, Promise<string>>();
  let destroyed = false;

  function publish(next: SystemWallpaperState): void {
    if (destroyed) return;
    state = next;
    onChange?.(state);
  }

  function ensureLoaded(): Promise<readonly SystemWallpaperEntry[]> {
    if (destroyed) return Promise.resolve([]);
    if (!adapter) return Promise.resolve([]);
    if (state.status === "ready") return Promise.resolve(state.entries);
    if (state.status === "loading" && listPromise) return listPromise;
    publish({ entries: [], status: "loading" });
    const pending = adapter.list().then((entries) => {
      if (!destroyed) publish({ entries, status: "ready" });
      return entries;
    }, (error: unknown) => {
      if (!destroyed) publish({ entries: [], status: "error" });
      throw error;
    });
    listPromise = pending;
    return pending;
  }

  async function select(id: string): Promise<SystemWallpaperSelection | null> {
    if (!adapter || destroyed) return null;
    const revision = ++selectionRevision;
    pendingSelectionId = id;
    if (state.status !== "ready" || !state.entries.some((entry) => entry.id === id)) return null;
    const dataUrl = await loadOriginal(id);
    if (!isCurrent(id, revision)) return null;
    const selection = { dataUrl, id };
    selectionRevisions.set(selection, revision);
    return selection;
  }

  function loadOriginal(id: string): Promise<string> {
    const cached = originalLoads.get(id);
    if (cached) {
      originalLoads.delete(id);
      originalLoads.set(id, cached);
      return cached;
    }
    let source: Promise<string>;
    try {
      source = Promise.resolve(adapter!.load(id));
    } catch (error) {
      source = Promise.reject(error);
    }
    const pending = source.then(
      (dataUrl) => dataUrl,
      (error: unknown) => {
        if (originalLoads.get(id) === pending) originalLoads.delete(id);
        publish({ entries: [], status: "error" });
        throw error;
      },
    );
    originalLoads.set(id, pending);
    while (originalLoads.size > SYSTEM_WALLPAPER_IMAGE_CACHE_LIMIT) {
      const oldest = originalLoads.keys().next().value as string | undefined;
      if (oldest === undefined) break;
      originalLoads.delete(oldest);
    }
    return pending;
  }

  function isCurrent(id: string, revision: number): boolean {
    return !destroyed && selectionRevision === revision && pendingSelectionId === id;
  }

  return {
    destroy: () => {
      destroyed = true;
      selectionRevision += 1;
      pendingSelectionId = null;
      originalLoads.clear();
      listPromise = null;
    },
    restore: async () => {
      if (!adapter?.restore || destroyed || state.status !== "idle") return;
      publish({ entries: [], status: "loading" });
      try {
        const entries = await adapter.restore();
        if (!destroyed) publish({ entries, status: entries.length ? "ready" : "idle" });
      } catch {
        if (!destroyed) publish({ entries: [], status: "error" });
      }
    },
    ensureLoaded,
    getState: () => state,
    getSelectionRevision: () => selectionRevision,
    invalidateSelection: () => {
      selectionRevision += 1;
      pendingSelectionId = null;
    },
    isSelectionCurrent: (selection, revision) => {
      if (destroyed) return false;
      if (typeof selection === "string") {
        return pendingSelectionId === selection &&
          (revision === undefined || selectionRevision === revision);
      }
      return selectionRevisions.get(selection) === selectionRevision &&
        pendingSelectionId === selection.id;
    },
    select,
  };
}

export type SystemWallpaperEditorActions = {
  loadCurrent: () => Promise<void>;
  restoreCurrent: (apply?: boolean) => Promise<void>;
  select: (id: string) => Promise<void>;
};

export type SystemWallpaperSelectionPipeline = {
  apply: (selection: SystemWallpaperSelection) => void;
  isAlive: () => boolean;
  onError: () => void;
  onUnavailable?: () => void;
  resolve: (selection: SystemWallpaperSelection) => Promise<HTMLImageElement | null>;
};

export function createSystemWallpaperEditorActions(
  controller: SystemWallpaperController,
  pipeline: SystemWallpaperSelectionPipeline,
): SystemWallpaperEditorActions {
  async function select(id: string): Promise<void> {
    const pending = controller.select(id);
    const revision = controller.getSelectionRevision();
    await pending.then(async (selection) => {
      if (!selection || !pipeline.isAlive() || !controller.isSelectionCurrent(id, revision)) return;
      try {
        const image = await pipeline.resolve(selection);
        if (!image) {
          if (pipeline.isAlive() && controller.isSelectionCurrent(selection)) pipeline.onError();
          return;
        }
        if (!pipeline.isAlive() || !controller.isSelectionCurrent(selection)) return;
        pipeline.apply(selection);
      } catch {
        if (pipeline.isAlive() && controller.isSelectionCurrent(selection)) pipeline.onError();
      }
    }, () => {
      if (pipeline.isAlive() && controller.isSelectionCurrent(id, revision)) pipeline.onError();
    });
  }

  return {
    restoreCurrent: async (apply = true) => {
      const revision = controller.getSelectionRevision();
      await controller.restore();
      if (!apply || !pipeline.isAlive() || revision !== controller.getSelectionRevision()) return;
      if (controller.getState().entries.some((entry) => entry.id === SYSTEM_WALLPAPER_CURRENT_ID)) {
        await select(SYSTEM_WALLPAPER_CURRENT_ID);
      }
    },
    loadCurrent: async () => {
      controller.invalidateSelection();
      const revision = controller.getSelectionRevision();
      let entries: readonly SystemWallpaperEntry[];
      try {
        entries = await controller.ensureLoaded();
      } catch {
        if (pipeline.isAlive()) pipeline.onError();
        return;
      }
      if (!pipeline.isAlive() || controller.getSelectionRevision() !== revision) return;
      const current = entries.find((entry) => entry.id === SYSTEM_WALLPAPER_CURRENT_ID);
      if (!current) {
        pipeline.onUnavailable?.();
        return;
      }
      await select(current.id);
    },
    select,
  };
}

export function wireSystemWallpaperActions(
  root: HTMLElement,
  actions: SystemWallpaperEditorActions,
): void {
  root.querySelector<HTMLElement>("[data-system-wallpaper]")?.addEventListener("click", () => {
    void actions.select(SYSTEM_WALLPAPER_CURRENT_ID);
  });
  const current = root.querySelector<HTMLButtonElement>("[data-action='load-current-wallpaper']");
  current?.addEventListener("click", () => {
    current.setAttribute("aria-busy", "true");
    void actions.loadCurrent().catch(() => undefined).finally(() => {
      current.removeAttribute("aria-busy");
    });
  });
}

export function syncSystemWallpaperControls(
  root: HTMLElement,
  state: CaptureWindowState,
): void {
  const current = root.querySelector<HTMLElement>("[data-system-wallpaper], [data-action='load-current-wallpaper']");
  if (!current) return;
  const selected = state.background.kind === "wallpaper" &&
    state.background.systemId === SYSTEM_WALLPAPER_CURRENT_ID;
  current.dataset.selected = String(selected);
  current.setAttribute("aria-pressed", String(selected));
}
