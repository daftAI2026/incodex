/**
 * [INPUT]: 依赖主 Runtime 源码与合成 Electron/窗口测试环境。
 * [OUTPUT]: 验证代际提交到原生权限续接；ACK 前不唤醒，Skip 不复活。
 * [POS]: 主 Runtime 的组合契约回归，不执行真实 App 安装。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { runInNewContext } from "node:vm";
import { createRendererUpdateCoordinator } from "./runtime/incodex-runtime-load.cts";
import { createMainActions } from "./runtime/incodex-main-actions.cts";

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

  test("does not silently swallow executeJavaScript rejection", async () => {
    const f = hotWindowFixture(), errors: unknown[] = [];
    f.win.webContents.executeJavaScript = () => Promise.reject(new Error("injection failed"));
    f.context.reportInjectionError = error => { errors.push(error); };
    f.api.hookWindow(f.win, f.a);
    for (let i = 0; i < 8; i++) await Promise.resolve();
    expect(errors).toHaveLength(1);
    expect(String(errors[0])).toContain("injection failed");
  });

  test("a later presentation probe failure cannot erase a valid injection ACK", async () => {
    const f = hotWindowFixture(), phases: string[] = [], errors: unknown[] = [];
    f.context.reportInjectionProbe = async () => { throw new Error("page navigated during UI probe"); };
    f.context.reportInjectionError = error => { errors.push(error); };
    f.api.hookWindow(f.win, f.a, (_win: unknown, _value: unknown, phase: string) => { phases.push(phase); });
    for (let i = 0; i < 8; i++) await Promise.resolve();
    phases.length = 0;
    for (const callback of f.listeners.get("did-finish-load") ?? []) callback();
    for (let i = 0; i < 12; i++) await Promise.resolve();
    expect(errors).toHaveLength(1);
    expect(phases).toEqual(["pending", "acknowledged"]);
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

  test("arms seamless update handoff without yielding official main startup", () => {
    const start = main.indexOf("function startRuntime()");
    const end = main.indexOf("\nconst startupGate", start);
    const startup = main.slice(start, end);

    expect(start).toBeGreaterThanOrEqual(0);
    expect(end).toBeGreaterThan(start);
    expect(main).toContain(
      'process.platform === "darwin" ? loadVerifiedRuntimeModule("incodex-macos-update.cjs") : null',
    );
    expect(startup).toContain("macosUpdate.prepareUpdateHandoff({");
    expect(startup).not.toContain("await macosUpdate.prepareUpdateHandoff(");
    expect(startup).not.toContain("async function startRuntime");
    expect(startup).toContain("userRoot: USER_ROOT");
    expect(startup).toContain("execPath: process.execPath");
    expect(startup).toContain("pid: process.pid");
    expect(startup.indexOf("macosUpdate.prepareUpdateHandoff({")).toBeLessThan(
      startup.indexOf("return attachElectron()"),
    );
    expect(main).not.toContain('electron.app.once("before-quit", () => {');
    expect(main).not.toContain("macosUpdate.spawnCoordinator({");
  });
});


describe("native open with an older installed loader", () => {
  const start = main.indexOf("function startRuntime()");
  const end = main.indexOf("\nconst startupGate", start);

  async function runStartup(platform: string, env: Record<string, string>, incognito: boolean) {
    const calls: string[] = [];
    const startup = runInNewContext(`${main.slice(start, end)}\nstartRuntime;`, {
      process: { platform, env, execPath: "synthetic-host", pid: 123 },
      macosUpdate: { prepareUpdateHandoff: () => calls.push("handoff") },
      isIncognito: () => incognito,
      USER_ROOT: "synthetic-root",
      attachElectron: () => { calls.push("attach"); return Promise.resolve(); },
    });
    await startup();
    return calls;
  }

  test("native-owned macOS open never attaches a second Runtime through an older loader", async () => {
    expect(await runStartup("darwin", {
      INCODEX_NATIVE_OPEN: "1", INCODEX_INCOGNITO: "1", INCODEX_CLEANUP_OWNER: "native",
    }, true)).toEqual([]);
  });

  test("installed windows and partial/native-open flags keep the ordinary Runtime path", async () => {
    const complete = { INCODEX_NATIVE_OPEN: "1", INCODEX_INCOGNITO: "1", INCODEX_CLEANUP_OWNER: "native" };
    for (const missing of Object.keys(complete)) {
      const partial = { ...complete }; delete partial[missing as keyof typeof partial];
      expect(await runStartup("darwin", partial, true)).toEqual(["attach"]);
    }
    expect(await runStartup("win32", complete, true)).toEqual(["attach"]);
    expect(await runStartup("darwin", {}, false)).toEqual(["handoff", "attach"]);
    expect(await runStartup("darwin", { ...complete, INCODEX_CLEANUP_OWNER: "runtime" }, true)).toEqual(["attach"]);
  });
});


function hotWindowFixture(platform = "darwin", privateWindow = false) {
  const start = main.indexOf("async function injectRendererCandidate(");
  const source = start < 0 ? hookWindowSource() : main.slice(start, main.indexOf("\nasync function attachElectron()", start));
  const listeners = new Map<string, Array<() => void>>(), appEvents = new Map<string, () => void>();
  const executed: string[] = [];
  let watchCallback: ((event: string, name: string | null) => void) | undefined, closed = false, authorized = true;
  const a = { key: "A", id: "ui-A", runtimeRoot: "/test/runtime", source: "window.__incodexRendererGeneration={...window.__incodexRendererRequest,restartRequired:false};" };
  let candidate: any = a, prepares = 0;
  const win = { id: 7, isDestroyed: () => false, webContents: {
    isDestroyed: () => false, session: {}, getURL: () => "app://-/index.html",
    on(name: string, callback: () => void) { listeners.set(name, [...listeners.get(name) ?? [], callback]); },
    executeJavaScript(text: string) { executed.push(text); return Promise.resolve(runInNewContext(text, { window: {} })); },
  } };
  const loader = { readRendererGeneration: () => a, createRendererUpdateCoordinator,
    prepareRendererUpdate: () => { prepares++; if (candidate instanceof Error) throw candidate; return candidate; }, rendererUpdateStillSelected: () => true };
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
    reportInjectionProbe: async () => {}, reportInjectionError(_error?: unknown) {}, logLaunch(_event?: string, _data?: any) {},
    electron: { app: { once(name: string, callback: () => void) { appEvents.set(name, callback); } } },
  };
  const api: any = runInNewContext(`${source}; ({ hookWindow, ${main.includes("function createMacRendererUpdater(") ? "createMacRendererUpdater," : ""} ${main.includes("function injectRendererCandidate(") ? "injectRendererCandidate," : ""} })`, context);
  return { api, context, win, a, executed, listeners, appEvents,
    watch: (name: string | null) => watchCallback?.("rename", name), closed: () => closed,
    prepares: () => prepares, select(value: any) { candidate = value; }, block() { authorized = false; } };
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
  test("a failed full verification reports no verified publication even if its pointer still matches", async () => {
    const f = hotWindowFixture(), logs: any[] = [];
    f.context.logLaunch = (_event: string, data: any) => { logs.push(data); };
    const updater = f.api.createMacRendererUpdater(f.context.electron);
    f.api.hookWindow(f.win, (win: unknown) => updater.sourceForWindow(win), updater.recordWindow);
    await updater.refresh();
    f.select(new Error("asset hash mismatch")); await updater.refresh();
    expect(logs.at(-1)).toMatchObject({ phase: "retained", published: null,
      main: { generation: "A" }, failure: "verification-failed" });
  });
  test("lost ACK and failed rollback never claim that renderer runs the controller's active generation", async () => {
    const f = hotWindowFixture(), logs: any[] = [];
    f.context.logLaunch = (_event: string, data: any) => { logs.push(data); };
    const updater = f.api.createMacRendererUpdater(f.context.electron);
    f.api.hookWindow(f.win, (win: unknown) => updater.sourceForWindow(win), updater.recordWindow);
    await updater.refresh();
    f.win.webContents.executeJavaScript = () => Promise.resolve(undefined);
    f.select({ ...f.a, key: "B", id: "ui-B" }); await updater.refresh();
    expect(logs.at(-1)).toMatchObject({ phase: "rollback-failed", main: { generation: "A" },
      renderers: [{ windowId: 7, generation: null, ui: null, state: "rollback-failed" }] });
    f.select(new Error("corrupt next publication")); await updater.refresh();
    expect(logs.at(-1)).toMatchObject({ phase: "rollback-failed", rollbackFailed: true,
      published: null, failure: "verification-failed",
      renderers: [{ generation: null, state: "rollback-failed" }] });
  });
  for (const staleResult of ["reject", "ack"] as const) {
    test(`a stale injection ${staleResult} cannot replace a newer window acknowledgment`, async () => {
      const f = hotWindowFixture(), logs: any[] = [];
      f.context.logLaunch = (_event: string, data: any) => { logs.push(data); };
      let complete!: () => void, calls = 0, current = f.a;
      const first = new Promise<any>((resolve, reject) => {
        complete = () => staleResult === "reject" ? reject(new Error("old document replaced"))
          : resolve({ protocol: 1, id: "ui-A", restartRequired: false });
      });
      f.win.webContents.executeJavaScript = text => ++calls === 1 ? first
        : Promise.resolve(runInNewContext(text, { window: {} }));
      const updater = f.api.createMacRendererUpdater(f.context.electron);
      f.api.hookWindow(f.win, () => current, updater.recordWindow);
      await updater.refresh();
      current = { ...f.a, key: "B", id: "ui-B" };
      for (const callback of f.listeners.get("did-finish-load") ?? []) callback();
      for (let i = 0; i < 12; i++) await Promise.resolve();
      expect(logs.at(-1).renderers[0]).toMatchObject({ generation: "B", state: "acknowledged" });
      complete();
      for (let i = 0; i < 12; i++) await Promise.resolve();
      expect(logs.at(-1).renderers[0]).toMatchObject({ generation: "B", state: "acknowledged" });
    });
  }
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
  test("preparing an action factory does not invoke any host capability", () => {
    const accesses: PropertyKey[] = [];
    const deps = new Proxy({}, { get(_target, property) {
      accesses.push(property); throw new Error("prepare must not read or invoke host capabilities");
    } });
    const actions = createMainActions(deps);
    expect(actions.protocol).toBe(1);
    expect(Object.isFrozen(actions)).toBe(true);
    expect(accesses).toEqual([]);
  });
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


describe("permission presentation generation", () => {
  function controllerFixture(initial: unknown, choose: Promise<string> | string = "later") {
    const start = main.indexOf("function createAccessibilitySetupController(");
    const source = main.slice(start, main.indexOf("\nfunction sessionBurnExpectation(", start));
    const shown: any[] = [], written: string[] = [];
    let marker: any = { state: "pending", installId: "test-install", appPath: "/Applications/ChatGPT.app", requestedAtMs: 1 };
    let current = { copy: { title: "A", body: "body-A", repair: "repair", later: "later" },
      layoutDirection: "leftToRight", permission: { shouldPrompt: () => true } };
    let choice = choose, captures = 0;
    const controller: any = runInNewContext(source + ";createAccessibilitySetupController(options)", {
      fs: {}, spawn() {}, process: { platform: "darwin" },
      readAccessibilityMarker: () => ({ kind: "ok", marker }),
      accessibilityRequestMatches: () => true,
      accessibilityPathIsDefaultApp: () => true, accessibilityInstallIdIsSafe: () => true,
      accessibilityCopyValue: (copy: any, key: string) => copy?.[key] ?? "",
      writeAccessibilityMarkerState(_fs: unknown, _path: unknown, _app: unknown, _id: unknown, _time: unknown, state: string) {
        marker = { ...marker, state }; written.push(state); return true;
      },
      logLaunch() {},
      options: { appPath: marker.appPath, installId: marker.installId,
        systemPreferences: { isTrustedAccessibilityClient: () => initial },
        capturePresentation() { captures++; return current; },
        createSetupWindow(presentation: any) { shown.push(presentation); return {
          choice: Promise.resolve(choice), onClose() {}, close() {}, setState() {},
        }; }, setInterval: () => ({}), clearInterval() {},
      },
    });
    return { controller, shown, written, captures: () => captures,
      next(value: typeof current) { current = value; marker = { ...marker, state: "pending", requestedAtMs: marker.requestedAtMs + 1 }; choice = "later"; } };
  }

  test("one request retains A while the next request uses B without rebuilding the controller", async () => {
    let finish!: (choice: string) => void;
    const pending = new Promise<string>(resolve => { finish = resolve; });
    const f = controllerFixture(false, pending);
    const aRequest = f.controller.run();
    for (let i = 0; i < 10; i++) await Promise.resolve();
    expect(f.shown).toHaveLength(1);
    expect(f.shown[0].copy.title).toBe("A");
    // Publication does not reconstruct the card or retarget its captured data.
    expect(f.controller.run()).toBe(aRequest);
    finish("later"); await aRequest;
    f.next({ copy: { title: "B", body: "body-B", repair: "repair", later: "later" },
      layoutDirection: "rightToLeft", permission: { shouldPrompt: () => true } });
    await f.controller.run();
    expect(f.shown.map(item => [item.copy.title, item.layoutDirection])).toEqual([
      ["A", "leftToRight"], ["B", "rightToLeft"],
    ]);
    expect(f.captures()).toBe(2);
    expect(f.written).toEqual(["deferred", "deferred"]);
  });

  test("feature rules cannot turn an unavailable host read into missing access or granted", async () => {
    const unknown = controllerFixture(undefined);
    expect(await unknown.controller.run()).toMatchObject({ state: "unknown" });
    expect(unknown.shown).toHaveLength(0);
    expect(unknown.written).toEqual([]);
    const missing = controllerFixture(false);
    expect(await missing.controller.run()).toMatchObject({ state: "deferred" });
    expect(missing.shown).toHaveLength(1); // Positive prompt witness for the same fixture.
  });

  test("copy and direction come from one captured permission feature, even if selection changes", () => {
    const start = main.indexOf("function captureAccessibilityPresentation(");
    expect(start).toBeGreaterThanOrEqual(0);
    const source = main.slice(start, main.indexOf("\nfunction createAccessibilitySetupController(", start));
    let reads = 0;
    const b = { featureProtocol: 1, permission: { resolveCopy: () => ({ title: "B" }), resolveLayoutDirection: () => "rightToLeft" } };
    const a = { featureProtocol: 1, permission: { resolveCopy: () => { current = b; return { title: "A" }; }, resolveLayoutDirection: () => "leftToRight" } };
    let current: any = a;
    const presentation = runInNewContext(source + "\n;captureAccessibilityPresentation(provider, 'en')", {
      provider: () => { reads++; return current; },
    });
    expect(reads).toBe(1);
    expect(presentation.copy.title).toBe("A");
    expect(presentation.layoutDirection).toBe("leftToRight");
    expect(presentation.permission).toBe(a.permission);
    expect(Object.isFrozen(presentation)).toBe(true);
  });

  test("native permission handoff retries only after a newly committed active generation", async () => {
    const f = hotWindowFixture(), activated: string[] = [];
    const updater = f.api.createMacRendererUpdater(f.context.electron, undefined, undefined,
      (generation: any) => { activated.push(generation.key); });
    f.api.hookWindow(f.win, (win: unknown) => updater.sourceForWindow(win), updater.recordWindow);
    await updater.refresh(); expect(activated).toEqual(["A"]);
    f.select({ ...f.a, key: "B", id: "ui-B" });
    await updater.refresh(); expect(activated).toEqual(["A", "B"]);
    await updater.refresh(); expect(activated).toEqual(["A", "B"]);
    f.select({ ...f.a, key: "C", id: "ui-C", restartRequired: true });
    await updater.refresh(); expect(activated).toEqual(["A", "B"]);
    f.select(new Error("invalid next generation"));
    await updater.refresh(); expect(activated).toEqual(["A", "B"]);
    expect(main).toContain("() => { void accessibilitySetupController?.run(); }");
  });

  test("committed generation wakes the same native controller without reviving Skip or creating another owner", async () => {
    const f = hotWindowFixture(), requests: any[] = [];
    const marker: any = { kind: "cli-owned", marker: { state: "granted", presentationOwner: "cli",
      requestId: "11111111-1111-4111-8111-111111111111", resumeReason: "post-update" } };
    let releaseNative!: (value: unknown) => void, releaseAck!: () => void;
    let updater: any;
    const begin = main.indexOf("function createNativePermissionSetupController(");
    const end = main.indexOf("\nfunction sessionBurnExpectation(", begin);
    expect(begin).toBeGreaterThanOrEqual(0); expect(end).toBeGreaterThan(begin);
    const controller: any = runInNewContext(main.slice(begin, end) + ";createNativePermissionSetupController(options)", {
      fs: {}, process: { platform: "darwin" },
      accessibilityPathIsDefaultApp: () => true, accessibilityInstallIdIsSafe: () => true,
      readNativePermissionMarker: () => marker,
      options: { platform: "darwin", appPath: "/Applications/ChatGPT.app", installId: "test-install", isIncognito: false,
        startPermissionCheck(resume: any) {
          requests.push({ runtimeSha256: updater.status().active.key, ...resume });
          return new Promise(resolve => { releaseNative = resolve; });
        } },
    });
    updater = f.api.createMacRendererUpdater(f.context.electron, undefined, undefined, () => { void controller.run(); });
    f.api.hookWindow(f.win, (win: unknown) => updater.sourceForWindow(win), updater.recordWindow);
    await updater.refresh(); await controller.run(); expect(requests).toEqual([]);
    marker.marker.state = "pending";
    const ack = new Promise<void>(resolve => { releaseAck = resolve; });
    const execute = f.win.webContents.executeJavaScript;
    f.win.webContents.executeJavaScript = async (source: string) => { if (source.includes("ui-B")) await ack; return execute(source); };
    f.select({ ...f.a, key: "B", id: "ui-B" });
    const switching = updater.refresh();
    for (let i = 0; i < 12; i++) await Promise.resolve();
    expect(updater.status().active.key).toBe("A"); expect(requests).toEqual([]);
    releaseAck(); await switching;
    for (let i = 0; i < 12; i++) await Promise.resolve();
    expect(requests).toEqual([{ runtimeSha256: "B", resumeRequestId: marker.marker.requestId }]);
    f.select({ ...f.a, key: "C", id: "ui-C" }); await updater.refresh();
    expect(requests).toHaveLength(1); // B 的 native owner 未完，C 不建立第二张卡。
    marker.marker.state = "deferred";
    releaseNative({ ok: true, state: "native-owned" });
    for (let i = 0; i < 12; i++) await Promise.resolve();
    await controller.run(); await updater.refresh();
    expect(requests).toHaveLength(1); // 用户 Skip 后，不因聚焦或同代刷新复活。
    controller.dispose(); updater.dispose();
  });

  test("host requests permission capabilities for the same candidate transaction", async () => {
    const f = hotWindowFixture();
    const loader: any = f.context.require(), prepared: any[] = [], loaded: any[] = [];
    const prepare = loader.prepareRendererUpdate;
    loader.prepareRendererUpdate = (...args: any[]) => { prepared.push(args); return prepare(); };
    const actions = { protocol: 1, featureProtocol: 1, permission: {}, open() {}, handle() {} };
    loader.loadMainActions = (...args: any[]) => { loaded.push(args); return actions; };
    const updater = f.api.createMacRendererUpdater(f.context.electron, {}, actions);
    f.api.hookWindow(f.win, () => updater.sourceForWindow(f.win)); await updater.refresh();
    expect(prepared.at(-1)[3]).toMatchObject({ mainActions: true, permissionCopy: true });
    expect(loaded.at(-1)[4]).toEqual({ permissionCopy: true });
    expect(updater.status().active.mainActions).toBe(actions);
    expect(main).toContain("permissionLocale: Object.freeze({");
    expect(main).toContain("accessibilitySetupController = createNativePermissionSetupController({");
    expect(main).toContain("runtimeSha256: rendererUpdater?.status().active?.key");
  });
});
