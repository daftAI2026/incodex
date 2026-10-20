/**
 * [INPUT]: 依赖真实 loader/probe 源码、隔离 catalog 与显式目录通知。
 * [OUTPUT]: 验证 loader 隔离、probe 生命周期与真实文件移除观察。
 * [POS]: 主进程 VM 契约；OS 通知另由 Node probe 集成套件验证，不操作宿主 App。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  readFileSync,
  existsSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import * as nodeCrypto from "node:crypto";
import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import * as nodePath from "node:path";
import { join } from "node:path";
import { createRequire } from "node:module";
import vm from "node:vm";
import { RUNTIME_EXTERNAL_ARTIFACT_NAMES } from "./runtime-manifest.ts";

const requireFixture = createRequire(import.meta.url);
const { createGuardedFsForTest } = requireFixture(
  "../crates/incodex-cli/tests/fixtures/install-permission-runtime.cjs",
) as {
  createGuardedFsForTest: (options: Record<string, unknown>) => typeof nodeFs & {
    requestRemovalObserved: boolean;
  };
};

const PREFLIGHT_ENV = "INCODEX_INTERNAL_PERMISSION_PREFLIGHT";
const ORIGINAL_MAIN = "official-main.cjs";
const RUNTIME_VERSION = "1.0.0";

function sha256(value: string | Buffer): string {
  return createHash("sha256").update(value).digest("hex");
}

function createBundleFixture() {
  const root = mkdtempSync(join(tmpdir(), "incodex-install-permission-"));
  const home = join(root, "home");
  const bundle = join(root, "ChatGPT.app", "Contents", "Resources", "app.asar.unpacked");
  const runtimeRoot = join(home, ".incodex", "runtime");
  const runtimeFiles: Record<string, string> = Object.fromEntries(
    RUNTIME_EXTERNAL_ARTIFACT_NAMES.map((name) => [name, `fixture:${name}`]),
  );
  const fileHashes = Object.fromEntries(
    Object.entries(runtimeFiles).map(([name, bytes]) => [name, sha256(bytes)]),
  );
  const manifest = Buffer.from(`${JSON.stringify({
    runtimeVersion: RUNTIME_VERSION,
    sourceCommit: "",
    files: fileHashes,
  })}\n`);
  const manifestSha256 = sha256(manifest);
  const release = `releases/${RUNTIME_VERSION}-${manifestSha256}`;
  const releaseDir = join(runtimeRoot, release);

  mkdirSync(bundle, { recursive: true });
  mkdirSync(releaseDir, { recursive: true });
  writeFileSync(join(bundle, "package.json"), JSON.stringify({
    __incodex: { originalMain: ORIGINAL_MAIN },
  }));
  writeFileSync(join(bundle, ORIGINAL_MAIN), "// isolated official-main marker\n");
  for (const [name, bytes] of Object.entries(runtimeFiles)) {
    writeFileSync(join(releaseDir, name), bytes);
  }
  writeFileSync(join(releaseDir, "runtime-manifest.json"), manifest);
  writeFileSync(join(runtimeRoot, "current.json"), `${JSON.stringify({
    schemaVersion: 1,
    version: RUNTIME_VERSION,
    sourceCommit: "",
    release,
    manifestSha256,
    files: fileHashes,
  })}\n`);

  return { root, home, bundle, runtimeFile: join(releaseDir, "incodex-main.cjs") };
}

function loadActualBootstrap(options: {
  platform: string;
  env?: Record<string, string>;
  runtime: Record<string, unknown> | ((fixture: { electron: { app: { quit: () => void } }; events: string[] }) => Record<string, unknown>);
}) {
  const fixture = createBundleFixture();
  const events: string[] = [];
  let quitCalls = 0;
  const runtimeFile = fixture.runtimeFile;
  const originalMain = join(fixture.bundle, ORIGINAL_MAIN);
  const electron = { app: { quit: () => { quitCalls += 1; events.push("probe-app-quit"); } } };
  const runtimeRequire = (request: string) => {
    if (request === "node:crypto") return nodeCrypto;
    if (request === "node:fs") return nodeFs;
    if (request === "node:path") return nodePath;
    if (request === runtimeFile) {
      events.push("runtime-required");
      return typeof options.runtime === "function"
        ? options.runtime({ electron, events })
        : options.runtime;
    }
    if (request === "electron") return electron;
    if (request === originalMain) {
      events.push("official-main-required");
      return {};
    }
    throw new Error(`unexpected loader require: ${request}`);
  };
  const context = vm.createContext({
    __dirname: fixture.bundle,
    process: {
      platform: options.platform,
      execPath: "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
      env: { HOME: fixture.home, ...options.env },
    },
    require: runtimeRequire,
    console: {
      error: () => events.push("loader-error"),
    },
  });
  const source = readFileSync(join(import.meta.dir, "runtime/incodex-loader.cts"), "utf8");
  const marker = '"__INCODEX_RUNTIME_FILES__"';
  const artifactCatalogSource = source.replace(marker, JSON.stringify(RUNTIME_EXTERNAL_ARTIFACT_NAMES));
  if (artifactCatalogSource === source) throw new Error("loader artifact marker was not embedded");
  new vm.Script(artifactCatalogSource, { filename: "incodex-loader.cts" }).runInContext(context);
  return { ...fixture, events, quitCalls: () => quitCalls, quitApp: electron.app.quit };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

async function nextTurn() {
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

describe("install permission preflight loader mode", () => {
  test("Darwin awaits the explicit probe and exits without requiring official main", async () => {
    const probe = deferred<void>();
    const fixture = loadActualBootstrap({
      platform: "darwin",
      env: { [PREFLIGHT_ENV]: "1" },
      runtime: ({ electron, events }) => ({
        runInstallPermissionProbe() {
          events.push("probe-started");
          return probe.promise.then(() => {
            events.push("probe-completed");
            electron.app.quit();
          });
        },
        startupGate: Promise.resolve(),
      }),
    });

    try {
      expect(fixture.events).not.toContain("official-main-required");
      expect(fixture.events).toEqual(["runtime-required", "probe-started"]);
      probe.resolve();
      await nextTurn();
      expect(fixture.events).toEqual(["runtime-required", "probe-started", "probe-completed", "probe-app-quit"]);
      expect(fixture.quitCalls()).toBe(1);
      expect(fixture.events).not.toContain("official-main-required");
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("a missing probe export never falls through to official main", async () => {
    const fixture = loadActualBootstrap({
      platform: "darwin",
      env: { [PREFLIGHT_ENV]: "1" },
      runtime: { startupGate: Promise.resolve() },
    });
    try {
      await nextTurn();
      expect(fixture.events).not.toContain("official-main-required");
      expect(fixture.events.filter((event) => event === "probe-app-quit")).toHaveLength(1);
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("a synchronous probe exception never falls through to official main", async () => {
    const fixture = loadActualBootstrap({
      platform: "darwin",
      env: { [PREFLIGHT_ENV]: "1" },
      runtime: {
        runInstallPermissionProbe() {
          throw new Error("isolated probe failure");
        },
      },
    });
    try {
      await nextTurn();
      expect(fixture.events).not.toContain("official-main-required");
      expect(fixture.events.filter((event) => event === "probe-app-quit")).toHaveLength(1);
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("a rejected probe promise never falls through to official main", async () => {
    const fixture = loadActualBootstrap({
      platform: "darwin",
      env: { [PREFLIGHT_ENV]: "1" },
      runtime: {
        runInstallPermissionProbe() {
          return Promise.reject(new Error("isolated probe rejection"));
        },
      },
    });
    try {
      await nextTurn();
      expect(fixture.events).not.toContain("official-main-required");
      expect(fixture.events.filter((event) => event === "probe-app-quit")).toHaveLength(1);
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("ordinary Darwin startup requires official main before awaiting Runtime", async () => {
    const startup = deferred<void>();
    let probeCalls = 0;
    const fixture = loadActualBootstrap({
      platform: "darwin",
      runtime: {
        startupGate: startup.promise,
        runInstallPermissionProbe() {
          probeCalls += 1;
          return Promise.resolve();
        },
      },
    });
    try {
      expect(fixture.events).toContain("official-main-required");
      expect(fixture.events).not.toContain("probe-app-quit");
      expect(probeCalls).toBe(0);
      startup.resolve();
      await nextTurn();
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("the explicit environment flag does not enable the probe on non-Darwin", async () => {
    let probeCalls = 0;
    const fixture = loadActualBootstrap({
      platform: "linux",
      env: { [PREFLIGHT_ENV]: "1" },
      runtime: {
        startupGate: Promise.resolve(),
        runInstallPermissionProbe() {
          probeCalls += 1;
          return Promise.resolve();
        },
      },
    });
    try {
      expect(fixture.events).toContain("official-main-required");
      expect(fixture.events).not.toContain("probe-app-quit");
      expect(probeCalls).toBe(0);
      await nextTurn();
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });
});

const APP_PATH = "/Applications/ChatGPT.app";
const APP_ASAR_PATH = join(APP_PATH, "Contents", "Resources", "app.asar");
const APP_EXECUTABLE_PATH = join(APP_PATH, "Contents", "MacOS", "ChatGPT");
const INSTALL_ID = "11111111-1111-4111-8111-111111111111";
const REQUEST_ID = "22222222-2222-4222-8222-222222222222";

function loadActualMainPreflight(options: { installId?: string; requestId?: string } = {}) {
  const root = mkdtempSync(join(tmpdir(), "incodex-main-preflight-"));
  const home = join(root, "home");
  const userRoot = join(home, ".incodex");
  const transactionRoot = join(userRoot, "transactions");
  const installRoot = join(transactionRoot, INSTALL_ID);
  for (const directory of [home, userRoot, transactionRoot, installRoot]) {
    mkdirSync(directory, { recursive: true, mode: 0o700 });
  }
  writeFileSync(join(installRoot, "accessibility-setup.json"), JSON.stringify({
    schemaVersion: 1,
    installId: INSTALL_ID,
    requestId: options.requestId ?? REQUEST_ID,
    appPath: APP_PATH,
    requestedAtMs: 1000,
    presentationOwner: "cli",
    state: "pending",
  }), { mode: 0o600 });

  const events: string[] = [];
  let screenReads = 0;
  const packageJson = JSON.stringify({ __incodex: { installId: options.installId ?? INSTALL_ID } });
  const packagePath = join(APP_ASAR_PATH, "package.json");
  const packageStatPath = join(root, "package-stub.json");
  writeFileSync(packageStatPath, packageJson, { mode: 0o600 });
  const actualFs = nodeFs;
  let watchCallback: (() => void) | undefined;
  let watchRegistrations = 0;
  let watchCloses = 0;
  const fsMock = new Proxy(actualFs, {
    get(target, property) {
      if (property === "watch") {
        return (_directory: string, _options: unknown, callback: () => void) => {
          watchRegistrations += 1;
          watchCallback = callback;
          return { on() { return this; }, close() { watchCloses += 1; watchCallback = undefined; } };
        };
      }
      if (property === "lstatSync") {
        return (file: nodeFs.PathLike) => file === packagePath
          ? actualFs.lstatSync(packageStatPath)
          : Reflect.apply(Reflect.get(target, property), target, [file]);
      }
      if (property === "readFileSync") {
        return (file: nodeFs.PathOrFileDescriptor, encoding?: BufferEncoding) => file === packagePath
          ? packageJson
          : Reflect.apply(Reflect.get(target, property), target,
            encoding === undefined ? [file] : [file, encoding]);
      }
      const value = Reflect.get(target, property);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const electron = {
    app: {
      getAppPath: () => APP_ASAR_PATH,
      setActivationPolicy: (policy: string) => events.push(`activation:${policy}`),
      quit: () => events.push("app-quit"),
    },
    systemPreferences: {
      getMediaAccessStatus: (kind: string) => { screenReads += 1; events.push(`screen:${kind}`); return "denied"; },
    },
  };
  const safeHome = {
    resolveSourceHome: (candidate: string | undefined, fallback: string) => candidate || fallback,
    isManagedSessionHome: () => false,
  };
  const instance = {
    targetIdFromExec: () => "preflight-target",
    targetStateDir: () => join(root, "target"),
    processIdentity: () => ({ processStartIdentity: "process-start-identity" }),
  };
  const macosUpdate = { prepareUpdateHandoff: () => events.push("update-handoff") };
  // 只控制通知输送；真实 probe 保留文件读取、nonce 校验和退休逻辑。
  const probeModule = { exports: {} as Record<string, unknown> };
  const probeSource = readFileSync(join(import.meta.dir, "runtime/incodex-permission-probe.cts"), "utf8")
    .replace(/^export \{ ([^\r\n]+) \};?\s*$/m, "module.exports = { $1 };");
  new Function("require", "module", "exports", probeSource)(
    (request: string) => request === "node:fs" ? fsMock : requireFixture(request),
    probeModule, probeModule.exports,
  );
  const runtimeRequire = (request: string): unknown => {
    if (request === "node:child_process") return { spawn: () => { throw new Error("unexpected spawn"); } };
    if (request === "node:fs") return fsMock;
    if (request === "node:os") return { homedir: () => home };
    if (request === "node:path") return nodePath;
    if (request === "./incodex-safe-home.cjs") return safeHome;
    if (request === "./incodex-ipc-guard.cjs") return {};
    if (request === "./incodex-instance.cjs") return instance;
    if (request === "./incodex-window-kind.cjs") return {};
    if (request === "./incodex-window-lifecycle.cjs") return {};
    if (request === "./incodex-codex-mode.cjs") {
      return { CODEX_MODE_FALLBACK_EXPRESSION: "", createCodexModeReadiness: () => ({}) };
    }
    if (request === "./incodex-runtime-load.cjs") return { loadRuntimeModule: () => macosUpdate };
    if (request === "./incodex-dock-menu.cjs") return {};
    if (request === "./incodex-permission-probe.cjs") return probeModule.exports;
    if (request === "electron") return electron;
    throw new Error(`unexpected main require: ${request}`);
  };
  const runtimeProcess = {
    env: {
      HOME: home,
      [PREFLIGHT_ENV]: JSON.stringify({ installId: INSTALL_ID, requestId: REQUEST_ID }),
    },
    platform: "darwin",
    execPath: APP_EXECUTABLE_PATH,
    pid: 4242,
    versions: { electron: "test-electron", chrome: "test-chrome" },
    getuid: () => process.getuid?.() ?? nodeFs.statSync(root).uid,
  };
  const module = { exports: {} as Record<string, unknown> };
  const source = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8")
    .replace('const permissionProbe = "__INCODEX_PERMISSION_PROBE__";', "const permissionProbe = require(\"./incodex-permission-probe.cjs\");");
  new Function("require", "module", "exports", "process", "__dirname", "Buffer", source)(
    runtimeRequire, module, module.exports, runtimeProcess, root, Buffer,
  );
  return {
    root,
    events,
    runtime: module.exports,
    requestPath: join(installRoot, "permission-probe-request.json"),
    responsePath: join(installRoot, "permission-probe-response.json"),
    screenReads: () => screenReads,
    watchState: () => ({ registrations: watchRegistrations, closes: watchCloses }),
    notifyWatch: () => {
      if (!watchCallback) throw new Error("preflight watcher is not active");
      watchCallback();
    },
  };
}

describe("install permission preflight Runtime entry", () => {
  test("the one-shot Runtime uses the real responder and exits only after the CLI retires its request", async () => {
    const fixture = loadActualMainPreflight();
    let probeFinished = false;
    let probeError: unknown;
    try {
      await (fixture.runtime.startupGate as Promise<void>);
      expect(fixture.events).toEqual([]);
      const run = fixture.runtime.runInstallPermissionProbe as () => Promise<void>;
      const probe = run();
      const completion = probe.then(() => { probeFinished = true; }, (error) => { probeError = error; });
      expect(fixture.events).toEqual(["activation:prohibited"]);
      expect(fixture.screenReads()).toBe(0);
      expect(fixture.watchState()).toEqual({ registrations: 1, closes: 0 });
      const nonce = "a".repeat(32);
      writeFileSync(fixture.requestPath, `${JSON.stringify({
        schemaVersion: 1,
        nonce,
        installId: INSTALL_ID,
        requestId: REQUEST_ID,
        appPath: APP_PATH,
        pid: 4242,
        processStartIdentity: "process-start-identity",
        requestedAtMs: Date.now(),
      })}\n`, { mode: 0o600 });
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(existsSync(fixture.responsePath)).toBe(false);
      expect(fixture.screenReads()).toBe(0);
      expect(probeFinished).toBe(false);
      expect(fixture.events).toEqual(["activation:prohibited"]);
      fixture.notifyWatch();
      const deadline = Date.now() + 5_000;
      while (!existsSync(fixture.responsePath) && Date.now() < deadline) await Bun.sleep(10);
      expect(existsSync(fixture.responsePath)).toBe(true);
      expect(JSON.parse(readFileSync(fixture.responsePath, "utf8"))).toMatchObject({
        nonce,
        requestId: REQUEST_ID,
        rawStatus: "denied",
        status: "not-granted",
      });
      expect(fixture.screenReads()).toBe(1);
      expect(probeFinished).toBe(false);
      unlinkSync(fixture.requestPath);
      unlinkSync(fixture.responsePath);
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(probeFinished).toBe(false);
      expect(fixture.events).toEqual(["activation:prohibited", "screen:screen"]);
      fixture.notifyWatch();
      await completion;
      expect(probeError).toBeUndefined();
      expect(probeFinished).toBe(true);
      expect(fixture.events).toEqual(["activation:prohibited", "screen:screen", "app-quit"]);
      expect(fixture.watchState()).toEqual({ registrations: 1, closes: 1 });
    } finally {
      try { unlinkSync(fixture.requestPath); } catch {}
      try { unlinkSync(fixture.responsePath); } catch {}
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("the Runtime refuses a preflight request for another installed identity", async () => {
    const fixture = loadActualMainPreflight({ installId: "33333333-3333-4333-8333-333333333333" });
    try {
      await (fixture.runtime.startupGate as Promise<void>);
      const run = fixture.runtime.runInstallPermissionProbe as () => Promise<void>;
      expect(run).toThrow("identity mismatch");
      expect(fixture.events).toEqual([]);
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("the Runtime refuses a request ID that differs from the private CLI marker", async () => {
    const fixture = loadActualMainPreflight({ requestId: "33333333-3333-4333-8333-333333333333" });
    try {
      await (fixture.runtime.startupGate as Promise<void>);
      const run = fixture.runtime.runInstallPermissionProbe as () => Promise<void>;
      expect(run).toThrow("marker mismatch");
      expect(fixture.events).toEqual([]);
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });
});

function createRequestRemovalWatcherFixture(options: {
  lstatErrorCode?: string;
  existsSyncReturnsFalse?: boolean;
} = {}) {
  const root = mkdtempSync(join(tmpdir(), "incodex-request-removal-watch-"));
  const home = join(root, "home");
  const installDirectory = join(home, ".incodex", "transactions", INSTALL_ID);
  mkdirSync(installDirectory, { recursive: true, mode: 0o700 });
  const requestPath = join(installDirectory, "permission-probe-request.json");
  let callback: ((event: string, filename: string | null) => void) | undefined;
  const lstatOutcomes: string[] = [];
  const records: string[] = [];
  const fileSystem = new Proxy(nodeFs, {
    get(target, property) {
      if (property === "watch") {
        return (_path: nodeFs.PathLike, ...args: unknown[]) => {
          for (let index = args.length - 1; index >= 0; index -= 1) {
            if (typeof args[index] === "function") { callback = args[index] as typeof callback; break; }
          }
          return { close() {} } as nodeFs.FSWatcher;
        };
      }
      if (property === "lstatSync") {
        return (file: nodeFs.PathLike, ...args: unknown[]) => {
          if (nodePath.resolve(String(file)) === requestPath && options.lstatErrorCode) {
            lstatOutcomes.push(options.lstatErrorCode);
            throw Object.assign(new Error("test lstat failure"), { code: options.lstatErrorCode });
          }
          try {
            const result = Reflect.apply(Reflect.get(target, property), target, [file, ...args]);
            if (nodePath.resolve(String(file)) === requestPath) lstatOutcomes.push("present");
            return result;
          } catch (error) {
            if (nodePath.resolve(String(file)) === requestPath) {
              lstatOutcomes.push((error as NodeJS.ErrnoException).code ?? "unknown");
            }
            throw error;
          }
        };
      }
      if (property === "existsSync" && options.existsSyncReturnsFalse) {
        return (file: nodeFs.PathLike) => nodePath.resolve(String(file)) === requestPath
          ? false
          : nodeFs.existsSync(file);
      }
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
  const guardedFs = createGuardedFsForTest({
    home,
    virtualPackage: join(home, "package.json"),
    installDirectory,
    receipt: join(home, "receipt.jsonl"),
    record: (event: string) => records.push(event),
    mode: "probe",
    processStartIdentity: "test-process-start",
    fileSystem,
  });
  guardedFs.watch(installDirectory, () => {});
  return {
    root,
    requestPath,
    lstatOutcomes,
    records,
    observed: () => guardedFs.requestRemovalObserved,
    readRequest: () => guardedFs.lstatSync(requestPath),
    emit: (event: string, filename: string | null) => {
      if (!callback) throw new Error("watch callback was not captured");
      callback(event, filename);
    },
  };
}

describe("install permission child fixture request retirement observation", () => {
  test("uses the guarded request lstat instead of the watcher filename", () => {
    const fixture = createRequestRemovalWatcherFixture();
    try {
      writeFileSync(fixture.requestPath, "private request fixture\n", { mode: 0o600 });
      fixture.emit("rename", ".permission-probe-request.123.tmp");
      expect(fixture.observed()).toBe(false);
      fixture.readRequest();
      expect(fixture.observed()).toBe(false);
      expect(fixture.lstatOutcomes).toEqual(["present"]);

      unlinkSync(fixture.requestPath);
      fixture.emit("rename", ".permission-probe-request.123.tmp");
      expect(fixture.observed()).toBe(false);
      expect(() => fixture.readRequest()).toThrow();
      expect(fixture.lstatOutcomes).toEqual(["present", "ENOENT"]);
      expect(fixture.observed()).toBe(true);
      expect(fixture.records.filter((event) => event === "request-removal-observed"))
        .toEqual(["request-removal-observed"]);
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("does not treat a non-ENOENT lstat failure as request removal", () => {
    const fixture = createRequestRemovalWatcherFixture({
      lstatErrorCode: "EACCES",
      existsSyncReturnsFalse: true,
    });
    try {
      fixture.emit("rename", "permission-probe-request.json");
      expect(() => fixture.readRequest()).toThrow("test lstat failure");
      expect(fixture.lstatOutcomes).toEqual(["EACCES"]);
      expect(fixture.observed()).toBe(false);
      expect(fixture.records.filter((event) => event === "request-removal-observed")).toEqual([]);
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });
});
