/**
 * [INPUT]: 接收私有 HOME、安装身份、探针状态、运行模式与 dist 路径。
 * [OUTPUT]: 执行真实 dist Loader 与探针；仅由实际lstat缺席记录请求移除。
 * [POS]: Rust 安装前置回归夹具；仅映射 TCC 与 App 身份边界，不启动真实 App。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
"use strict";

// ---- 固定身份只用于 VM 内的逻辑校验，不访问这些系统路径。 ----
const APP_PATH = "/Applications/ChatGPT.app";
const APP_ASAR_PATH = `${APP_PATH}/Contents/Resources/app.asar`;
const APP_ASAR_PACKAGE_PATH = `${APP_ASAR_PATH}/package.json`;
const APP_EXECUTABLE_PATH = `${APP_PATH}/Contents/MacOS/ChatGPT`;
const ORIGINAL_MAIN = "official-main.cjs";
const RUNTIME_VERSION = "1.0.0";
const PREFLIGHT_ENV = "INCODEX_INTERNAL_PERMISSION_PREFLIGHT";
const UUID = /^[a-f0-9]{8}-[a-f0-9]{4}-[1-5][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/i;
const SCREEN_STATUSES = new Set(["granted", "denied", "not-determined", "restricted", "unknown"]);

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const vm = require("node:vm");
const os = require("node:os");
const ACCOUNT_HOME = path.resolve(os.userInfo().homedir);

function fail(message) {
  throw new Error(`[fixture] ${message}`);
}

function assert(condition, message) {
  if (!condition) fail(message);
}

function isAbsolutePath(value) {
  return typeof value === "string" && value.length > 0 && path.isAbsolute(value);
}

function readConfig() {
  const configPath = process.argv[2];
  assert(isAbsolutePath(configPath), "argv[2] must name an absolute config path");
  const config = JSON.parse(fs.readFileSync(configPath, "utf8"));
  const required = ["distDirectory", "home", "installId", "mode", "receipt", "requestId", "screenStatus"].sort();
  assert(config && typeof config === "object" && !Array.isArray(config), "config must be an object");
  assert(Object.keys(config).sort().join(",") === required.join(","), "config fields do not match the fixture contract");
  assert(isAbsolutePath(config.home), "home must be absolute");
  assert(isAbsolutePath(config.distDirectory), "distDirectory must be absolute");
  assert(isAbsolutePath(config.receipt), "receipt must be absolute");
  const home = path.resolve(config.home);
  assert(home !== ACCOUNT_HOME && !home.startsWith(`${ACCOUNT_HOME}${path.sep}`), "fixture HOME must not use the account home");
  assert(!home.startsWith(`${APP_PATH}${path.sep}`), "fixture HOME must not use the official App path");
  assert(!path.resolve(config.receipt).startsWith(`${APP_PATH}${path.sep}`), "receipt must not target the official App path");
  assert(UUID.test(config.installId), "installId must be a UUID");
  assert(UUID.test(config.requestId), "requestId must be a UUID");
  assert(config.mode === "probe" || config.mode === "normal", "mode must be probe or normal");
  assert(SCREEN_STATUSES.has(config.screenStatus), "screenStatus is outside the closed fixture set");
  return {
    ...config,
    home: path.resolve(config.home),
    distDirectory: fs.realpathSync(config.distDirectory),
    receipt: path.resolve(config.receipt),
  };
}

function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function writeNewFile(file, bytes, mode = 0o600) {
  const descriptor = fs.openSync(file, fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL, mode);
  try {
    fs.writeFileSync(descriptor, bytes);
    fs.fsyncSync(descriptor);
  } finally {
    fs.closeSync(descriptor);
  }
}

function writeNewAtomic(file, bytes) {
  const temporary = path.join(path.dirname(file), `.${path.basename(file)}.${process.pid}.tmp`);
  writeNewFile(temporary, bytes);
  try {
    fs.linkSync(temporary, file);
  } finally {
    try { fs.unlinkSync(temporary); } catch {}
  }
}

function assertPrivateDirectory(directory, uid, label) {
  const stat = fs.lstatSync(directory);
  assert(stat.isDirectory() && !stat.isSymbolicLink(), `${label} must be a real directory`);
  assert(stat.uid === uid && (stat.mode & 0o077) === 0, `${label} must be owned and private`);
}

function assertPrivateFile(file, uid, label) {
  const stat = fs.lstatSync(file);
  assert(stat.isFile() && !stat.isSymbolicLink(), `${label} must be a regular file`);
  assert(stat.uid === uid && (stat.mode & 0o077) === 0, `${label} must be owned and private`);
}

function createReceipt(file, mode) {
  const parent = path.dirname(file);
  const parentStat = fs.lstatSync(parent);
  assert(parentStat.isDirectory() && !parentStat.isSymbolicLink(), "receipt parent must be a real directory");
  assert(parentStat.uid === process.getuid() && (parentStat.mode & 0o077) === 0,
    "receipt parent must be private and belong to the fixture user");
  const descriptor = fs.openSync(file,
    fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL | fs.constants.O_APPEND,
    0o600);
  let closed = false;
  function record(event, details = {}) {
    if (closed) return;
    const value = {
      schemaVersion: 1,
      event,
      mode,
      pid: process.pid,
      observedAtMs: Date.now(),
      ...details,
    };
    fs.writeSync(descriptor, `${JSON.stringify(value)}\n`);
    fs.fsyncSync(descriptor);
  }
  function close() {
    if (closed) return;
    closed = true;
    fs.closeSync(descriptor);
  }
  return { record, close };
}

function sanitizeEnvironment(home) {
  for (const key of Object.keys(process.env)) delete process.env[key];
  Object.assign(process.env, {
    HOME: home,
    TMPDIR: path.join(home, "tmp"),
    PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
    LC_ALL: "C",
  });
  process.umask(0o077);
}

function prepareRuntime(home, distDirectory) {
  const catalogPath = path.join(path.dirname(distDirectory), "runtime-artifacts.json");
  const catalog = JSON.parse(fs.readFileSync(catalogPath, "utf8"));
  assert(catalog.loader === "incodex-loader.cjs", "dist catalog loader name changed");
  assert(Array.isArray(catalog.external) && catalog.external.length > 0, "dist catalog has no external files");

  const sources = new Map();
  for (const name of catalog.external) {
    assert(typeof name === "string" && path.basename(name) === name, "catalog contains a non-basename artifact");
    const sourcePath = path.join(distDirectory, name);
    const sourceStat = fs.lstatSync(sourcePath);
    assert(sourceStat.isFile() && !sourceStat.isSymbolicLink(), `dist artifact ${name} must be a regular file`);
    sources.set(name, fs.readFileSync(sourcePath));
  }

  const files = Object.fromEntries([...sources].map(([name, bytes]) => [name, sha256(bytes)]));
  const manifest = Buffer.from(`${JSON.stringify({ runtimeVersion: RUNTIME_VERSION, sourceCommit: "", files })}\n`);
  const manifestSha256 = sha256(manifest);
  const runtimeRoot = path.join(home, ".incodex", "runtime");
  assert(!fs.existsSync(runtimeRoot), "runtime root already exists; fixture refuses to replace it");
  fs.mkdirSync(runtimeRoot, { recursive: true, mode: 0o700 });
  const releases = path.join(runtimeRoot, "releases");
  fs.mkdirSync(releases, { mode: 0o700 });
  const release = `releases/${RUNTIME_VERSION}-${manifestSha256}`;
  const releaseDirectory = path.join(runtimeRoot, release);
  fs.mkdirSync(releaseDirectory, { mode: 0o700 });
  for (const [name, bytes] of sources) writeNewFile(path.join(releaseDirectory, name), bytes);
  writeNewFile(path.join(releaseDirectory, "runtime-manifest.json"), manifest);
  const current = Buffer.from(`${JSON.stringify({
    schemaVersion: 1,
    version: RUNTIME_VERSION,
    sourceCommit: "",
    release,
    manifestSha256,
    files,
  })}\n`);
  writeNewAtomic(path.join(runtimeRoot, "current.json"), current);
  return { releaseDirectory, loaderPath: path.join(distDirectory, catalog.loader) };
}

function prepareBundleMetadata(home, installId) {
  const bundleDirectory = path.join(home, ".fixture", "app.asar.unpacked");
  fs.mkdirSync(bundleDirectory, { recursive: true, mode: 0o700 });
  writeNewFile(path.join(bundleDirectory, "package.json"), Buffer.from(`${JSON.stringify({
    __incodex: { originalMain: ORIGINAL_MAIN },
  })}\n`));

  const virtualPackage = path.join(home, ".fixture", "asar-package.json");
  writeNewFile(virtualPackage, Buffer.from(`${JSON.stringify({
    __incodex: { installId },
  })}\n`));
  return { bundleDirectory, virtualPackage };
}

function diagnosticError(value, config) {
  const name = value && typeof value.name === "string" ? value.name : "Error";
  const replacePrivatePaths = (text) => String(text)
    .replaceAll(config.home, "<fixture-home>")
    .replaceAll(config.distDirectory, "<dist>")
    .replaceAll(APP_PATH, "<logical-app>");
  const message = value && typeof value.message === "string" ? value.message : String(value);
  const stack = value && typeof value.stack === "string" ? value.stack.split("\n").slice(0, 8).join("\n") : "";
  return {
    name: name.slice(0, 80),
    message: replacePrivatePaths(message).slice(0, 1200),
    ...(stack ? { stack: replacePrivatePaths(stack).slice(0, 2400) } : {}),
  };
}

function attachProbeErrorReceipt(runtime, config, record) {
  if (typeof runtime?.runInstallPermissionProbe !== "function") return runtime;
  const runProbe = runtime.runInstallPermissionProbe;
  runtime.runInstallPermissionProbe = function (...args) {
    let result;
    try {
      result = Reflect.apply(runProbe, this, args);
    } catch (error) {
      record("probe-threw", { error: diagnosticError(error, config) });
      throw error;
    }
    if (!result || typeof result.then !== "function") return result;
    return result.catch((error) => {
      record("probe-rejected", { error: diagnosticError(error, config) });
      throw error;
    });
  };
  return runtime;
}

function pathValue(value) {
  if (typeof value === "string") return path.resolve(value);
  if (Buffer.isBuffer(value)) return path.resolve(value.toString());
  if (value instanceof URL) return path.resolve(require("node:url").fileURLToPath(value));
  return null;
}

function createGuardedFs(options) {
  const { home, virtualPackage, installDirectory, receipt, record } = options;
  const fixtureFs = options.fileSystem || fs;
  const virtualPackageRealPath = path.resolve(virtualPackage);
  const installDirectoryPath = path.resolve(installDirectory);
  const receiptPath = path.resolve(receipt);
  const ownedDescriptors = new Set();
  let probeReadyRecorded = false;
  let requestRemovalObserved = false;
  const requestPath = path.join(installDirectoryPath, "permission-probe-request.json");

  function guardPath(value, allowVirtualPackageRead = false) {
    const normalized = pathValue(value);
    if (!normalized) return null;
    if (normalized === APP_ASAR_PACKAGE_PATH && allowVirtualPackageRead) return virtualPackageRealPath;
    if (normalized === APP_PATH || normalized.startsWith(`${APP_PATH}/`) || normalized.startsWith("/Applications/")) {
      fail("fixture refused physical access to /Applications");
    }
    const allowed = normalized === home || normalized.startsWith(`${home}${path.sep}`) ||
      normalized === receiptPath || normalized.startsWith(`${path.dirname(receiptPath)}${path.sep}`);
    if (!allowed) fail("fixture refused filesystem access outside owned roots");
    return normalized;
  }

  return new Proxy(fixtureFs, {
    get(target, property) {
      if (property === "requestRemovalObserved") return requestRemovalObserved;
      if (property === "promises") fail("fixture refuses unguarded fs.promises access");
      if (property === "openSync") {
        return (file, ...args) => {
          const descriptor = Reflect.apply(target.openSync, target, [guardPath(file), ...args]);
          ownedDescriptors.add(descriptor);
          return descriptor;
        };
      }
      if (property === "lstatSync") {
        return (file, ...args) => {
          const normalized = guardPath(file, true);
          try {
            return Reflect.apply(target.lstatSync, target, [normalized, ...args]);
          } catch (error) {
            if (options.mode === "probe" && !requestRemovalObserved && normalized === requestPath &&
                error?.code === "ENOENT") {
              requestRemovalObserved = true;
              record("request-removal-observed", { processStartIdentity: options.processStartIdentity });
            }
            throw error;
          }
        };
      }
      if (property === "readFileSync") {
        return (file, ...args) => {
          if (typeof file === "number") assert(ownedDescriptors.has(file), "fixture rejected an unowned file descriptor");
          const guardedFile = typeof file === "number" ? file : guardPath(file, true);
          return Reflect.apply(target.readFileSync, target, [guardedFile, ...args]);
        };
      }
      if (property === "existsSync") {
        return (file) => Reflect.apply(target.existsSync, target, [guardPath(file, true)]);
      }
      if (property === "watch") {
        return (file, ...args) => {
          const normalized = guardPath(file);
          const watcher = Reflect.apply(target.watch, target, [normalized, ...args]);
          if (options.mode === "probe" && normalized === installDirectoryPath && !probeReadyRecorded) {
            probeReadyRecorded = true;
            record("probe-ready", { processStartIdentity: options.processStartIdentity });
          }
          return watcher;
        };
      }
      const value = Reflect.get(target, property, target);
      if (typeof value !== "function") return value;
      return (...args) => {
        const method = String(property);
        if (["closeSync", "fstatSync", "fsyncSync", "readSync", "writeSync"].includes(method)) {
          assert(Number.isInteger(args[0]) && ownedDescriptors.has(args[0]), "fixture rejected an unowned file descriptor");
          const result = Reflect.apply(value, target, args);
          if (method === "closeSync") ownedDescriptors.delete(args[0]);
          return result;
        }
        const pathArgumentIndexes = ["copyFileSync", "linkSync", "renameSync"].includes(method) ? [0, 1] : [0];
        for (const index of pathArgumentIndexes) {
          if (index < args.length && pathValue(args[index]) !== null) args[index] = guardPath(args[index]);
          else if (index === 0 && ["readFileSync", "writeFileSync"].includes(method) &&
              Number.isInteger(args[index]) && ownedDescriptors.has(args[index])) continue;
          else if (index < args.length && args[index] !== undefined && args[index] !== null) {
            fail("fixture rejected a non-path filesystem argument");
          }
        }
        return Reflect.apply(value, target, args);
      };
    },
  });
}

function executeCommonJs(bytes, filename, requireModule, processShim, dirname, consoleShim) {
  const module = { exports: {} };
  const context = vm.createContext({
    Buffer,
    Date,
    Promise,
    setTimeout,
    clearTimeout,
    setInterval,
    clearInterval,
    console: consoleShim,
  });
  const wrapper = new vm.Script(
    `(function(require, module, exports, process, __dirname, Buffer) {\n${bytes.toString("utf8")}\n})`,
    { filename },
  ).runInContext(context);
  wrapper(requireModule, module, module.exports, processShim, dirname, Buffer);
  return module.exports;
}

function runFixture() {
  const config = readConfig();
  sanitizeEnvironment(config.home);
  assertPrivateDirectory(config.home, process.getuid(), "fixture HOME");
  const receipt = createReceipt(config.receipt, config.mode);
  let loaderErrors = 0;
  let screenReads = 0;
  let appQuitCalls = 0;
  let processStartIdentity = null;

  process.on("exit", (code) => {
    try { receipt.record("process-exit", { code, loaderErrors, screenReads, appQuitCalls, processStartIdentity }); } catch {}
    receipt.close();
  });

  try {
    const incodexRoot = path.join(config.home, ".incodex");
    const transactionRoot = path.join(incodexRoot, "transactions");
    const installDirectory = path.join(transactionRoot, config.installId);
    if (config.mode === "probe") {
      assertPrivateDirectory(incodexRoot, process.getuid(), "Incodex root");
      assertPrivateDirectory(transactionRoot, process.getuid(), "transaction root");
      assertPrivateDirectory(installDirectory, process.getuid(), "install transaction directory");
      assertPrivateFile(path.join(installDirectory, "accessibility-setup.json"), process.getuid(), "CLI permission marker");
    }

    fs.mkdirSync(path.join(config.home, "tmp"), { recursive: true, mode: 0o700 });
    const { releaseDirectory, loaderPath } = prepareRuntime(config.home, config.distDirectory);
    const { bundleDirectory, virtualPackage } = prepareBundleMetadata(config.home, config.installId);
    const instance = require(path.join(config.distDirectory, "incodex-instance.cjs"));
    const liveIdentity = instance.processIdentity(process.pid);
    assert(liveIdentity && liveIdentity.pid === process.pid && liveIdentity.processStartIdentity,
      "current process identity is unavailable");
    processStartIdentity = liveIdentity.processStartIdentity;
    receipt.record("child-start", { processStartIdentity });
    const fsForVm = createGuardedFs({
      home: config.home,
      virtualPackage,
      installDirectory,
      receipt: config.receipt,
      record: receipt.record,
      mode: config.mode,
      processStartIdentity,
    });

    const app = {
      getAppPath: () => APP_ASAR_PATH,
      quit: () => {
        appQuitCalls += 1;
        receipt.record(config.mode === "probe" ? "probe-quit" : "app-quit", {
          screenReads, loaderErrors, requestRemovalObserved: fsForVm.requestRemovalObserved === true,
          processStartIdentity,
        });
      },
    };
    const electron = {
      app,
      systemPreferences: {
        getMediaAccessStatus(kind) {
          assert(kind === "screen", "fixture only accepts screen permission reads");
          screenReads += 1;
          receipt.record("screen-status-read", { screenReads, processStartIdentity });
          return config.screenStatus;
        },
      },
    };
    const processShim = {
      env: {
        HOME: config.home,
        ...(config.mode === "probe" ? {
          [PREFLIGHT_ENV]: JSON.stringify({ installId: config.installId, requestId: config.requestId }),
        } : {}),
      },
      platform: "darwin",
      execPath: APP_EXECUTABLE_PATH,
      pid: process.pid,
      versions: { electron: "isolated-fixture", chrome: "isolated-fixture" },
      getuid: () => process.getuid(),
    };
    const safeHome = {
      resolveSourceHome: (candidate, fallback) => candidate || fallback,
      isManagedSessionHome: () => false,
    };
    const vmConsole = {
      error: (...values) => {
        loaderErrors += 1;
        process.exitCode = 1;
        receipt.record("runtime-error", { errors: values.map((value) => diagnosticError(value, config)) });
      },
      warn() {},
      log() {},
    };
    const runtimeLoad = {
      loadRuntimeModule: () => ({}),
      readRuntimeJson: (name, directory) => JSON.parse(
        fsForVm.readFileSync(path.join(directory, name), "utf8"),
      ),
    };
    const codexModePath = path.join(releaseDirectory, "incodex-codex-mode.cjs");
    const codexMode = executeCommonJs(
      fsForVm.readFileSync(codexModePath),
      codexModePath,
      (request) => fail(`unexpected Codex-mode dependency ${request}`),
      processShim,
      releaseDirectory,
      vmConsole,
    );
    const mainRequire = (request) => {
      if (request === "node:child_process") return { spawn: () => { throw new Error("fixture forbids child launch"); } };
      if (request === "node:fs") return fsForVm;
      if (request === "node:os") return { homedir: () => config.home, tmpdir: () => path.join(config.home, "tmp") };
      if (request === "node:path") return path;
      if (request === "./incodex-safe-home.cjs") return safeHome;
      if (request === "./incodex-ipc-guard.cjs") return {};
      if (request === "./incodex-instance.cjs") return instance;
      if (request === "./incodex-window-kind.cjs" || request === "./incodex-window-lifecycle.cjs") return {};
      if (request === "./incodex-codex-mode.cjs") return codexMode;
      if (request === "./incodex-runtime-load.cjs") return runtimeLoad;
      if (request === "./incodex-dock-menu.cjs") return {};
      if (request === "electron") return electron;
      throw new Error("fixture rejected an unexpected main dependency");
    };

    const mainPath = path.join(releaseDirectory, "incodex-main.cjs");
    const mainBytes = fs.readFileSync(mainPath);
    const originalMainPath = path.join(bundleDirectory, ORIGINAL_MAIN);
    const loaderRequire = (request) => {
      if (request === "node:crypto") return crypto;
      if (request === "node:fs") return fsForVm;
      if (request === "node:path") return path;
      if (request === "electron") return electron;
      if (request === originalMainPath) {
        if (config.mode !== "normal") {
          receipt.record("unexpected-official-main");
          process.exitCode = 1;
          throw new Error("probe profile reached the official main entry");
        }
        receipt.record("normal-open", { processStartIdentity });
        return {};
      }
      if (request === mainPath) {
        receipt.record("runtime-required", { processStartIdentity });
        if (config.mode === "normal") return { startupGate: Promise.resolve() };
        try {
          const runtime = executeCommonJs(mainBytes, mainPath, mainRequire, processShim, releaseDirectory, vmConsole);
          return attachProbeErrorReceipt(runtime, config, receipt.record);
        } catch (error) {
          receipt.record("runtime-main-threw", { error: diagnosticError(error, config) });
          throw error;
        }
      }
      throw new Error("fixture rejected an unexpected loader dependency");
    };

    const loaderBytes = fs.readFileSync(loaderPath);
    executeCommonJs(loaderBytes, loaderPath, loaderRequire, processShim, bundleDirectory, vmConsole);
    receipt.record("loader-evaluated", { processStartIdentity });
  } catch (error) {
    loaderErrors += 1;
    process.exitCode = 1;
    try { receipt.record("fixture-error", { error: diagnosticError(error, config) }); } catch {}
  }
}

if (require.main === module) {
  runFixture();
} else {
  module.exports = { createGuardedFsForTest: createGuardedFs };
}
