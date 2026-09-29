import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { runInNewContext } from "node:vm";

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
    expect(main).toContain('action === "configure-dock-menu"');
    expect(main).toContain("dockMenuController.configure(payload?.label)");
  });

  test("passes the authorized renderer window into the incognito launch", () => {
    const start = main.indexOf('electron.ipcMain.handle("incodex-action"');
    const end = main.indexOf("\n  });", start);
    const handler = main.slice(start, end);

    expect(handler).toContain("BrowserWindow.fromWebContents(event.sender)");
    expect(handler).toContain("launchIncognito(sourceWindow)");
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
    const end = main.indexOf("\n  electron.ipcMain.handle", start);
    const finish = main.slice(start, end);

    expect(start).toBeGreaterThanOrEqual(0);
    expect(end).toBeGreaterThan(start);
    expect(finish).toContain("if (incognitoExitStarted) return;");
    expect(finish).toContain("incognitoExitStarted = true;");
    expect(finish).not.toContain("windowsPlatform && incognitoExitStarted");
  });

  test("defers circle-x cleanup until the official close is accepted", () => {
    const quitStart = main.indexOf('if (action === "quit")');
    const quitEnd = main.indexOf('\n    return ipcGuard.actionResponse', quitStart);
    const quit = main.slice(quitStart, quitEnd);

    expect(quitStart).toBeGreaterThanOrEqual(0);
    expect(quitEnd).toBeGreaterThan(quitStart);
    expect(quit).toContain("electron.app.quit()");
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
