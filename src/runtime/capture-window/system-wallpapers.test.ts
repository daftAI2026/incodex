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
import { createCaptureBackgroundImageStore } from "./backgrounds.ts";
import { captureWindowCopy } from "./copy.ts";
import { createCaptureWindowState } from "./model.ts";
import { captureWindowTemplate } from "./view.ts";

const SOURCE = { height: 800, scaleFactor: 2, width: 1200 } as const;
const ENTRY: SystemWallpaperEntry = {
  id: "system:sonoma-1",
  name: "Sonoma",
  thumbnail: "data:image/png;base64,sonoma-thumb",
};
const CURRENT_ENTRY: SystemWallpaperEntry = {
  id: "system-wallpaper-current",
  name: "DefaultDesktop.heic",
  thumbnail: "data:image/png;base64,current-thumb",
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
  test("loads and applies only the current desktop through the shared pipeline", async () => {
    const loaded: string[] = [];
    const applied: string[] = [];
    const controller = createSystemWallpaperController(adapterWith({
      list: async () => [CURRENT_ENTRY],
      load: async (id) => {
        loaded.push(id);
        return `data:image/png;base64,${id}`;
      },
    }));
    const actions = createSystemWallpaperEditorActions(controller, {
      apply: ({ id }) => applied.push(id),
      isAlive: () => true,
      onError: () => { throw new Error("unexpected current wallpaper failure"); },
      resolve: async () => ({} as HTMLImageElement),
    });

    await actions.loadCurrent();

    expect(loaded).toEqual([CURRENT_ENTRY.id]);
    expect(applied).toEqual([CURRENT_ENTRY.id]);
  });

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

  test("deduplicates same-id originals and evicts the oldest entry after two cached ids", async () => {
    const entries = [
      ENTRY,
      { id: "system:ventura-1", name: "Ventura", thumbnail: "ventura-thumb" },
      { id: "system:sequoia-1", name: "Sequoia", thumbnail: "sequoia-thumb" },
    ];
    const loads: string[] = [];
    const controller = createSystemWallpaperController(adapterWith({
      list: async () => entries,
      load: async (id) => {
        loads.push(id);
        return `data:image/png;base64,${id}`;
      },
    }));
    await controller.ensureLoaded();

    await controller.select(ENTRY.id);
    await controller.select("system:ventura-1");
    await controller.select(ENTRY.id);
    await controller.select("system:sequoia-1");
    await controller.select("system:ventura-1");

    expect(loads).toEqual([
      ENTRY.id,
      "system:ventura-1",
      "system:sequoia-1",
      "system:ventura-1",
    ]);
  });

  test("shares an in-flight original request and removes a failed promise before retry", async () => {
    const failure = new Error("conversion failed");
    const pending = deferred<string>();
    let loadCalls = 0;
    const controller = createSystemWallpaperController(adapterWith({
      load: () => {
        loadCalls += 1;
        return loadCalls === 1 ? pending.promise : Promise.resolve("data:image/png;base64,retry");
      },
    }));
    await controller.ensureLoaded();

    const first = controller.select(ENTRY.id);
    const duplicate = controller.select(ENTRY.id);
    expect(loadCalls).toBe(1);
    const firstFailure = first.catch((error) => error);
    const duplicateFailure = duplicate.catch((error) => error);
    pending.reject(failure);
    expect(await firstFailure).toBe(failure);
    expect(await duplicateFailure).toBe(failure);

    await controller.ensureLoaded();
    await expect(controller.select(ENTRY.id)).resolves.toEqual({
      dataUrl: "data:image/png;base64,retry",
      id: ENTRY.id,
    });
    expect(loadCalls).toBe(2);
  });

  test("resolves selections through the shared background image store", async () => {
    const decodedUrls: string[] = [];
    const image = {} as HTMLImageElement;
    const backgroundImages = createCaptureBackgroundImageStore(async (url) => {
      decodedUrls.push(url);
      return image;
    });
    const controller = createSystemWallpaperController(adapterWith());
    await controller.ensureLoaded();
    const applied: string[] = [];
    let errors = 0;
    const pipeline = {
      apply: ({ id }: { id: string }) => applied.push(id),
      decode: async () => { throw new Error("legacy decode path"); },
      isAlive: () => true,
      onError: () => { errors += 1; },
      remember: () => { throw new Error("legacy remember path"); },
      resolve: ({ dataUrl, id }: { dataUrl: string; id: string }) =>
        backgroundImages.resolve({ dataUrl, kind: "wallpaper", systemId: id }),
    };
    const actions = createSystemWallpaperEditorActions(controller, pipeline);

    await actions.select(ENTRY.id);
    await actions.select(ENTRY.id);

    expect(errors).toBe(0);
    expect(decodedUrls).toEqual([`data:image/png;base64,${ENTRY.id}`]);
    expect(applied).toEqual([ENTRY.id, ENTRY.id]);
  });

  test("does not apply an older A after the user selects B and A again", async () => {
    const firstA = deferred<HTMLImageElement>();
    const latestA = deferred<HTMLImageElement>();
    const firstResolveStarted = deferred<void>();
    let aResolves = 0;
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
      resolve: async ({ dataUrl }) => {
        if (dataUrl.includes(ENTRY.id)) {
          aResolves += 1;
          if (aResolves === 1) {
            firstResolveStarted.resolve();
            return firstA.promise;
          }
          return latestA.promise;
        }
        return {} as HTMLImageElement;
      },
      isAlive: () => true,
      onError: () => { throw new Error("unexpected resolve failure"); },
    });

    const oldA = actions.select(ENTRY.id);
    await firstResolveStarted.promise;
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
  test("puts one current-wallpaper action in the existing wallpaper group", () => {
    const markup = captureWindowTemplate(createCaptureWindowState(SOURCE), captureWindowCopy("en-US"), {
      systemWallpapers: { entries: [], status: "ready" },
    });

    expect(markup).toContain('data-background-section="wallpapers"');
    expect(markup).toContain('data-action="load-current-wallpaper"');
    expect(markup).toContain("Use current wallpaper");
    expect(markup).not.toContain('data-background-section="system-wallpapers"');
    expect(markup).not.toContain('data-action="load-system-wallpapers"');
    expect(markup).not.toContain("data-system-wallpaper=");
  });

  test("shows current-wallpaper loading without pretending it is selected", () => {
    const state = createCaptureWindowState(SOURCE);
    const markup = captureWindowTemplate(state, captureWindowCopy("en-US"), {
      systemWallpapers: { entries: [], status: "loading" },
    });

    expect(markup).toContain('data-action="load-current-wallpaper"');
    expect(markup).toContain('aria-busy="true"');
    expect(markup).toContain("disabled");
    expect(markup).toContain('aria-pressed="false"');
  });

  test("disables the current action when the adapter is unavailable", () => {
    const state = createCaptureWindowState(SOURCE);
    const markup = captureWindowTemplate(state, captureWindowCopy("en-US"), {
      systemWallpapers: { entries: [], status: "unavailable" },
    });

    expect(markup).toContain('data-background-section="wallpapers"');
    expect(markup).toContain('data-action="load-current-wallpaper"');
    expect(markup).toContain('disabled');
    expect(markup).toContain("The current wallpaper is unavailable");
  });

  test("renders current-wallpaper loading and errors without a catalog", () => {
    const loading = captureWindowTemplate(
      createCaptureWindowState(SOURCE),
      captureWindowCopy("en-US"),
      { systemWallpapers: { entries: [], status: "loading" } },
    );
    expect(loading).toContain("Loading current wallpaper");

    const error = captureWindowTemplate(
      createCaptureWindowState(SOURCE),
      captureWindowCopy("en-US"),
      { systemWallpapers: { entries: [], status: "error" } },
    );
    expect(error).toContain("Unable to load the current wallpaper");

    const selected = captureWindowTemplate({
      ...createCaptureWindowState(SOURCE),
      background: {
        dataUrl: "data:image/jpeg;base64,current",
        kind: "wallpaper" as const,
        systemId: CURRENT_ENTRY.id,
      },
    }, captureWindowCopy("en-US"), {
      systemWallpapers: { entries: [], status: "ready" },
    });
    expect(selected).toContain('data-action="load-current-wallpaper"');
    expect(selected).toContain('aria-pressed="true"');
    expect(selected).not.toContain("data-system-wallpaper=");
  });

  test("does not render native catalog entries in current-only mode", () => {
    const markup = captureWindowTemplate(
      createCaptureWindowState(SOURCE),
      captureWindowCopy("en-US"),
      { systemWallpapers: { entries: [ENTRY], status: "ready" } },
    );
    expect(markup).not.toContain(ENTRY.name);
    expect(markup).not.toContain(ENTRY.thumbnail);
    expect(markup).not.toContain('data-system-wallpaper=');
  });
});

test("current wallpaper discovery cannot override a newer background choice", async () => {
  const pending = deferred<SystemWallpaperEntry[]>();
  let loads = 0;
  const controller = createSystemWallpaperController({
    list: () => pending.promise,
    load: async () => { loads += 1; return "data:image/jpeg;base64,current"; },
  });
  const applied: string[] = [];
  const actions = createSystemWallpaperEditorActions(controller, {
    apply: (selection) => applied.push(selection.id),
    resolve: async () => ({} as HTMLImageElement),
    isAlive: () => true,
    onError: () => { throw new Error("unexpected failure"); },
  });
  const request = actions.loadCurrent();
  controller.invalidateSelection();
  pending.resolve([CURRENT_ENTRY]);
  await request;
  expect(loads).toBe(0);
  expect(applied).toEqual([]);
});


test("available current wallpaper precedes upload and hides the fetch action", () => {
  const markup = captureWindowTemplate(createCaptureWindowState(SOURCE), captureWindowCopy("en-US"), {
    systemWallpapers: { entries: [CURRENT_ENTRY], status: "ready" },
  });
  expect(markup).toContain('data-system-wallpaper="system-wallpaper-current"');
  expect(markup.indexOf('data-system-wallpaper="')).toBeLessThan(markup.indexOf("data-background-wallpaper"));
  expect(markup).not.toContain('data-action="load-current-wallpaper"');
});

test("restores the remembered entry without selecting a background", async () => {
  let loads = 0;
  const controller = createSystemWallpaperController({
    list: async () => [], load: async () => { loads += 1; return ""; },
    restore: async () => [CURRENT_ENTRY],
  });
  await controller.restore();
  expect(controller.getState().entries).toEqual([CURRENT_ENTRY]);
  expect(loads).toBe(0);
  expect(controller.isSelectionCurrent(CURRENT_ENTRY.id)).toBe(false);
});


test("fetching current wallpaper immediately applies it, not just its thumbnail", async () => {
  const applied: string[] = [];
  const controller = createSystemWallpaperController({ list: async () => [CURRENT_ENTRY], load: async () => "data:image/jpeg;base64,current" });
  const actions = createSystemWallpaperEditorActions(controller, {
    apply: ({ id }) => applied.push(id), resolve: async () => ({} as HTMLImageElement), isAlive: () => true,
    onError: () => { throw new Error("unexpected error"); },
  });
  await actions.loadCurrent();
  expect(applied).toEqual([CURRENT_ENTRY.id]);
});

test("restored current wallpaper is applied unless a newer user selection wins", async () => {
  const applied: string[] = [];
  const pending = deferred<SystemWallpaperEntry[]>();
  const controller = createSystemWallpaperController({ list: async () => [], restore: () => pending.promise, load: async () => "data:image/jpeg;base64,current" });
  const actions = createSystemWallpaperEditorActions(controller, {
    apply: ({ id }) => applied.push(id), resolve: async () => ({} as HTMLImageElement), isAlive: () => true, onError: () => {},
  });
  const restoring = actions.restoreCurrent();
  controller.invalidateSelection();
  pending.resolve([CURRENT_ENTRY]);
  await restoring;
  expect(applied).toEqual([]);
  await actions.restoreCurrent();
  expect(applied).toEqual([CURRENT_ENTRY.id]);
});
