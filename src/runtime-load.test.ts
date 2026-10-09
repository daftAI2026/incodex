/**
 * [INPUT]: 依赖隔离临时 Runtime catalog 与 loader 源码。
 * [OUTPUT]: 验证 sibling 哈希检查和不阻塞官方 main 的实验加载顺序。
 * [POS]: Runtime 启动与已安装 Loader 兼容性的回归边界。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { targetStateDir } from "./runtime/incodex-instance.cts";
import {
  devHotEnabled,
  hotHomeRoot,
  loadRuntimeModule,
  readRuntimeJson,
  resolveRuntimeFile,
} from "./runtime/incodex-runtime-load.cts";

function hash(bytes: string | Buffer): string {
  return createHash("sha256").update(bytes).digest("hex");
}

function runtimeFixture(name: string, source: string) {
  const home = mkdtempSync(join(tmpdir(), "incodex-verified-runtime-"));
  const runtimeRoot = join(home, ".incodex", "runtime");
  const version = "1.2.3";
  const sourceCommit = "";
  const files = { [name]: hash(source) };
  const manifestBytes = Buffer.from(`${JSON.stringify({ runtimeVersion: version, sourceCommit, files })}\n`);
  const manifestSha256 = hash(manifestBytes);
  const release = `releases/${version}-${manifestSha256}`;
  const releaseDir = join(runtimeRoot, release);
  mkdirSync(releaseDir, { recursive: true });
  writeFileSync(join(releaseDir, name), source);
  writeFileSync(join(releaseDir, "runtime-manifest.json"), manifestBytes);
  writeFileSync(join(runtimeRoot, "current.json"), `${JSON.stringify({
    schemaVersion: 1,
    version,
    sourceCommit,
    release,
    manifestSha256,
    files,
  })}\n`);
  return { home, releaseDir };
}

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

  test("permission sibling modules are checked before their bytes are executed", () => {
    const name = "incodex-permission-ui.cjs";
    const marker = join(tmpdir(), `incodex-tampered-sibling-${Date.now()}`);
    const fixture = runtimeFixture(name, `module.exports = { verified: true };`);
    writeFileSync(
      join(fixture.releaseDir, name),
      `require("node:fs").writeFileSync(${JSON.stringify(marker)}, "executed"); module.exports = {};`,
    );

    expect(() => loadRuntimeModule(name, fixture.releaseDir, { HOME: fixture.home })).toThrow(
      "Runtime artifact hash mismatch incodex-permission-ui.cjs",
    );
    expect(existsSync(marker)).toBe(false);
  });

  test("permission copy JSON is read from the active manifest-verified release", () => {
    const name = "incodex-permission-copy.json";
    const copy = { en: { title: "Verified copy" } };
    const fixture = runtimeFixture(name, JSON.stringify(copy));

    expect(readRuntimeJson(name, fixture.releaseDir, { HOME: fixture.home })).toEqual(copy);
  });

  test("verified modules retain Node builtin resolution from the release directory", () => {
    const name = "incodex-permission-ui.cjs";
    const fixture = runtimeFixture(name, 'const path = require("node:path"); module.exports = { base: path.basename("/tmp/verified") };');

    expect(loadRuntimeModule(name, fixture.releaseDir, { HOME: fixture.home })).toEqual({ base: "verified" });
  });

  test("Runtime release verification finds current.json beside the release without HOME", () => {
    const name = "incodex-permission-ui.cjs";
    const fixture = runtimeFixture(name, 'module.exports = { verified: true };');

    expect(loadRuntimeModule(name, fixture.releaseDir, {})).toEqual({ verified: true });
  });

  test("dev-hot Runtime modules keep using the explicit target override", () => {
    const home = mkdtempSync(join(tmpdir(), "incodex-hot-verified-"));
    const execPath = "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT";
    const overrideDir = targetStateDir(join(home, ".incodex"), execPath);
    mkdirSync(overrideDir, { recursive: true });
    writeFileSync(join(overrideDir, "incodex-main.cjs"), "// dev-hot main");
    writeFileSync(join(overrideDir, "incodex-permission-ui.cjs"), 'module.exports = { source: "override" };');

    expect(loadRuntimeModule(
      "incodex-permission-ui.cjs",
      overrideDir,
      { HOME: home, INCODEX_DEV_HOT: "1" },
      execPath,
    )).toEqual({ source: "override" });
  });

  test("the asar loader starts official main before awaiting Runtime attachment", () => {
    const loader = readFileSync(join(import.meta.dir, "runtime/incodex-loader.cts"), "utf8");
    expect(loader).toContain("runtimeStartup = loadMain();");
    expect(loader).toContain("require(originalMain())");
    expect(loader).toContain("await runtimeStartup");
    expect(loader).toContain('error?.code === "INCODEX_STARTUP_BLOCKED"');
    expect(loader.indexOf("require(originalMain())")).toBeLessThan(
      loader.indexOf("await runtimeStartup"),
    );
    expect(loader).not.toContain('require("./incodex-main.cjs")');
    expect(loader).toContain("current.json");
  });

  test("the status menu stays inside an artifact already gated by installed loaders", () => {
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");

    expect(main).not.toContain('require("./incodex-status-menu.cjs")');
    expect(main).toContain("dockMenu.createStatusMenuController");
    expect(main).toContain("dockMenu.createNativeStatusMenuBridge");
  });

  test("the loader preserves Runtime startup reporting without delaying official protocol setup", () => {
    const loader = readFileSync(join(import.meta.dir, "runtime/incodex-loader.cts"), "utf8");
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    expect(loader).toContain("const runtime = require(file);");
    expect(loader).toContain("return runtime.startupGate");
    expect(loader).toContain('error?.code === "INCODEX_STARTUP_BLOCKED"');
    expect(loader.indexOf("require(originalMain())")).toBeLessThan(
      loader.indexOf("await runtimeStartup"),
    );
    expect(main).toContain("const startupGate = startRuntime();");
    expect(main.indexOf("return attachElectron();")).toBeGreaterThan(
      main.indexOf("macosUpdate.prepareUpdateHandoff({"),
    );
    expect(main).not.toContain("await macosUpdate.prepareUpdateHandoff(");
    expect(main).toContain('error.code = "INCODEX_STARTUP_BLOCKED"');
  });

  test("an ordinary incognito click starts a child that reloads the current Runtime", () => {
    const loader = readFileSync(join(import.meta.dir, "runtime/incodex-loader.cts"), "utf8");
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    expect(loader).toContain("const current = JSON.parse(fs.readFileSync(currentPath, \"utf8\"));");
    expect(main).toContain("child = spawn(bin, args");
    expect(main).toContain('INCODEX_INCOGNITO: "1"');
    expect(main).toContain("CODEX_ELECTRON_USER_DATA_PATH: session.chromium");
    expect(main).toContain("`--user-data-dir=$" + "{chromiumPath}`");
    expect(main).toContain("const args = incognitoLaunchArguments(session.chromium)");
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
    const launchStart = main.indexOf("async function launchIncognitoOnce(");
    const launchEnd = main.indexOf("\nconst allowedWindows", launchStart);
    const launch = main.slice(launchStart, launchEnd);

    expect(main).toContain('const args = [`--user-data-dir=${chromiumPath}`, "codex://new?mode=codex"]');
    expect(launch).toContain("const args = incognitoLaunchArguments(session.chromium)");
  });

  test("failed launches remain single-flight through promise settlement", () => {
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    const launchStart = main.indexOf("async function launchIncognitoOnce(");
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
      "win.webContents.executeJavaScript(codexMode.CODEX_MODE_FALLBACK_EXPRESSION, false)",
    );
    expect(selector).not.toContain("sendInputEvent");
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
