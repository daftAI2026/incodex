// @ts-nocheck
"use strict";

const { spawn } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const { pathToFileURL } = require("node:url");

const REGISTRATION_SCHEMA_VERSION = 2;
const HELPER_FILE_NAME = "incodex";
const COORDINATOR_FILE_NAME = "incodex-update-coordinator";
const INTERPOSER_FILE_NAME = "libincodex-sparkle-interpose.dylib";
const READY_TIMEOUT_MS = 2_000;
const READY_POLL_MS = 20;

async function prepareUpdateHandoff(options = {}) {
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
        INCODEX_MACOS_UPDATE_READY_PATH: readyPath,
        INCODEX_MACOS_UPDATE_PENDING_PATH: pendingPath,
      },
    });
    const waitForCoordinator = options.waitForCoordinator || waitForReadyFile;
    if (!(await waitForCoordinator(readyPath, child))) {
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
    await loadInterposer(registration.interposerPath, registration.appPath);
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

async function defaultLoadInterposer(interposerPath, appPath) {
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
  const { NobjcLibrary } = await import(pathToFileURL(modulePath).href);
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
  return new Promise((resolve) => {
    const started = Date.now();
    const timer = setInterval(() => {
      if (readReadyFile(readyPath)) {
        clearInterval(timer);
        resolve(true);
        return;
      }
      if (child?.exitCode !== null || Date.now() - started >= READY_TIMEOUT_MS) {
        clearInterval(timer);
        removeReadyFile(readyPath);
        resolve(false);
      }
    }, READY_POLL_MS);
  });
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

module.exports = { prepareUpdateHandoff };
