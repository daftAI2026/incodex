/**
 * [INPUT]: 依赖主 Runtime 接线、登记 helper 与既有权限任务协议。
 * [OUTPUT]: 验证旧请求只交接原生 owner，不创建第二张卡或重置权限。
 * [POS]: 跨语言边界的隔离回归；不启动真实宿主或读取用户权限。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { afterEach, describe, expect, test } from "bun:test";
import { EventEmitter } from "node:events";
import { createHash } from "node:crypto";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runInNewContext } from "node:vm";

const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
const update = require("./runtime/incodex-macos-update.cts");
const APP = "/Applications/ChatGPT.app";
const EXEC = `${APP}/Contents/MacOS/ChatGPT`;
const INSTALL = "11111111-1111-4111-8111-111111111111";
const roots: string[] = [];
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }); });
const hash = (value: string) => createHash("sha256").update(value).digest("hex");

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "incodex-permission-handoff-")); roots.push(root);
  const helperSha256 = hash("helper");
  const release = join(root, "helpers/macos-update", helperSha256);
  const helperPath = join(release, "incodex");
  const coordinatorAppPath = join(release, "Incodex Update Coordinator.app");
  const coordinatorPath = join(coordinatorAppPath, "Contents/MacOS/incodex-update-coordinator");
  const interposerPath = join(release, "libincodex-sparkle-interpose.dylib");
  for (const dir of [join(root, "macos-update"), join(root, "runtime"), join(coordinatorAppPath, "Contents/MacOS")]) {
    mkdirSync(dir, { recursive: true, mode: 0o700 });
  }
  writeFileSync(helperPath, "helper", { mode: 0o700 });
  writeFileSync(coordinatorPath, "coordinator", { mode: 0o700 });
  writeFileSync(interposerPath, "interposer", { mode: 0o600 });
  writeFileSync(join(coordinatorAppPath, "Contents/Info.plist"), "plist");
  const registrationPath = join(root, "macos-update/registration.json");
  const registration = { schemaVersion: 2, installId: INSTALL, appPath: APP, helperPath, helperSha256,
    coordinatorAppPath, coordinatorPath, coordinatorSha256: hash("coordinator"),
    interposerPath, interposerSha256: hash("interposer") };
  writeFileSync(registrationPath, JSON.stringify(registration), { mode: 0o600 });
  const pointerPath = join(root, "runtime/current.json");
  writeFileSync(pointerPath, JSON.stringify({ schemaVersion: 1, manifestSha256: hash("manifest") }), { mode: 0o600 });
  const calls: any[] = [], child = new EventEmitter();
  Object.assign(child, { unref() {} });
  const options = { userRoot: root, execPath: EXEC, installId: INSTALL, platform: "darwin", runtimeSha256: hash("manifest"),
    env: { HOME: "/fixture-home", INCODEX_DEV_HOT: "1", INCODEX_MACOS_UPDATE_RELAUNCH: "stale",
      INCODEX_INTERNAL_PERMISSION_UPDATE: "stale", INCODEX_INTERNAL_UPDATE_NOTICE_WORKER: "1",
      INCODEX_INTERNAL_RUNTIME_SYNC: "1" },
    spawnProcess: (...args: any[]) => { calls.push(args); return child; } };
  return { root, options, calls, child, registrationPath, registration, pointerPath, helperPath };
}

function handoffController(marker: any, start: () => Promise<any>) {
  const begin = main.indexOf("function createNativePermissionSetupController(");
  expect(begin).toBeGreaterThanOrEqual(0);
  const end = main.indexOf("\nfunction sessionBurnExpectation(", begin);
  return runInNewContext(main.slice(begin, end) + ";createNativePermissionSetupController(options)", {
    options: { platform: "darwin", appPath: APP, installId: INSTALL, isIncognito: false,
      startPermissionCheck: start }, process: { platform: "darwin" }, fs: {},
    accessibilityPathIsDefaultApp: (app: string) => app === APP,
    accessibilityInstallIdIsSafe: (id: string) => id === INSTALL,
    readAccessibilityMarker: () => marker,
  });
}

describe("single native permission owner", () => {
  test("production host wires the native handoff instead of a second JS mutator", () => {
    const begin = main.indexOf("  let accessibilitySetupController = null;");
    const end = main.indexOf('  electron.app.once("will-quit", () => accessibilitySetupController?.dispose());', begin);
    expect(begin).toBeGreaterThanOrEqual(0); expect(end).toBeGreaterThan(begin);
    const owners: string[] = [], responders: string[] = [];
    const fake = (owner: string) => () => { owners.push(owner); return { run() {}, dispose() {} }; };
    runInNewContext(main.slice(begin, end), {
      isIncognito: () => false, process: { platform: "darwin", execPath: EXEC, pid: 1, versions: {} },
      path: require("node:path"), DEFAULT_APP_EXECUTABLE_PATH: EXEC, DEFAULT_APP_PATH: APP,
      USER_ROOT: "/fixture", ACCESSIBILITY_MARKER_NAME: "accessibility-setup.json", ACCESSIBILITY_BUNDLE_ID: "com.openai.codex",
      electron: { app: {}, systemPreferences: {} }, fs: {}, spawn() {},
      readInstalledRuntimeIdentity: () => ({ appPath: APP, installId: INSTALL }),
      instance: { processIdentity: () => ({ processStartIdentity: "one" }) },
      permissionProbe: { createPermissionProbeResponder() { responders.push("one"); return {}; } },
      createAccessibilitySetupController: fake("legacy"), createNativePermissionSetupController: fake("native"),
      logLaunch() {},
    });
    expect(responders).toEqual(["one"]);
    expect(owners).toEqual(["native"]);
  });

  test("pending legacy request uses one native flight even if CLI takes over during the await", async () => {
    const marker: any = { kind: "ok", marker: { state: "pending" } };
    let finish!: (value: any) => void, starts = 0;
    const controller: any = handoffController(marker, () => {
      starts++; marker.kind = "cli-owned"; return new Promise(resolve => { finish = resolve; });
    });
    const first = controller.run();
    for (let i = 0; i < 8; i++) await Promise.resolve();
    expect(starts).toBe(1); expect(controller.run()).toBe(first);
    finish({ ok: true, state: "native-owned" }); await first;
    expect(await controller.run()).toMatchObject({ state: "cli-owned" });
    expect(starts).toBe(1);
    controller.dispose();
  });

  test("CLI-owned, terminal and unknown markers never fall back to JS presentation or reset", async () => {
    let starts = 0;
    const marker: any = { kind: "cli-owned" };
    const controller: any = handoffController(marker, async () => { starts++; return { ok: true }; });
    expect(await controller.run()).toMatchObject({ state: "cli-owned" });
    marker.kind = "ok"; marker.marker = { state: "granted" };
    expect(await controller.run()).toMatchObject({ state: "granted" });
    marker.kind = "unsafe";
    expect(await controller.run()).toMatchObject({ state: "unsafe" });
    expect(starts).toBe(0);
    marker.kind = "ok"; marker.marker = { state: "awaiting-user" };
    await controller.run(); expect(starts).toBe(1); // 正控制。
    controller.dispose();
  });
});

describe("registered native permission task producer", () => {
  test("reuses the existing bounded task protocol and retains one human-lived child", async () => {
    const f = fixture();
    expect(typeof update.startPermissionCheck).toBe("function");
    const pending = update.startPermissionCheck(f.options);
    expect(f.calls).toHaveLength(1);
    const [file, args, opts] = f.calls[0];
    expect(file).toBe(f.helperPath); expect(args).toEqual([]);
    expect(opts.detached).toBe(true); expect(opts.stdio).toBe("ignore");
    expect(opts.env.HOME).toBe("/fixture-home");
    for (const key of ["INCODEX_DEV_HOT", "INCODEX_MACOS_UPDATE_RELAUNCH", "INCODEX_INTERNAL_UPDATE_NOTICE_WORKER", "INCODEX_INTERNAL_RUNTIME_SYNC"]) {
      expect(opts.env[key]).toBeUndefined();
    }
    expect(JSON.parse(opts.env.INCODEX_INTERNAL_PERMISSION_UPDATE)).toEqual({ schemaVersion: 1,
      installId: INSTALL, helperSha256: f.registration.helperSha256,
      runtimeSha256: hash("manifest"), source: "runtime" });
    let settled = false; void pending.then(() => { settled = true; });
    for (let i = 0; i < 8; i++) await Promise.resolve();
    expect(settled).toBe(false); // 不以启动成功冒充授权完成，也不设置人的期限。
    f.child.emit("close", 0);
    expect(await pending).toEqual({ ok: true, state: "native-owned" });
  });

  test.each(["epoch", "helper", "pointer", "selection", "symlink", "platform", "nondefault"])("rejects %s before spawning", async kind => {
    const f = fixture();
    expect(typeof update.startPermissionCheck).toBe("function");
    if (kind === "epoch") f.options.installId = "22222222-2222-4222-8222-222222222222";
    if (kind === "selection") f.options.runtimeSha256 = hash("other generation");
    if (kind === "helper") writeFileSync(f.helperPath, "changed");
    if (kind === "pointer") writeFileSync(f.pointerPath, JSON.stringify({ schemaVersion: 1, manifestSha256: "bad" }));
    if (kind === "symlink") { rmSync(f.helperPath); symlinkSync(join(f.root, "absent"), f.helperPath); }
    if (kind === "platform") f.options.platform = "win32";
    if (kind === "nondefault") f.options.execPath = "/tmp/Copy.app/Contents/MacOS/ChatGPT";
    expect(await update.startPermissionCheck(f.options)).toMatchObject({ ok: false, state: "pending" });
    expect(f.calls).toHaveLength(0);
  });

  test("spawn and native failure stay pending and never cause local reset or marker writes", async () => {
    const f = fixture();
    expect(typeof update.startPermissionCheck).toBe("function");
    const before = readFileSync(f.registrationPath, "utf8");
    const failure = update.startPermissionCheck(f.options); f.child.emit("close", 1);
    expect(await failure).toMatchObject({ ok: false, state: "pending" });
    f.options.spawnProcess = () => { throw new Error("fixture spawn failure"); };
    expect(await update.startPermissionCheck(f.options)).toMatchObject({ ok: false, state: "pending" });
    expect(readFileSync(f.registrationPath, "utf8")).toBe(before);
    chmodSync(f.helperPath, 0o700);
  });
});
