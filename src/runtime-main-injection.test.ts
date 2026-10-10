import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { runInNewContext } from "node:vm";
import { createRendererUpdateCoordinator } from "./runtime/incodex-runtime-load.cts";

const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8").replaceAll(
  "\r\n",
  "\n",
);

function hookWindowSource(): string {
  const start = main.indexOf("function hookWindow(");
  const end = main.indexOf("\nasync function attachElectron()", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  return main.slice(start, end);
}

describe("Electron UI injection reporting", () => {
  test("raising the session never reveals hidden prewarmed windows", () => {
    const calls: string[] = [];
    const window = (name: string, visible: boolean, minimized = false) => ({
      isVisible: () => visible,
      isMinimized: () => minimized,
      restore: () => calls.push(`${name}:restore`),
      show: () => calls.push(`${name}:show`),
      focus: () => {},
      moveTop: () => {},
    });
    const windows = [window("primary", true), window("prewarm", false), window("minimized", false, true)];
    const start = main.indexOf("function raiseOurWindows()");
    const end = main.indexOf("\nasync function raiseExistingIncognito()", start);
    runInNewContext(`${main.slice(start, end)}\nraiseOurWindows()`, {
      require: () => ({}), process: { platform: "test", pid: 1 },
      mainWindows: () => windows, hideAuxiliaryWindows: () => {}, raisePid: () => {},
      shownWindows: new WeakSet(),
    });
    expect(calls).toEqual(["primary:show", "minimized:restore", "minimized:show"]);
  });

  test("an existing session can raise a previously shown window after the host hides it", () => {
    let visible = true;
    let shows = 0;
    const primary = {
      isVisible: () => visible, isMinimized: () => false,
      show: () => { visible = true; shows++; }, focus: () => {},
    };
    const start = main.indexOf("function raiseOurWindows()");
    const end = main.indexOf("\nasync function raiseExistingIncognito()", start);
    const context = {
      require: () => ({}), process: { platform: "test", pid: 1 },
      mainWindows: () => [primary], hideAuxiliaryWindows: () => {}, raisePid: () => {},
      shownWindows: new WeakSet(),
    };
    const source = `${main.slice(start, end)}\nraiseOurWindows()`;
    runInNewContext(source, context);
    visible = false;
    runInNewContext(source, context);
    expect(visible).toBe(true);
    expect(shows).toBe(2);
    expect(main).toContain('shownWindows.add(win)');
  });

  test("native menu launches inherit geometry only from a real main window", () => {
    const start = main.indexOf("function captureSourceBounds(");
    const end = main.indexOf("\nfunction readSourceBounds()", start);
    const capture = main.slice(start, end);

    expect(start).toBeGreaterThanOrEqual(0);
    expect(end).toBeGreaterThan(start);
    expect(capture).toContain("mainWindows(electron)");
    expect(capture).not.toContain("BrowserWindow.getAllWindows()[0]");
  });

  test("uses the authorized IPC sender window before any focus fallback", () => {
    const start = main.indexOf("function captureSourceBounds(");
    const end = main.indexOf("\nfunction readSourceBounds()", start);
    const capture = main.slice(start, end);
    const senderWindow = {
      isDestroyed: () => false,
      getBounds: () => ({ x: 385, y: 107, width: 1311, height: 873 }),
    };
    const prewarm = {
      isDestroyed: () => false,
      getBounds: () => ({ x: 0, y: 0, width: 960, height: 720 }),
    };

    const bounds = runInNewContext(`${capture}\ncaptureSourceBounds(senderWindow)`, {
      require: () => ({ BrowserWindow: { getFocusedWindow: () => null } }),
      mainWindows: () => [prewarm],
      isAuxiliaryWindow: () => false,
      senderWindow,
    });

    expect(bounds).toBe("385,107,1311,873");
  });

  test("uses the visible main window when an accessibility launch has no focused window", () => {
    const start = main.indexOf("function captureSourceBounds(");
    const end = main.indexOf("\nfunction readSourceBounds()", start);
    const capture = main.slice(start, end);
    const prewarm = {
      isDestroyed: () => false,
      isVisible: () => false,
      isMinimized: () => false,
      getBounds: () => ({ x: 0, y: 0, width: 960, height: 720 }),
    };
    const primary = {
      isDestroyed: () => false,
      isVisible: () => true,
      isMinimized: () => false,
      getBounds: () => ({ x: 410, y: 114, width: 1398, height: 930 }),
    };
    const electron = {
      BrowserWindow: {
        getFocusedWindow: () => null,
      },
    };

    const bounds = runInNewContext(`${capture}\ncaptureSourceBounds()`, {
      require: () => electron,
      mainWindows: () => [prewarm, primary],
      isAuxiliaryWindow: () => false,
    });

    expect(bounds).toBe("410,114,1398,930");
  });

  test("lets an authorized renderer configure the macOS Dock decorator", () => {
    expect(main).toContain('require("./incodex-dock-menu.cjs")');
    expect(readFileSync(join(import.meta.dir, "runtime/incodex-main-actions.cts"), "utf8")).toContain('action === "configure-dock-menu"');
    expect(main).toContain("dockMenuController?.configure(label)");
  });

  test("passes the authorized renderer window into the incognito launch", () => {
    const start = main.indexOf('electron.ipcMain.handle("incodex-action"');
    const end = main.indexOf("\n  });", start);
    const handler = main.slice(start, end);

    expect(handler).toContain("BrowserWindow.fromWebContents(event.sender)");
    expect(handler).toContain("actions.handle(payload?.action, payload, sourceWindow)");
  });

  test("snapshots launch geometry before asynchronous owner and session work", () => {
    const start = main.indexOf("function launchIncognito(");
    const end = main.indexOf("\nfunction runtimeOwnedSessionEnv", start);
    let geometry = "385,107,1311,873";
    let deferred: (() => string) | undefined;
    runInNewContext(`${main.slice(start, end)}\nlaunchIncognito({})`, {
      windowsPlatform: null,
      launchHolder: {},
      instance: { singleFlight: (_holder: unknown, launch: () => string) => { deferred = launch; } },
      captureSourceBounds: () => geometry,
      launchIncognitoOnce: (bounds: string) => bounds,
    });
    geometry = "0,0,960,720";
    expect(deferred?.()).toBe("385,107,1311,873");
  });

  test("propagates renderer accessibility only to diagnostic incognito launches", () => {
    const start = main.indexOf("function incognitoLaunchArguments(");
    const end = main.indexOf("\nasync function launchIncognitoOnce", start);
    expect(start).toBeGreaterThanOrEqual(0);
    expect(end).toBeGreaterThan(start);
    const helper = main.slice(start, end);
    const context = { process: { argv: ["ChatGPT", "--force-renderer-accessibility"] } };

    const defaultArgs = runInNewContext(
      `${helper}\nincognitoLaunchArguments("/session/chromium", ["ChatGPT"])`,
      context,
    );
    const diagnosticArgs = runInNewContext(
      `${helper}\nincognitoLaunchArguments("/session/chromium")`,
      context,
    );

    expect(defaultArgs).toEqual(["--user-data-dir=/session/chromium", "codex://new?mode=codex"]);
    expect(diagnosticArgs).toEqual([
      "--force-renderer-accessibility",
      "--user-data-dir=/session/chromium",
      "codex://new?mode=codex",
    ]);
  });

  test("keeps macOS recovery timing while Windows rechecks asynchronous UI readiness", () => {
    const hook = hookWindowSource();

    expect(hook).toContain('win.webContents.on("dom-ready", () => run(false))');
    expect(hook).toContain('win.webContents.on("did-finish-load", () => run(true))');
    expect(hook).toContain("run(false)");
    expect(hook.match(/run\(true\)/g)).toHaveLength(1);
    expect(hook).toContain("if (windowsPlatform && isIncognito())");
    expect(hook).toContain("windowsPlatform.observeRuntimeUiReadiness(");
    expect(hook).toContain("reportInjectionProbe(win, false)");
  });

  test("does not silently swallow executeJavaScript rejection", () => {
    const hook = hookWindowSource();

    expect(hook).not.toContain(".catch(() => {})");
    expect(hook).toMatch(/\.catch\(\(error\) => reportInjectionError\(error\)\)/);
  });

  test("observes only incognito content windows for session closure", () => {
    const created = main.indexOf('electron.app.on("browser-window-created"');
    const officialReturn = main.indexOf("if (!isIncognito()) return;", created);
    expect(created).toBeGreaterThanOrEqual(0);
    expect(officialReturn).toBeGreaterThan(created);

    const beforeOfficialReturn = main.slice(created, officialReturn);
    expect(main).toContain('require("./incodex-window-lifecycle.cjs")');
    expect(main).toContain("isIncognito()\n    ? windowLifecycle.createIncognitoWindowLifecycle(");
    expect(main).toContain("createIncognitoWindowLifecycle(finishIncognito)");
    expect(beforeOfficialReturn).toContain("incognitoWindowLifecycle?.observe(win)");
    expect(beforeOfficialReturn).not.toContain("open.isVisible()");
  });

  test("keeps every incognito exit path idempotent on both platforms", () => {
    const start = main.indexOf("function finishIncognito(code)");
    const end = main.indexOf("\n  registerMainActionHandler", start);
    const finish = main.slice(start, end);

    expect(start).toBeGreaterThanOrEqual(0);
    expect(end).toBeGreaterThan(start);
    expect(finish).toContain("if (incognitoExitStarted) return;");
    expect(finish).toContain("incognitoExitStarted = true;");
    expect(finish).not.toContain("windowsPlatform && incognitoExitStarted");
  });

  test("defers circle-x cleanup until the official close is accepted", () => {
    const actions = readFileSync(join(import.meta.dir, "runtime/incodex-main-actions.cts"), "utf8");
    const quitStart = actions.indexOf('if (action === "quit")');
    const quitEnd = actions.indexOf('\n    return { ok: false, code: "UNKNOWN_ACTION"', quitStart);
    const quit = actions.slice(quitStart, quitEnd);

    expect(quitStart).toBeGreaterThanOrEqual(0);
    expect(quitEnd).toBeGreaterThan(quitStart);
    expect(quit).toContain("deps.quit()");
    expect(main).toContain("quit: () => electron.app.quit()");
    expect(quit).not.toContain("burnIncognitoHome()");
    expect(quit).not.toContain("clearPid(");
    expect(main).not.toContain('electron.app.on("before-quit"');
  });

  test("does not quit the installed Windows main process on the user's behalf", () => {
    const attach = main.slice(main.indexOf("async function attachElectron()"));

    expect(attach).not.toContain("windowsPlatform.listenForNormalExit(");
    expect(attach).not.toContain("INCODEX_WINDOWS_REGISTRATION_ID");
    expect(attach).not.toMatch(
      /if \(windowsPlatform && !isIncognito\(\)\)[\s\S]*electron\.app\.quit\(\)/,
    );
  });
});


function hotWindowFixture(platform = "darwin", privateWindow = false) {
  const start = main.indexOf("async function injectRendererCandidate(");
  const source = start < 0 ? hookWindowSource() : main.slice(start, main.indexOf("\nasync function attachElectron()", start));
  const listeners = new Map<string, Array<() => void>>(), appEvents = new Map<string, () => void>();
  const executed: string[] = [];
  let watchCallback: ((event: string, name: string | null) => void) | undefined, closed = false, authorized = true;
  const a = { key: "A", id: "ui-A", runtimeRoot: "/test/runtime", source: "window.__incodexRendererGeneration={...window.__incodexRendererRequest,restartRequired:false};" };
  let candidate = a, prepares = 0;
  const win = { id: 7, isDestroyed: () => false, webContents: {
    isDestroyed: () => false, session: {}, getURL: () => "app://-/index.html",
    on(name: string, callback: () => void) { listeners.set(name, [...listeners.get(name) ?? [], callback]); },
    executeJavaScript(text: string) { executed.push(text); return Promise.resolve(runInNewContext(text, { window: {} })); },
  } };
  const loader = { readRendererGeneration: () => a, createRendererUpdateCoordinator,
    prepareRendererUpdate: () => { prepares++; return candidate; }, rendererUpdateStillSelected: () => true };
  const context = {
    process: { platform, pid: 123 }, instance: { processIdentity: () => ({ processStartIdentity: "process-start-A" }) }, __dirname: "/test/runtime/releases/A", windowsPlatform: null,
    fs: { watch(_root: string, _options: unknown, callback: typeof watchCallback) {
      watchCallback = callback; return { on() {}, close() { closed = true; } };
    } },
    require: () => loader, isIncognito: () => privateWindow,
    mainWindows: () => [win], hookedWindows: new WeakSet<object>(),
    isAuxiliaryWindow: () => false, rememberWindow() {}, hookPreload() {},
    ipcGuard: { bindWindowIdentity: () => authorized, urlAllowed: () => authorized },
    allowedWindows: new WeakSet(), trustedOrigins: new Set(),
    readLocaleOverride: () => "en", codexModeReadiness: { observe() {} },
    reportInjectionProbe: async () => {}, reportInjectionError() {}, logLaunch(_event?: string, _data?: any) {},
    electron: { app: { once(name: string, callback: () => void) { appEvents.set(name, callback); } } },
  };
  const api: any = runInNewContext(`${source}; ({ hookWindow, ${main.includes("function createMacRendererUpdater(") ? "createMacRendererUpdater," : ""} ${main.includes("function injectRendererCandidate(") ? "injectRendererCandidate," : ""} })`, context);
  return { api, context, win, a, executed, listeners, appEvents,
    watch: (name: string | null) => watchCallback?.("rename", name), closed: () => closed,
    prepares: () => prepares, select(value: typeof a) { candidate = value; }, block() { authorized = false; } };
}

describe("macOS renderer Runtime integration", () => {
  test("navigation uses the current provider and window hooks register once", async () => {
    const f = hotWindowFixture(); let current = f.a;
    f.api.hookWindow(f.win, () => current);
    f.api.hookWindow(f.win, () => current);
    current = { ...f.a, key: "B", id: "ui-B" };
    for (const callback of f.listeners.get("did-finish-load") ?? []) callback();
    await Promise.resolve();
    expect(f.listeners.get("dom-ready")).toHaveLength(1);
    expect(f.listeners.get("did-finish-load")).toHaveLength(1);
    expect(f.executed.at(-1)).toContain("ui-B");
  });
  test("watches publication events, coalesces them, and releases its watcher at exit", async () => {
    const f = hotWindowFixture(); f.api.hookWindow(f.win, () => f.a);
    expect(typeof f.api.createMacRendererUpdater).toBe("function");
    const updater = f.api.createMacRendererUpdater(f.context.electron);
    await updater.refresh(); const before = f.prepares();
    f.watch("unrelated.json"); await Promise.resolve(); expect(f.prepares()).toBe(before);
    f.select({ ...f.a, key: "B", id: "ui-B" });
    f.watch("current.json"); f.watch("current.json"); await updater.refresh();
    expect(updater.status().active.key).toBe("B");
    expect(f.executed.filter(value => value.includes("ui-B"))).toHaveLength(1);
    f.appEvents.get("will-quit")!(); expect(f.closed()).toBe(true);
    f.select({ ...f.a, key: "C", id: "ui-C" }); f.watch("current.json"); await updater.refresh();
    expect(updater.status().active.key).toBe("B");
  });
  test("diagnostics distinguish startup controller, main hooks, publication and actual window ACK", async () => {
    const f = hotWindowFixture(), logs: Array<{ event: string; data: any }> = [];
    f.context.logLaunch = (event: string, data: any) => { logs.push({ event, data }); };
    const updater = f.api.createMacRendererUpdater(f.context.electron);
    f.api.hookWindow(f.win, (win: unknown) => updater.sourceForWindow(win), updater.recordWindow);
    await updater.refresh();
    f.select({ ...f.a, key: "B", id: "ui-B" }); await updater.refresh();
    const data = logs.filter(item => item.event === "renderer-runtime-update").at(-1)?.data;
    expect(data).toMatchObject({ schemaVersion: 1, pid: 123, processStartIdentity: "process-start-A",
      phase: "active", published: { generation: "B" }, controller: { generation: "A" },
      main: { generation: "B" }, renderers: [{ windowId: 7, generation: "B", ui: "ui-B", state: "acknowledged" }] });
    expect(JSON.stringify(data)).not.toContain("source");
  });
  test("an acknowledged window keeps the candidate across navigation while another ACK is pending", async () => {
    const f = hotWindowFixture();
    let release!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; });
    const second = { ...f.win, webContents: { ...f.win.webContents, on() {},
      async executeJavaScript(text: string) {
        if (text.includes("ui-B")) await gate;
        return runInNewContext(text, { window: {} });
      } } };
    f.context.mainWindows = () => [f.win, second];
    const updater = f.api.createMacRendererUpdater(f.context.electron);
    const source = (win: unknown) => updater.sourceForWindow?.(win) ?? updater.status().active;
    f.api.hookWindow(f.win, source); f.api.hookWindow(second, source);
    await updater.refresh();
    f.select({ ...f.a, key: "B", id: "ui-B" });
    const switching = updater.refresh();
    for (let i = 0; i < 12; i++) await Promise.resolve();
    expect(updater.status().active.key).toBe("A");
    for (const callback of f.listeners.get("did-finish-load") ?? []) callback();
    await Promise.resolve();
    const navigation = f.executed.at(-1);
    release(); await switching;
    expect(navigation).toContain("ui-B");
    expect(updater.status().active.key).toBe("B");
  });
  test("does not start automatic updates in private or Windows processes", () => {
    for (const [platform, privateWindow] of [["darwin", true], ["win32", false]] as const) {
      const f = hotWindowFixture(platform, privateWindow);
      expect(typeof f.api.createMacRendererUpdater).toBe("function");
      expect(f.api.createMacRendererUpdater(f.context.electron)).toBeNull();
      expect(f.prepares()).toBe(0);
    }
  });
  test("a pinned string injector uses its embedded identity instead of an incomplete request", async () => {
    const f = hotWindowFixture();
    await f.api.injectRendererCandidate(f.win, "window.__observedRequest=window.__incodexRendererRequest;");
    const window: any = {};
    runInNewContext(f.executed[0]!, { window });
    expect(window.__observedRequest).toBeUndefined();
  });
  test("revalidates the existing window authorization before applying a candidate", async () => {
    const f = hotWindowFixture(); f.block();
    expect(typeof f.api.injectRendererCandidate).toBe("function");
    expect(await f.api.injectRendererCandidate(f.win, f.a)).toBe(false);
    expect(f.executed).toEqual([]);
  });
});

describe("main action hooks", () => {
  test("pure factory preserves action semantics and captures only stable dependencies", async () => {
    const file = join(import.meta.dir, "runtime/incodex-main-actions.cts");
    const { createMainActions } = await import(file);
    let privateWindow = false, quits = 0; const launches: unknown[] = [], labels: unknown[] = [];
    const actions = createMainActions({ isIncognito: () => privateWindow,
      configureDockMenu: (label: unknown) => { labels.push(label); return true; },
      configureStatusMenu: async () => false,
      launchIncognito: async (win: unknown) => { launches.push(win); return { ok: true }; }, quit: () => { quits++; } });
    const win = {};
    expect(await actions.handle("open", {}, win)).toMatchObject({ ok: true, code: "OK" });
    expect(launches).toEqual([win]);
    expect(await actions.handle("configure-dock-menu", { label: "Private" })).toEqual({ ok: true, code: "OK" });
    expect(labels).toEqual(["Private"]);
    expect(await actions.handle("configure-status-menu", {})).toEqual({ ok: false, code: "UNAVAILABLE" });
    expect(await actions.handle("quit", {})).toMatchObject({ ok: false, code: "NOT_INCOGNITO" });
    privateWindow = true;
    expect(await actions.open()).toMatchObject({ ok: false, code: "ALREADY_INCOGNITO" });
    expect(launches).toHaveLength(1);
    expect(await actions.handle("quit", {})).toEqual({ ok: true, code: "OK" });
    expect(quits).toBe(1);
    expect(await actions.handle("unrecognized", {})).toEqual({ ok: false, code: "UNKNOWN_ACTION" });
  });
  test("one authorized IPC handler captures A for an in-flight request and dispatches later requests to B", async () => {
    const start = main.indexOf("function registerMainActionHandler(");
    expect(start).toBeGreaterThanOrEqual(0);
    const source = main.slice(start, main.indexOf("\nfunction createMacRendererUpdater(", start));
    const callbacks: any[] = [], senderWindow = {}; let authorized = true, reads = 0, release!: () => void;
    const pending = new Promise<void>(resolve => { release = resolve; });
    const seen: unknown[] = [];
    const a = { async handle(_action: string, _payload: unknown, win: unknown) { seen.push(win); await pending; return { ok: true, code: "A" }; } };
    const b = { async handle() { return { ok: true, code: "B" }; } }; let current = a;
    runInNewContext(source+';registerMainActionHandler(electron, () => { reads++; return current(); });', {
      electron: { ipcMain: { handle(_channel: string, callback: unknown) { callbacks.push(callback); } }, BrowserWindow: { fromWebContents: () => senderWindow } },
      authorizeEvent: () => ({ ok: authorized, code: "DENIED" }),
      ipcGuard: { actionResponse: (requestId: string, response: unknown) => ({ requestId, ...response as object }) },
      current: () => current, get reads() { return reads; }, set reads(value: number) { reads = value; },
    });
    const old = callbacks[0]({ sender: {} }, { action: "open", requestId: "old" });
    current = b as typeof a;
    expect(await callbacks[0]({ sender: {} }, { action: "open", requestId: "new" })).toEqual({ requestId: "new", ok: true, code: "B" });
    authorized = false;
    expect(await callbacks[0]({}, { requestId: "denied" })).toMatchObject({ ok: false, code: "DENIED" });
    expect(reads).toBe(2);
    release(); expect(await old).toEqual({ requestId: "old", ok: true, code: "A" });
    expect(seen).toEqual([senderWindow]); expect(callbacks).toHaveLength(1);
  });
  test("actions commit with the renderer transaction, retaining A on rejected activation", async () => {
    const f = hotWindowFixture();
    const a = { protocol: 1, handle() {}, open() {} }, b = { ...a };
    const loader: any = f.context.require(); loader.loadMainActions = () => b;
    const updater = f.api.createMacRendererUpdater(f.context.electron, {}, a);
    f.api.hookWindow(f.win, () => updater.sourceForWindow(f.win)); await updater.refresh();
    expect(updater.status().active.mainActions).toBe(a);
    f.select({ ...f.a, key: "B", id: "ui-B" });
    f.win.webContents.executeJavaScript = async () => false as any;
    await updater.refresh(); expect(updater.status().active.mainActions).toBe(a);
    // A failure to ACK must not publish B's action routing.
    expect(updater.status().phase).toBe("rollback-failed");
  });
});
