import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { targetStateDir } from "./runtime/incodex-instance.cts";
import { devHotEnabled, hotHomeRoot, resolveRuntimeFile } from "./runtime/incodex-runtime-load.cts";

describe("runtime load", () => {
  test("HOME missing does not yield a relative .incodex path", () => {
    expect(hotHomeRoot({})).toBeNull();
    expect(hotHomeRoot({ HOME: "" })).toBeNull();
    expect(hotHomeRoot({ HOME: "/Users/me" })).toBe(join("/Users/me", ".incodex"));
  });

  test("production ignores home overrides unless INCODEX_DEV_HOT=1", () => {
    const home = mkdtempSync(join(tmpdir(), "incodex-hot-"));
    const bundledDir = mkdtempSync(join(tmpdir(), "incodex-bundle-"));
    writeFileSync(join(bundledDir, "incodex-main.cjs"), "bundled");
    const dest = targetStateDir(join(home, ".incodex"), "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT");
    mkdirSync(dest, { recursive: true });
    writeFileSync(join(dest, "incodex-main.cjs"), "override");

    expect(devHotEnabled({})).toBe(false);
    expect(
      resolveRuntimeFile("incodex-main.cjs", bundledDir, { HOME: home }, "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT"),
    ).toBe(join(bundledDir, "incodex-main.cjs"));
    expect(
      resolveRuntimeFile(
        "incodex-main.cjs",
        bundledDir,
        { HOME: home, INCODEX_DEV_HOT: "1" },
        "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
      ),
    ).toBe(join(dest, "incodex-main.cjs"));
  });

  test("the asar loader fail-opens non-blocking attach errors", () => {
    const loader = readFileSync(join(import.meta.dir, "runtime/incodex-loader.cts"), "utf8");
    expect(loader).toContain("await loadMain();");
    expect(loader).toContain("require(originalMain())");
    expect(loader).toContain('error?.code === "INCODEX_STARTUP_BLOCKED"');
    expect(loader.indexOf("require(originalMain())")).toBeGreaterThan(loader.indexOf("await loadMain()"));
    expect(loader).not.toContain('require("./incodex-main.cjs")');
    expect(loader).toContain("current.json");
  });

  test("the loader gates official main on the incognito lease startup", () => {
    const loader = readFileSync(join(import.meta.dir, "runtime/incodex-loader.cts"), "utf8");
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    expect(loader).toContain("const runtime = require(file);");
    expect(loader).toContain("await runtime.startupGate");
    expect(loader).toContain('error?.code === "INCODEX_STARTUP_BLOCKED"');
    expect(loader.indexOf("require(originalMain())")).toBeGreaterThan(loader.indexOf("await loadMain()"));
    expect(main).toContain("const startupGate = attachElectron();");
    expect(main).toContain('error.code = "INCODEX_STARTUP_BLOCKED"');
  });

  test("an ordinary incognito click starts a child that reloads the current Runtime", () => {
    const loader = readFileSync(join(import.meta.dir, "runtime/incodex-loader.cts"), "utf8");
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    expect(loader).toContain("const current = JSON.parse(fs.readFileSync(currentPath, \"utf8\"));");
    expect(main).toContain("child = spawn(bin, args");
    expect(main).toContain('INCODEX_INCOGNITO: "1"');
    expect(main).toContain("CODEX_ELECTRON_USER_DATA_PATH: session.chromium");
    expect(main).toContain("`--user-data-dir=$" + "{session.chromium}`");
    expect(main).toContain("safeHome.handoffSessionOwner");
  });

  test("Windows swaps only the native lifecycle while keeping the shared UI and macOS launcher", () => {
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    expect(main).toContain('process.platform === "win32"');
    expect(main).toContain('require("./incodex-windows-platform.cjs")');
    expect(main).toContain("windowsPlatform.launchIncognito");
    expect(main).toContain("child = spawn(bin, args");
    expect(main).toContain("safeHome.handoffSessionOwner");
    expect(main).toContain("hookWindow(win, source)");
    expect(main).toContain('win.webContents.on("dom-ready", () => run(false))');
    expect(main).toContain('win.webContents.on("did-finish-load", () => run(true))');
    expect(main).toContain('probe?.accepted === true');
    expect(main).toContain('if (!windowsPlatform) markSessionReady()');
    expect(main).toContain('if (!acceptedWindows.has(win) || win.isDestroyed() || !win.isVisible()) return');
    expect(main).toContain('acceptedWindows.add(win)');
  });

  test("a normal Windows Runtime stays normal while only the macOS janitor is skipped", () => {
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8").replaceAll(
      "\r\n",
      "\n",
    );

    expect(main).toContain(
      'if (!isIncognito()) {\n    if (!windowsPlatform) {\n      try {\n        safeHome.sweepOrphanSessions',
    );
    expect(main).toContain('  } else {\n    process.env.INCODEX_INCOGNITO = "1";\n  }');
    expect(main).not.toContain('if (!isIncognito() && !windowsPlatform)');
  });

  test("an ordinary incognito click launches the official Codex route", () => {
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    const launchStart = main.indexOf("async function launchIncognitoOnce()");
    const launchEnd = main.indexOf("\nconst allowedWindows", launchStart);
    const launch = main.slice(launchStart, launchEnd);

    expect(launch).toMatch(
      /const args\s*=\s*\[`--user-data-dir=\$\{session\.chromium\}`,[\s\S]*codex:\/\/new\?mode=codex/,
    );
  });

  test("failed launches remain single-flight through promise settlement", () => {
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    const launchStart = main.indexOf("async function launchIncognitoOnce()");
    const launchEnd = main.indexOf("\nconst allowedWindows", launchStart);
    const launch = main.slice(launchStart, launchEnd);

    expect(launch).toContain("if (!prepared.ok) return Promise.resolve(prepared);");
    expect(launch).toContain('return Promise.resolve({ ok: false, reason: "spawn-failed" });');
  });

  test("the installed Runtime verifies the primary route before using Control+3 as fallback", () => {
    const loader = readFileSync(join(import.meta.dir, "../dist/incodex-loader.cjs"), "utf8");
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    const selectorStart = main.indexOf("function selectOfficialCodexModeFallback(win)");
    const selectorEnd = main.indexOf("\nfunction ", selectorStart + 1);
    const selector = main.slice(selectorStart, selectorEnd);

    expect(main).not.toContain("codexModeSelected");
    expect(main).toContain('require("./incodex-codex-mode.cjs")');
    expect(loader).toContain('"incodex-codex-mode.cjs"');
    expect(selector).toContain("if (!isIncognito()) return");
    expect(selector).toContain(
      'win.webContents.sendInputEvent({ type: "keyDown", keyCode: "3", modifiers: ["control"] })',
    );
    expect(selector).toContain(
      'win.webContents.sendInputEvent({ type: "keyUp", keyCode: "3", modifiers: ["control"] })',
    );
    const readyStart = main.indexOf('win.once("ready-to-show", () => {');
    const readyEnd = main.indexOf("\n    });", readyStart);
    const ready = main.slice(readyStart, readyEnd);
    expect(ready).not.toContain("selectOfficialCodexModeFallback");
    expect(main).toContain("codexModeReadiness.observe(win)");
  });

  test("an ordinary incognito click marks its session pending before child handoff", () => {
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    expect(main).toContain("handoffPending: true");
  });

  test("IPC identity is revoked when a main-frame navigation starts", () => {
    const guard = readFileSync(join(import.meta.dir, "runtime/incodex-ipc-guard.cts"), "utf8");
    expect(guard).toContain('"did-start-navigation"');
    expect(guard).toContain('"will-redirect"');
    expect(guard).toContain("revokeWindowIdentityOnNavigation");
  });
});
