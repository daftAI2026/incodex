/**
 * [INPUT]: 依赖主 Runtime 源码、POSIX path 和 Electron 事件夹具。
 * [OUTPUT]: 验证 fallback、owner 与 Darwin 退出诊断路径门。
 * [POS]: 实验合流回归；VM 固定 POSIX 路径，不访问真实 App、Keychain 或用户 Runtime。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { createCodexModeReadiness } from "./incodex-codex-mode.cts";

test("official quit diagnostics require explicit opt-in on the installed normal Mac host", () => {
  const main = readFileSync(new URL("./incodex-main.cts", import.meta.url), "utf8");
  const start = main.indexOf("function installQuitDiagnostics(");
  const end = main.indexOf("\nasync function attachElectron()", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  const source = main.slice(start, end).replace('"__INCODEX_QUIT_TRACE__"', "diagnosticFixture");
  const executable = "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT";
  const calls: any[] = [];
  const observed: any[] = [];
  const nativeModule = {};
  const app = {};
  const context: any = {
    process: { platform: "darwin", execPath: executable, pid: 42,
      resourcesPath: "/Applications/ChatGPT.app/Contents/Resources", env: {} },
    // Darwin app 路径始终使用 POSIX 语义，不跟随测试宿主平台。
    path: require("node:path").posix, DEFAULT_APP_EXECUTABLE_PATH: executable,
    isIncognito: () => false,
    require: (name: string) => { expect(name).toBe("node:module"); return nativeModule; },
    diagnosticFixture: { createQuitTrace: (options: any) => {
      calls.push(options); return { attachLifecycle: (host: unknown) => { expect(host).toBe(app); observed.push("attached"); } };
    } },
    logLaunch: (...args: any[]) => observed.push(args), electron: { app },
  };
  const invoke = () => runInNewContext(`${source}\ninstallQuitDiagnostics(electron)`, context);
  invoke();
  context.process.env.INCODEX_DIAGNOSTIC_QUIT_TRACE = "true";
  invoke();
  context.process.env.INCODEX_DIAGNOSTIC_QUIT_TRACE = "1";
  context.process.platform = "win32"; invoke();
  context.process.platform = "darwin";
  context.isIncognito = () => true; invoke();
  context.isIncognito = () => false;
  context.process.execPath = "/tmp/other/ChatGPT"; invoke();
  expect(calls).toEqual([]);
  expect(observed).toEqual([]);
  context.process.execPath = executable; invoke();
  expect(calls).toHaveLength(1);
  expect(calls[0]).toMatchObject({ moduleApi: nativeModule,
    skyPath: "/Applications/ChatGPT.app/Contents/Resources/native/sky.node" });
  expect(observed).toEqual(["attached", ["quit-trace-enabled", { pid: 42 }]]);
  calls[0].record("getter-value", true);
  expect(observed[2]).toEqual(["quit-trace-getter-value", { pid: 42, value: true }]);
  expect(main).toContain("installQuitDiagnostics(electron);");
});

test("experimental async fallback survives slow official route confirmation", async () => {
  const tasks: Array<{ callback: () => void; delay: number }> = [];
  const events: string[] = [];
  let probes = 0;
  let fallbacks = 0;
  const win = {
    isDestroyed: () => false, isFocused: () => true, once: () => {},
    webContents: {
      isDestroyed: () => false,
      executeJavaScript: async () => ({
        modeAvailable: true,
        modeLabel: ++probes > 8 ? "Codex" : "ChatGPT",
        officialBlockerVisible: false,
      }),
    },
  };
  const readiness = createCodexModeReadiness({
    isIncognito: () => true,
    log: (event: string) => events.push(event),
    primaryOtherChecksRequired: 1,
    confirmationFailuresRequired: 20,
    selectFallback: async () => { fallbacks++; return true; },
    scheduleTimer: (callback: () => void, delay: number) => {
      const task = { callback, delay }; tasks.push(task); return task;
    },
    cancelTimer: (task: { callback: () => void; delay: number }) => {
      const index = tasks.indexOf(task); if (index >= 0) tasks.splice(index, 1);
    },
  });
  readiness.observe(win);
  for (let turn = 0; turn < 9 && tasks.length; turn++) {
    tasks.shift()!.callback();
    for (let tick = 0; tick < 12; tick++) await Promise.resolve();
  }
  expect(fallbacks).toBe(1);
  expect(events).toEqual(["codex-mode-fallback-sent", "codex-mode-confirmed"]);
  expect(tasks).toHaveLength(0);
});

test("owner gating remembers only official show intent, never prewarm readiness", () => {
  const source = readFileSync(new URL("./incodex-main.cts", import.meta.url), "utf8");
  const start = source.indexOf('electron.app.on("browser-window-created"');
  const end = source.indexOf('\n  if (isIncognito() && windowsPlatform)', start);
  let createWindow: (_event: unknown, win: unknown) => void = () => {};
  let ready = false;
  const intent = new WeakSet();
  const shown = new WeakSet();
  const raised: string[] = [];
  const context = {
    electron: { app: { on: (_name: string, callback: typeof createWindow) => { createWindow = callback; } } },
    observeAccessibilityPresentationWindow: () => {}, accessibilitySetupController: null,
    isAuxiliaryWindow: () => false, hookWindow: () => {}, source: "",
    incognitoWindowLifecycle: null, rendererUpdater: null, isIncognito: () => true,
    get macOwnerReady() { return ready; },
    displayReadyWindows: intent, shownWindows: shown,
    applyChromeWindowTile: () => {}, raiseOurWindows: () => raised.push("raise"),
    markSessionReady: () => raised.push("ready"), markAcceptedWindowReady: () => {},
    windowsPlatform: null, setTimeout: () => {},
  };
  runInNewContext(source.slice(start, end), context);
  const makeWindow = () => {
    let visible = false;
    const listeners = new Map<string, () => void>();
    const win = {
      isDestroyed: () => false, isVisible: () => visible, isMinimized: () => false,
      hide: () => { visible = false; },
      once: (event: string, callback: () => void) => listeners.set(event, callback),
    };
    createWindow(null, win);
    return { win, emit: (event: string) => { if (event === "show") visible = true; listeners.get(event)?.(); } };
  };
  const prewarm = makeWindow();
  prewarm.emit("ready-to-show");
  expect(intent.has(prewarm.win)).toBe(false);
  expect(shown.has(prewarm.win)).toBe(false);
  const primary = makeWindow();
  primary.emit("show");
  expect(primary.win.isVisible()).toBe(false);
  expect(intent.has(primary.win)).toBe(true);
  expect(shown.has(primary.win)).toBe(true);
  expect(raised).toEqual([]);
  ready = true;
});

test("an unresponsive experimental fallback has a bounded lifetime", async () => {
  const tasks: Array<{ callback: () => void; delay: number }> = [];
  const events: string[] = [];
  const win = {
    isDestroyed: () => false, isFocused: () => true, once: () => {},
    webContents: {
      isDestroyed: () => false,
      executeJavaScript: async () => ({ modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false }),
    },
  };
  const readiness = createCodexModeReadiness({
    isIncognito: () => true, primaryOtherChecksRequired: 1,
    log: (event: string) => events.push(event),
    selectFallback: () => new Promise(() => {}),
    scheduleTimer: (callback: () => void, delay: number) => {
      const task = { callback, delay }; tasks.push(task); return task;
    },
    cancelTimer: (task: { callback: () => void; delay: number }) => {
      const index = tasks.indexOf(task); if (index >= 0) tasks.splice(index, 1);
    },
  });
  readiness.observe(win);
  tasks.shift()!.callback();
  for (let tick = 0; tick < 12; tick++) await Promise.resolve();
  expect(tasks.map((task) => task.delay)).toEqual([2_000]);
  tasks.shift()!.callback();
  for (let tick = 0; tick < 12; tick++) await Promise.resolve();
  expect(events).toContain("codex-mode-unresolved");
  expect(tasks).toHaveLength(0);
});
