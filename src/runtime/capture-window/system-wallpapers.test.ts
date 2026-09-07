/**
 * [INPUT]: 依赖 system-wallpapers.ts 的本机壁纸适配器生命周期，依赖 view.ts 的系统壁纸展示契约
 * [OUTPUT]: 为系统壁纸的不可用、加载、失败、空列表、异步竞态与选中态提供回归证明
 * [POS]: capture-window 系统壁纸边界测试，阻止 UI 假装拥有系统资源或绕过编辑器状态机
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import {
  createSystemWallpaperController,
  createSystemWallpaperEditorActions,
  type SystemWallpaperAdapter,
  type SystemWallpaperEntry,
} from "./system-wallpapers.ts";
import { captureWindowCopy } from "./copy.ts";
import { createCaptureWindowState } from "./model.ts";
import { captureWindowTemplate } from "./view.ts";

const SOURCE = { height: 800, scaleFactor: 2, width: 1200 } as const;
const ENTRY: SystemWallpaperEntry = {
  id: "system:sonoma-1",
  name: "Sonoma",
  thumbnail: "data:image/png;base64,sonoma-thumb",
};

function deferred<T>(): {
  promise: Promise<T>;
  reject: (error: unknown) => void;
  resolve: (value: T) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

function adapterWith(
  overrides: Partial<SystemWallpaperAdapter> = {},
): SystemWallpaperAdapter {
  return {
    list: async () => [ENTRY],
    load: async (id) => `data:image/png;base64,${id}`,
    ...overrides,
  };
}

describe("system wallpaper adapter lifecycle", () => {
  test("is explicitly unavailable without an injected adapter", async () => {
    const controller = createSystemWallpaperController(undefined);

    expect(controller.getState()).toEqual({
      entries: [],
      status: "unavailable",
    });
    expect(await controller.ensureLoaded()).toEqual([]);
    expect(controller.getState().status).toBe("unavailable");
  });

  test("loads the local catalog once and exposes loading and ready states", async () => {
    const pending = deferred<SystemWallpaperEntry[]>();
    let listCalls = 0;
    const controller = createSystemWallpaperController(adapterWith({
      list: () => {
        listCalls += 1;
        return pending.promise;
      },
    }));

    const loading = controller.ensureLoaded();
    expect(controller.getState()).toEqual({ entries: [], status: "loading" });

    pending.resolve([ENTRY]);
    await expect(loading).resolves.toEqual([ENTRY]);
    expect(controller.getState()).toEqual({ entries: [ENTRY], status: "ready" });
    await expect(controller.ensureLoaded()).resolves.toEqual([ENTRY]);
    expect(listCalls).toBe(1);
  });

  test("keeps adapter failures visible and does not invent an empty success", async () => {
    const failure = new Error("system bridge unavailable");
    const controller = createSystemWallpaperController(adapterWith({
      list: async () => { throw failure; },
    }));

    await expect(controller.ensureLoaded()).rejects.toBe(failure);
    expect(controller.getState().status).toBe("error");
    expect(controller.getState().entries).toEqual([]);
  });

  test("ignores an older image selection when a newer selection wins", async () => {
    const first = deferred<string>();
    const second = deferred<string>();
    const controller = createSystemWallpaperController(adapterWith({
      list: async () => [
        ENTRY,
        { id: "system:ventura-1", name: "Ventura", thumbnail: "ventura-thumb" },
      ],
      load: (id) => id === ENTRY.id ? first.promise : second.promise,
    }));
    await controller.ensureLoaded();

    const staleSelection = controller.select(ENTRY.id);
    const currentSelection = controller.select("system:ventura-1");
    second.resolve("data:image/png;base64,ventura");
    await expect(currentSelection).resolves.toEqual({
      dataUrl: "data:image/png;base64,ventura",
      id: "system:ventura-1",
    });
    first.resolve("data:image/png;base64,sonoma");
    await expect(staleSelection).resolves.toBeNull();
  });

  test("does not publish async completions after destruction", async () => {
    const pending = deferred<SystemWallpaperEntry[]>();
    let changes = 0;
    const controller = createSystemWallpaperController(adapterWith({
      list: () => pending.promise,
    }), () => { changes += 1; });

    const loading = controller.ensureLoaded();
    changes = 0;
    controller.destroy();
    pending.resolve([ENTRY]);

    await expect(loading).resolves.toEqual([ENTRY]);
    expect(controller.getState()).toEqual({ entries: [], status: "loading" });
    expect(changes).toBe(0);
  });

  test("does not start a directory request after destruction", async () => {
    let listCalls = 0;
    const controller = createSystemWallpaperController(adapterWith({
      list: async () => {
        listCalls += 1;
        return [ENTRY];
      },
    }));

    controller.destroy();
    await expect(controller.ensureLoaded()).resolves.toEqual([]);
    expect(listCalls).toBe(0);
  });

  test("does not list implicitly when a selection arrives before the catalog action", async () => {
    let listCalls = 0;
    const controller = createSystemWallpaperController(adapterWith({
      list: async () => {
        listCalls += 1;
        return [ENTRY];
      },
    }));

    await expect(controller.select(ENTRY.id)).resolves.toBeNull();
    expect(listCalls).toBe(0);
  });

  test("does not apply an older A after the user selects B and A again", async () => {
    const firstA = deferred<HTMLImageElement>();
    const latestA = deferred<HTMLImageElement>();
    const firstDecodeStarted = deferred<void>();
    let aDecodes = 0;
    const applied: string[] = [];
    const controller = createSystemWallpaperController(adapterWith({
      list: async () => [
        ENTRY,
        { id: "system:ventura-1", name: "Ventura", thumbnail: "ventura-thumb" },
      ],
      load: async (id) => `data:image/png;base64,${id}`,
    }));
    await controller.ensureLoaded();
    const actions = createSystemWallpaperEditorActions(controller, {
      apply: ({ id }) => applied.push(id),
      decode: async (dataUrl) => {
        if (dataUrl.includes(ENTRY.id)) {
          aDecodes += 1;
          if (aDecodes === 1) {
            firstDecodeStarted.resolve();
            return firstA.promise;
          }
          return latestA.promise;
        }
        return {} as HTMLImageElement;
      },
      isAlive: () => true,
      onError: () => { throw new Error("unexpected decode failure"); },
      remember: () => undefined,
    });

    const oldA = actions.select(ENTRY.id);
    await firstDecodeStarted.promise;
    const b = actions.select("system:ventura-1");
    const currentA = actions.select(ENTRY.id);
    firstA.resolve({} as HTMLImageElement);
    await oldA;
    await b;
    expect(applied).toEqual([]);
    latestA.resolve({} as HTMLImageElement);
    await currentA;
    expect(applied).toEqual([ENTRY.id]);
  });
});

describe("system wallpaper capture UI", () => {
  test("renders a sibling system-wallpapers group with a disabled unavailable action", () => {
    const state = createCaptureWindowState(SOURCE);
    const markup = captureWindowTemplate(state, captureWindowCopy("en-US"), {
      systemWallpapers: { entries: [], status: "unavailable" },
    });

    expect(markup).toContain('data-background-section="wallpapers"');
    expect(markup).toContain('data-background-section="system-wallpapers"');
    expect(markup).toContain('data-action="load-system-wallpapers"');
    expect(markup).toContain('disabled');
  });

  test("renders loading, error, empty, thumbnail, and model-owned selected states", () => {
    const loading = captureWindowTemplate(
      createCaptureWindowState(SOURCE),
      captureWindowCopy("en-US"),
      { systemWallpapers: { entries: [], status: "loading" } },
    );
    expect(loading).toContain("Loading system wallpapers");

    const error = captureWindowTemplate(
      createCaptureWindowState(SOURCE),
      captureWindowCopy("en-US"),
      { systemWallpapers: { entries: [], status: "error" } },
    );
    expect(error).toContain("Unable to load system wallpapers");

    const empty = captureWindowTemplate(
      createCaptureWindowState(SOURCE),
      captureWindowCopy("en-US"),
      { systemWallpapers: { entries: [], status: "ready" } },
    );
    expect(empty).toContain("No system wallpapers available");

    const selectedState = {
      ...createCaptureWindowState(SOURCE),
      background: {
        dataUrl: "data:image/png;base64,sonoma",
        kind: "wallpaper" as const,
        systemId: ENTRY.id,
      },
    };
    const selected = captureWindowTemplate(selectedState, captureWindowCopy("en-US"), {
      systemWallpapers: { entries: [ENTRY], status: "ready" },
    });
    expect(selected).toContain(ENTRY.thumbnail);
    expect(selected).toContain(`data-system-wallpaper="${ENTRY.id}"`);
    expect(selected).toContain('aria-pressed="true"');
  });

  test("escapes native names and thumbnails before placing them in markup", () => {
    const entry = {
      id: "system-safe",
      name: "<Sonoma & friends>",
      thumbnail: "data:image/png;base64,a&b\"c",
    };
    const markup = captureWindowTemplate(createCaptureWindowState(SOURCE), captureWindowCopy("en-US"), {
      systemWallpapers: { entries: [entry], status: "ready" },
    });

    expect(markup).toContain("&lt;Sonoma &amp; friends&gt;");
    expect(markup).toContain("data:image/png;base64,a&amp;b&quot;c");
    expect(markup).not.toContain(entry.name);
    expect(markup).not.toContain(entry.thumbnail);
  });
});
