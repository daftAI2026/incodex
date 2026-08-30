import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");

function hookWindowSource(): string {
  const start = main.indexOf("function hookWindow(");
  const end = main.indexOf("\nasync function attachElectron()", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  return main.slice(start, end);
}

describe("Electron UI injection reporting", () => {
  test("native menu launches inherit geometry only from a real main window", () => {
    const start = main.indexOf("function captureSourceBounds()");
    const end = main.indexOf("\nfunction readSourceBounds()", start);
    const capture = main.slice(start, end);

    expect(start).toBeGreaterThanOrEqual(0);
    expect(end).toBeGreaterThan(start);
    expect(capture).toContain("mainWindows(electron)[0]");
    expect(capture).not.toContain("BrowserWindow.getAllWindows()[0]");
  });

  test("lets an authorized renderer configure the macOS Dock decorator", () => {
    expect(main).toContain('require("./incodex-dock-menu.cjs")');
    expect(main).toContain('action === "configure-dock-menu"');
    expect(main).toContain("dockMenuController.configure(payload?.label)");
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

  test("ends an incognito session when either host hides its last main window", () => {
    const created = main.indexOf('electron.app.on("browser-window-created"');
    const officialReturn = main.indexOf("if (!isIncognito()) return;", created);
    expect(created).toBeGreaterThanOrEqual(0);
    expect(officialReturn).toBeGreaterThan(created);

    const beforeOfficialReturn = main.slice(created, officialReturn);
    expect(main).toContain('require("./incodex-window-lifecycle.cjs")');
    expect(main).toContain("windowLifecycle.createIncognitoWindowLifecycle(");
    expect(main).toContain("finishIncognito,");
    expect(beforeOfficialReturn).toContain("if (isIncognito())");
    expect(beforeOfficialReturn).toContain("incognitoWindowLifecycle.observe(win)");
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

  test("does not quit the installed Windows main process on the user's behalf", () => {
    const attach = main.slice(main.indexOf("async function attachElectron()"));

    expect(attach).not.toContain("windowsPlatform.listenForNormalExit(");
    expect(attach).not.toContain("INCODEX_WINDOWS_REGISTRATION_ID");
    expect(attach).not.toMatch(
      /if \(windowsPlatform && !isIncognito\(\)\)[\s\S]*electron\.app\.quit\(\)/,
    );
  });
});
