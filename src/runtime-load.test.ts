import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as runtimeLoad from "./runtime/incodex-runtime-load.cts";
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

function runtimeFixture(name: string, source: string, options: {
  home?: string;
  version?: string;
  siblings?: Record<string, string>;
} = {}) {
  const home = options.home ?? mkdtempSync(join(tmpdir(), "incodex-verified-runtime-"));
  const runtimeRoot = join(home, ".incodex", "runtime");
  const version = options.version ?? "1.2.3";
  const sourceCommit = "";
  const sources = { ...options.siblings, [name]: source };
  const files = Object.fromEntries(Object.entries(sources).map(([file, bytes]) => [file, hash(bytes)]));
  const manifestBytes = Buffer.from(`${JSON.stringify({ runtimeVersion: version, sourceCommit, files })}\n`);
  const manifestSha256 = hash(manifestBytes);
  const release = `releases/${version}-${manifestSha256}`;
  const releaseDir = join(runtimeRoot, release);
  mkdirSync(releaseDir, { recursive: true });
  for (const [file, bytes] of Object.entries(sources)) writeFileSync(join(releaseDir, file), bytes);
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
  test("a running generation keeps its verified lazy modules after publication selects another release", () => {
    const name = "incodex-permission-copy.json";
    const moduleName = "incodex-permission-ui.cjs";
    const fixture = runtimeFixture(name, '{"generation":"A"}', {
      siblings: { [moduleName]: 'module.exports = { generation: "A" };' },
    });
    try {
      expect(readRuntimeJson(name, fixture.releaseDir, { HOME: fixture.home })).toEqual({ generation: "A" });
      const pointerPath = join(fixture.home, ".incodex", "runtime", "current.json");
      const next = runtimeFixture(name, '{"generation":"B"}', {
        home: fixture.home, version: "1.2.4",
        siblings: { [moduleName]: 'module.exports = { generation: "B" };' },
      });
      // The publisher may move on, or its next pointer may be unreadable. Neither
      // revokes the immutable generation already verified by this process.
      expect(readRuntimeJson(name, fixture.releaseDir, { HOME: fixture.home })).toEqual({ generation: "A" });
      expect(loadRuntimeModule(moduleName, fixture.releaseDir, { HOME: fixture.home })).toEqual({ generation: "A" });
      expect(loadRuntimeModule(moduleName, next.releaseDir, { HOME: fixture.home })).toEqual({ generation: "B" });
      writeFileSync(pointerPath, "incomplete next publication");
      expect(readRuntimeJson(name, fixture.releaseDir, { HOME: fixture.home })).toEqual({ generation: "A" });
    } finally {
      rmSync(fixture.home, { recursive: true, force: true });
    }
  });

  test("an unselected release cannot establish a process generation pin", () => {
    const name = "incodex-permission-copy.json";
    const fixture = runtimeFixture(name, '{"generation":"unselected"}');
    try {
      const pointerPath = join(fixture.home, ".incodex", "runtime", "current.json");
      const pointer = JSON.parse(readFileSync(pointerPath, "utf8"));
      pointer.release = `releases/other-${pointer.manifestSha256}`;
      writeFileSync(pointerPath, JSON.stringify(pointer));
      expect(() => readRuntimeJson(name, fixture.releaseDir, { HOME: fixture.home })).toThrow(
        "Runtime artifact is outside the active release",
      );
    } finally {
      rmSync(fixture.home, { recursive: true, force: true });
    }
  });

  test("pinning does not trust cached bytes or a replaced manifest after pointer drift", () => {
    const name = "incodex-permission-ui.cjs";
    const fixture = runtimeFixture(name, 'module.exports = { generation: "A" };');
    try {
      expect(loadRuntimeModule(name, fixture.releaseDir, { HOME: fixture.home })).toEqual({ generation: "A" });
      const pointerPath = join(fixture.home, ".incodex", "runtime", "current.json");
      writeFileSync(pointerPath, "next pointer is irrelevant to the pinned process");
      const modulePath = join(fixture.releaseDir, name);
      const original = readFileSync(modulePath);
      writeFileSync(modulePath, 'module.exports = { generation: "tampered" };');
      expect(() => loadRuntimeModule(name, fixture.releaseDir, { HOME: fixture.home })).toThrow(
        `Runtime artifact hash mismatch ${name}`,
      );
      writeFileSync(modulePath, original);
      writeFileSync(join(fixture.releaseDir, "runtime-manifest.json"), "{}");
      expect(() => loadRuntimeModule(name, fixture.releaseDir, { HOME: fixture.home })).toThrow(
        "Runtime manifest hash mismatch",
      );
    } finally {
      rmSync(fixture.home, { recursive: true, force: true });
    }
  });

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

  test("the asar loader fail-opens non-blocking attach errors", () => {
    const loader = readFileSync(join(import.meta.dir, "runtime/incodex-loader.cts"), "utf8");
    expect(loader).toContain("await loadMain();");
    expect(loader).toContain("require(originalMain())");
    expect(loader).toContain('error?.code === "INCODEX_STARTUP_BLOCKED"');
    expect(loader.indexOf("require(originalMain())")).toBeGreaterThan(loader.indexOf("await loadMain()"));
    expect(loader).not.toContain('require("./incodex-main.cjs")');
    expect(loader).toContain("current.json");
  });

  test("the status menu stays inside an artifact already gated by installed loaders", () => {
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");

    expect(main).not.toContain('require("./incodex-status-menu.cjs")');
    expect(main).toContain("dockMenu.createStatusMenuController");
    expect(main).toContain("dockMenu.createNativeStatusMenuBridge");
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


describe("renderer update preparation", () => {
  const prepare = (...args: any[]) => (runtimeLoad as any).prepareRendererUpdate(...args);
  const selected = (candidate: any) => (runtimeLoad as any).rendererUpdateStillSelected(candidate);
  const injector = "incodex-inject.js", main = "incodex-main.cjs";
  test("verifies a newly selected UI while retaining the running release", () => {
    const a = runtimeFixture(injector, "generation A", { siblings: { [main]: "same main" } });
    try {
      const initial = prepare(a.releaseDir, { HOME: a.home });
      const b = runtimeFixture(injector, "generation B", { home: a.home, siblings: { [main]: "same main" } });
      const candidate = prepare(a.releaseDir, { HOME: a.home });
      expect(candidate).toMatchObject({ source: "generation B", restartRequired: false, releaseDir: b.releaseDir });
      expect(candidate.id).toBe(hash("generation B"));
      expect(candidate.key).not.toBe(initial.key);
      expect(selected(candidate)).toBe(true);
      runtimeFixture(injector, "generation C", { home: a.home, siblings: { [main]: "same main" } });
      expect(selected(candidate)).toBe(false);
      expect(runtimeLoad.readVerifiedRuntimeArtifact(injector, a.releaseDir, { HOME: a.home }).bytes.toString()).toBe("generation A");
    } finally { rmSync(a.home, { recursive: true, force: true }); }
  });
  test("supports the existing native host size while verifying unchanged assets", () => {
    const native = "incodex-permission-host", body = "n".repeat(3 * 1024 * 1024);
    const a = runtimeFixture(injector, "A", { siblings: { [main]: "same", [native]: body } });
    try {
      prepare(a.releaseDir, { HOME: a.home });
      runtimeFixture(injector, "B", { home: a.home, siblings: { [main]: "same", [native]: body } });
      expect(prepare(a.releaseDir, { HOME: a.home })).toMatchObject({ restartRequired: false, source: "B" });
    } finally { rmSync(a.home, { recursive: true, force: true }); }
  });
  test("main or native changes require restart and never prepare executable UI", () => {
    const a = runtimeFixture(injector, "A", { siblings: { [main]: "main A" } });
    try {
      prepare(a.releaseDir, { HOME: a.home });
      runtimeFixture(injector, "B", { home: a.home, siblings: { [main]: "main B" } });
      expect(prepare(a.releaseDir, { HOME: a.home })).toMatchObject({ restartRequired: true, source: "" });
    } finally { rmSync(a.home, { recursive: true, force: true }); }
  });
  for (const failure of ["bytes", "unchanged-bytes", "symlink", "release-ancestry", "mixed-manifest", "missing-entry", "pointer", "traversal"]) {
    test(`rejects ${failure} without changing the running generation`, () => {
      const a = runtimeFixture(injector, "A", { siblings: { [main]: "same" } });
      try {
        prepare(a.releaseDir, { HOME: a.home });
        const b = runtimeFixture(injector, "B", { home: a.home, siblings: { [main]: "same" } });
        const pointerPath = join(a.home, ".incodex/runtime/current.json");
        if (failure === "bytes") writeFileSync(join(b.releaseDir, injector), "tampered");
        if (failure === "unchanged-bytes") writeFileSync(join(b.releaseDir, main), "tampered");
        if (failure === "release-ancestry") {
          const releases = join(a.home, ".incodex/runtime/releases"), outside = join(a.home, "outside");
          renameSync(releases, outside); symlinkSync(outside, releases, "junction");
        }
        if (failure === "missing-entry") {
          const pointer = JSON.parse(readFileSync(pointerPath, "utf8"));
          delete pointer.files[main]; writeFileSync(pointerPath, JSON.stringify(pointer));
        }
        if (failure === "symlink") {
          rmSync(join(b.releaseDir, injector)); symlinkSync(join(a.releaseDir, injector), join(b.releaseDir, injector));
        }
        if (failure === "mixed-manifest") {
          const pointer = JSON.parse(readFileSync(pointerPath, "utf8"));
          pointer.files[main] = hash("other main"); writeFileSync(pointerPath, JSON.stringify(pointer));
        }
        if (failure === "pointer") writeFileSync(pointerPath, "partial publication");
        if (failure === "traversal") {
          const pointer = JSON.parse(readFileSync(pointerPath, "utf8"));
          pointer.release = "releases/../outside"; writeFileSync(pointerPath, JSON.stringify(pointer));
        }
        expect(() => prepare(a.releaseDir, { HOME: a.home })).toThrow();
        expect(runtimeLoad.readVerifiedRuntimeArtifact(injector, a.releaseDir, { HOME: a.home }).bytes.toString()).toBe("A");
      } finally { rmSync(a.home, { recursive: true, force: true }); }
    });
  }
});

describe("renderer update coordination", () => {
  function fixture() {
    const a = { key: "A", id: "ui-A", source: "A" }, b = { key: "B", id: "ui-B", source: "B" };
    let candidate: any = b, selected = "B";
    const calls: string[] = [], windows = ["one", "two"];
    const create = (apply: (window: string, value: any) => Promise<boolean>) =>
      (runtimeLoad as any).createRendererUpdateCoordinator({ initial: a,
        prepare: () => candidate, windows: () => [...windows],
        apply: async (window: string, value: any) => { calls.push(`${window}:${value.key}`); return apply(window, value); },
        isSelected: (value: any) => value.key === selected,
      });
    return { a, b, calls, create, addWindow(value: string) { windows.push(value); }, select(value: any) { candidate = value; selected = value.key; } };
  }
  test("commits only after all windows acknowledge", async () => {
    const f = fixture(); let release!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; });
    const coordinator = f.create(async (window) => { if (window === "two") await gate; return true; });
    const pending = coordinator.refresh();
    await Promise.resolve(); await Promise.resolve();
    expect(coordinator.status().active.key).toBe("A");
    release(); await pending;
    expect(coordinator.status()).toMatchObject({ phase: "active", active: { key: "B" } });
    expect(f.calls).toEqual(["one:B", "two:B"]);
  });
  test("includes a window opened while activation is still in flight", async () => {
    const f = fixture();
    const coordinator = f.create(async (window, value) => {
      if (window === "one" && value.key === "B") f.addWindow("three");
      return true;
    });
    await coordinator.refresh();
    expect(coordinator.status().active.key).toBe("B");
    expect(f.calls).toEqual(["one:B", "two:B", "three:B"]);
  });
  test("partial activation rolls back every attempted window, including a missing ACK", async () => {
    const f = fixture(), coordinator = f.create(async (window, value) => !(window === "two" && value.key === "B"));
    await coordinator.refresh();
    expect(coordinator.status()).toMatchObject({ phase: "retained", active: { key: "A" } });
    expect(f.calls).toEqual(["one:B", "two:B", "two:A", "one:A"]);
  });
  test("does not claim rollback succeeded when an individual window refuses it", async () => {
    const f = fixture(), coordinator = f.create(async (window, value) => window !== "two" || value.key !== "B" && value.key !== "A");
    await coordinator.refresh();
    expect(coordinator.status()).toMatchObject({ phase: "rollback-failed", active: { key: "A" } });
    expect(coordinator.status().windows).toContainEqual({ window: "two", state: "rollback-failed" });
  });
  test("coalesces updates and rolls back an obsolete candidate before applying the latest", async () => {
    const f = fixture(); let release!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; });
    const coordinator = f.create(async (window, value) => { if (window === "one" && value.key === "B") await gate; return true; });
    const first = coordinator.refresh();
    await Promise.resolve();
    f.select({ key: "C", id: "ui-C", source: "C" }); const second = coordinator.refresh();
    release(); await Promise.all([first, second]);
    expect(coordinator.status().active.key).toBe("C");
    expect(f.calls).toEqual(["one:B", "one:A", "one:C", "two:C"]);
  });
  test("an unsupported candidate or disposal never commits a new generation", async () => {
    const f = fixture(), coordinator = f.create(async () => true);
    f.select({ key: "B", restartRequired: true, source: "" });
    await coordinator.refresh();
    expect(coordinator.status()).toMatchObject({ phase: "restart-required", active: { key: "A" } });
    expect(f.calls).toEqual([]);
    coordinator.dispose(); f.select({ key: "C", source: "C" }); await coordinator.refresh();
    expect(coordinator.status().active.key).toBe("A");
    expect(f.calls).toEqual([]);
  });
});
