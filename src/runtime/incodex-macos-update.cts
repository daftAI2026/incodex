/**
 * [INPUT]: 依赖已校验的 macOS 更新资产、Coordinator ready 文件与官方 Sparkle/objc-js 原生能力
 * [OUTPUT]: 对外提供同步 prepareUpdateHandoff，在官方 main 初始化前完成有界的更新恢复布防
 * [POS]: runtime 的 Sparkle 交接边界；失败时放弃自动恢复，但绝不跨事件循环破坏官方启动时序
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
// @ts-nocheck
"use strict";

const { spawn } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const REGISTRATION_SCHEMA_VERSION = 2;
const HELPER_FILE_NAME = "incodex";
const COORDINATOR_FILE_NAME = "incodex-update-coordinator";
const INTERPOSER_FILE_NAME = "libincodex-sparkle-interpose.dylib";
const KEYCHAIN_HELPER_FILE_NAME = "incodex-keychain-helper";
const KEYCHAIN_PROOF_FILE_NAME = "authorization-proof.json";
const READY_TIMEOUT_MS = 1_000;
const READY_POLL_MS = 10;
const KEYCHAIN_PROBE_TIMEOUT_MS = 2_000;
const KEYCHAIN_PROBE_KILL_GRACE_MS = 500;
const WAIT_CELL = new Int32Array(new SharedArrayBuffer(Int32Array.BYTES_PER_ELEMENT));

function prepareUpdateHandoff(options = {}) {
  if ((options.platform || process.platform) !== "darwin") return false;

  let child = null;
  try {
    const userRoot = path.resolve(options.userRoot);
    const execPath = path.resolve(options.execPath || process.execPath);
    const pid = Number(options.pid || process.pid);
    if (!Number.isSafeInteger(pid) || pid <= 0) return false;

    const registration = readRegistration(userRoot);
    if (!registration || !matchesCurrentApp(registration, execPath)) return false;
    if (!verifiedRegistrationAssets(userRoot, registration)) return false;

    const readyPath = path.join(
      userRoot,
      "macos-update",
      `ready-${pid}-${crypto.randomBytes(8).toString("hex")}.json`,
    );
    const spawnProcess = options.spawnProcess || spawn;
    const pendingPath = path.join(userRoot, "macos-update", "pending.json");
    const handoffId = crypto.randomBytes(16).toString("hex");
    child = spawnProcess(registration.coordinatorPath, [], {
      detached: true,
      stdio: "ignore",
      env: {
        ...process.env,
        INCODEX_MACOS_UPDATE_COORDINATOR: "1",
        INCODEX_MACOS_UPDATE_HOST_PID: String(pid),
        INCODEX_MACOS_UPDATE_INSTALL_ID: registration.installId,
        INCODEX_MACOS_UPDATE_HOST_APP: registration.appPath,
        INCODEX_MACOS_UPDATE_HELPER_PATH: registration.helperPath,
        INCODEX_MACOS_UPDATE_HELPER_SHA256: registration.helperSha256,
        INCODEX_MACOS_UPDATE_HANDOFF_ID: handoffId,
        INCODEX_MACOS_UPDATE_READY_PATH: readyPath,
        INCODEX_MACOS_UPDATE_PENDING_PATH: pendingPath,
      },
    });
    const waitForCoordinator = options.waitForCoordinator || waitForReadyFile;
    if (!waitForCoordinator(readyPath, child)) {
      stopChild(child);
      return false;
    }

    const sparklePath = path.join(
      registration.appPath,
      "Contents",
      "Resources",
      "native",
      "sparkle.node",
    );
    const requireSparkle = options.requireSparkle || require;
    requireSparkle(sparklePath);

    process.env.INCODEX_MACOS_UPDATE_HOST_APP = registration.appPath;
    process.env.INCODEX_MACOS_UPDATE_COORDINATOR_APP = registration.coordinatorAppPath;
    const loadInterposer = options.loadInterposer || defaultLoadInterposer;
    loadInterposer(registration.interposerPath, registration.appPath);
    delete process.env.INCODEX_MACOS_UPDATE_HOST_APP;
    delete process.env.INCODEX_MACOS_UPDATE_COORDINATOR_APP;
    removeReadyFile(readyPath);
    child.unref?.();
    return true;
  } catch {
    delete process.env.INCODEX_MACOS_UPDATE_HOST_APP;
    delete process.env.INCODEX_MACOS_UPDATE_COORDINATOR_APP;
    stopChild(child);
    return false;
  }
}

function probeKeychainAuthorizationReadiness(options = {}) {
  if ((options.platform || process.platform) !== "darwin" || options.incognito) return false;

  try {
    const userRoot = path.resolve(options.userRoot);
    const execPath = path.resolve(options.execPath || process.execPath);
    const registration = readKeychainRegistration(userRoot);
    if (!registration || registration.authorizationReady) return false;
    if (!matchesCurrentApp(registration, execPath)) return false;
    if (!verifiedKeychainHelper(userRoot, registration)) return false;
    if (hasMatchingAuthorizationProof(userRoot, registration)) return false;

    const spawnProcess = options.spawnProcess || spawn;
    const child = spawnProcess(registration.helperPath, [], {
      detached: true,
      stdio: "ignore",
    });
    let finished = false;
    let killTimer = null;
    const timeoutMs = options.timeoutMs || KEYCHAIN_PROBE_TIMEOUT_MS;
    const timeout = setTimeout(() => {
      if (finished) return;
      try {
        child.kill("SIGTERM");
      } catch {
        // The helper may already have exited between the timer and the signal.
      }
      killTimer = setTimeout(() => {
        if (finished) return;
        try {
          child.kill("SIGKILL");
        } catch {
          // A completed helper needs no final cleanup.
        }
      }, KEYCHAIN_PROBE_KILL_GRACE_MS);
      killTimer.unref?.();
    }, timeoutMs);
    timeout.unref?.();

    child.once("error", () => {
      finished = true;
      clearTimeout(timeout);
      if (killTimer) clearTimeout(killTimer);
    });
    child.once("close", (code, signal) => {
      finished = true;
      clearTimeout(timeout);
      if (killTimer) clearTimeout(killTimer);
      if (code === 0 && signal == null) {
        writeAuthorizationProof(userRoot, registration);
      }
    });
    child.unref?.();
    return true;
  } catch {
    return false;
  }
}

function readKeychainRegistration(userRoot) {
  const file = path.join(userRoot, "macos-keychain", "registration.json");
  const descriptor = openRegularNoFollow(file);
  if (descriptor === null) return null;
  try {
    const metadata = fs.fstatSync(descriptor);
    if (!isPrivateFile(metadata, 0o600)) return null;
    const registration = JSON.parse(fs.readFileSync(descriptor, "utf8"));
    if (registration?.schemaVersion !== 1) return null;
    if (typeof registration.appPath !== "string" || !path.isAbsolute(registration.appPath)) {
      return null;
    }
    if (typeof registration.helperPath !== "string" || !path.isAbsolute(registration.helperPath)) {
      return null;
    }
    if (!/^[0-9a-f]{64}$/.test(registration.helperSha256)) return null;
    if (typeof registration.authorizationReady !== "boolean") return null;
    return registration;
  } catch {
    return null;
  } finally {
    fs.closeSync(descriptor);
  }
}

function verifiedKeychainHelper(userRoot, registration) {
  const expected = path.join(
    userRoot,
    "helpers",
    "macos-keychain",
    registration.helperSha256,
    KEYCHAIN_HELPER_FILE_NAME,
  );
  if (registration.helperPath !== expected) return false;
  const descriptor = openRegularNoFollow(expected);
  if (descriptor === null) return false;
  try {
    const metadata = fs.fstatSync(descriptor);
    if (!isPrivateFile(metadata, 0o700)) return false;
    const bytes = fs.readFileSync(descriptor);
    return crypto.createHash("sha256").update(bytes).digest("hex") === registration.helperSha256;
  } finally {
    fs.closeSync(descriptor);
  }
}

function isPrivateFile(metadata, mode) {
  const currentUid = typeof process.geteuid === "function" ? process.geteuid() : metadata.uid;
  return (
    metadata.isFile() &&
    metadata.uid === currentUid &&
    (metadata.mode & 0o777) === mode &&
    metadata.nlink === 1
  );
}

function authorizationProofBody(registration) {
  return {
    schemaVersion: 1,
    appPath: registration.appPath,
    helperPath: registration.helperPath,
    helperSha256: registration.helperSha256,
  };
}

function hasMatchingAuthorizationProof(userRoot, registration) {
  const file = path.join(userRoot, "macos-keychain", KEYCHAIN_PROOF_FILE_NAME);
  const descriptor = openRegularNoFollow(file);
  if (descriptor === null) return false;
  try {
    const metadata = fs.fstatSync(descriptor);
    if (!isPrivateFile(metadata, 0o600)) return false;
    const observed = JSON.parse(fs.readFileSync(descriptor, "utf8"));
    return JSON.stringify(observed) === JSON.stringify(authorizationProofBody(registration));
  } catch {
    return false;
  } finally {
    fs.closeSync(descriptor);
  }
}

function writeAuthorizationProof(userRoot, registration) {
  const directory = path.join(userRoot, "macos-keychain");
  const file = path.join(directory, KEYCHAIN_PROOF_FILE_NAME);
  const temporary = path.join(
    directory,
    `.authorization-proof-${process.pid}-${crypto.randomBytes(8).toString("hex")}.tmp`,
  );
  let descriptor;
  try {
    descriptor = fs.openSync(
      temporary,
      fs.constants.O_WRONLY |
        fs.constants.O_CREAT |
        fs.constants.O_EXCL |
        (fs.constants.O_NOFOLLOW || 0),
      0o600,
    );
    fs.writeFileSync(descriptor, `${JSON.stringify(authorizationProofBody(registration))}\n`);
    fs.fsyncSync(descriptor);
    fs.closeSync(descriptor);
    descriptor = undefined;
    fs.renameSync(temporary, file);
    const directoryDescriptor = fs.openSync(directory, fs.constants.O_RDONLY);
    fs.fsyncSync(directoryDescriptor);
    fs.closeSync(directoryDescriptor);
  } catch {
    if (descriptor !== undefined) fs.closeSync(descriptor);
    try {
      fs.unlinkSync(temporary);
    } catch {
      // A failed best-effort probe must not disturb official startup.
    }
  }
}

function defaultLoadInterposer(interposerPath, appPath) {
  const modulePath = path.join(
    appPath,
    "Contents",
    "Resources",
    "app.asar.unpacked",
    "node_modules",
    "objc-js",
    "dist",
    "index.js",
  );
  const { NobjcLibrary } = require(modulePath);
  const library = new NobjcLibrary(interposerPath);
  void library.NSObject;
}

function stopChild(child) {
  try {
    if (child && !child.killed) child.kill();
  } catch {
    // The handoff is fail-open; an already-exited coordinator needs no cleanup.
  }
}

function waitForReadyFile(readyPath, child) {
  const started = Date.now();
  while (Date.now() - started < READY_TIMEOUT_MS) {
    if (readReadyFile(readyPath)) return true;
    if (typeof child?.exitCode === "number") break;
    Atomics.wait(WAIT_CELL, 0, 0, READY_POLL_MS);
  }
  removeReadyFile(readyPath);
  return false;
}

function readReadyFile(readyPath) {
  const descriptor = openRegularNoFollow(readyPath);
  if (descriptor === null) return false;
  try {
    const body = JSON.parse(fs.readFileSync(descriptor, "utf8"));
    return body?.schemaVersion === 1 && Number.isSafeInteger(body.pid) && body.pid > 0;
  } catch {
    return false;
  } finally {
    fs.closeSync(descriptor);
  }
}

function removeReadyFile(readyPath) {
  try {
    fs.unlinkSync(readyPath);
  } catch {
    // The coordinator may already have failed before publishing readiness.
  }
}

function readRegistration(userRoot) {
  const registrationPath = path.join(userRoot, "macos-update", "registration.json");
  const descriptor = openRegularNoFollow(registrationPath);
  if (descriptor === null) return null;
  try {
    const registration = JSON.parse(fs.readFileSync(descriptor, "utf8"));
    if (registration?.schemaVersion !== REGISTRATION_SCHEMA_VERSION) return null;
    if (typeof registration.installId !== "string" || !registration.installId) return null;
    if (typeof registration.appPath !== "string" || !path.isAbsolute(registration.appPath)) {
      return null;
    }
    for (const field of ["helperPath", "coordinatorAppPath", "coordinatorPath", "interposerPath"]) {
      if (typeof registration[field] !== "string" || !path.isAbsolute(registration[field])) {
        return null;
      }
    }
    for (const field of ["helperSha256", "coordinatorSha256", "interposerSha256"]) {
      if (!/^[0-9a-f]{64}$/.test(registration[field])) return null;
    }
    return registration;
  } catch {
    return null;
  } finally {
    fs.closeSync(descriptor);
  }
}

function matchesCurrentApp(registration, execPath) {
  const appPath = path.dirname(path.dirname(path.dirname(execPath)));
  return path.resolve(registration.appPath) === appPath;
}

function verifiedRegistrationAssets(userRoot, registration) {
  const helpersRoot = path.join(userRoot, "helpers", "macos-update");
  const releaseDir = path.dirname(path.resolve(registration.helperPath));
  if (!isInside(helpersRoot, releaseDir)) return false;
  if (registration.helperPath !== path.join(releaseDir, HELPER_FILE_NAME)) return false;
  if (
    registration.coordinatorAppPath !==
    path.join(releaseDir, "Incodex Update Coordinator.app")
  ) {
    return false;
  }
  if (
    registration.coordinatorPath !==
    path.join(registration.coordinatorAppPath, "Contents", "MacOS", COORDINATOR_FILE_NAME)
  ) {
    return false;
  }
  if (registration.interposerPath !== path.join(releaseDir, INTERPOSER_FILE_NAME)) return false;
  const infoPlist = path.join(registration.coordinatorAppPath, "Contents", "Info.plist");
  return (
    verifiedFile(registration.helperPath, registration.helperSha256) &&
    verifiedFile(registration.coordinatorPath, registration.coordinatorSha256) &&
    verifiedFile(registration.interposerPath, registration.interposerSha256) &&
    openRegularNoFollowAndClose(infoPlist)
  );
}

function isInside(parent, child) {
  const prefix = `${path.resolve(parent)}${path.sep}`;
  return path.resolve(child).startsWith(prefix);
}

function verifiedFile(file, expectedHash) {
  const descriptor = openRegularNoFollow(file);
  if (descriptor === null) return false;
  try {
    const bytes = fs.readFileSync(descriptor);
    return crypto.createHash("sha256").update(bytes).digest("hex") === expectedHash;
  } finally {
    fs.closeSync(descriptor);
  }
}

function openRegularNoFollowAndClose(file) {
  const descriptor = openRegularNoFollow(file);
  if (descriptor === null) return false;
  fs.closeSync(descriptor);
  return true;
}

function openRegularNoFollow(file) {
  const flags = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0);
  let descriptor;
  try {
    descriptor = fs.openSync(file, flags);
    if (!fs.fstatSync(descriptor).isFile()) {
      fs.closeSync(descriptor);
      return null;
    }
    return descriptor;
  } catch {
    if (descriptor !== undefined) fs.closeSync(descriptor);
    return null;
  }
}

module.exports = { prepareUpdateHandoff, probeKeychainAuthorizationReadiness };
