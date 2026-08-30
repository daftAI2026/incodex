// @ts-nocheck
"use strict";

const { spawn } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const REGISTRATION_SCHEMA_VERSION = 1;
const HELPER_FILE_NAME = "incodex";

function spawnCoordinator(options = {}) {
  const platform = options.platform || process.platform;
  if (platform !== "darwin") return false;

  try {
    const userRoot = path.resolve(options.userRoot);
    const execPath = path.resolve(options.execPath || process.execPath);
    const pid = Number(options.pid || process.pid);
    if (!Number.isSafeInteger(pid) || pid <= 0) return false;

    const registration = readRegistration(userRoot);
    if (!registration || !matchesCurrentApp(registration, execPath)) return false;
    if (!verifiedHelper(userRoot, registration)) return false;

    const spawnProcess = options.spawnProcess || spawn;
    const child = spawnProcess(registration.helperPath, [], {
      detached: true,
      stdio: "ignore",
      env: {
        ...process.env,
        INCODEX_MACOS_UPDATE_WORKER: "1",
        INCODEX_MACOS_UPDATE_INSTALL_ID: registration.installId,
        INCODEX_MACOS_UPDATE_PARENT_PID: String(pid),
      },
    });
    child.unref();
    return true;
  } catch {
    return false;
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
    if (typeof registration.helperPath !== "string") return null;
    if (!/^[0-9a-f]{64}$/.test(registration.helperSha256)) return null;
    return registration;
  } finally {
    fs.closeSync(descriptor);
  }
}

function matchesCurrentApp(registration, execPath) {
  const appPath = path.dirname(path.dirname(path.dirname(execPath)));
  return path.resolve(registration.appPath) === appPath;
}

function verifiedHelper(userRoot, registration) {
  const helpersRoot = path.join(userRoot, "helpers", "macos-update");
  const expectedPrefix = `${helpersRoot}${path.sep}`;
  const helperPath = path.resolve(registration.helperPath);
  if (!helperPath.startsWith(expectedPrefix) || path.basename(helperPath) !== HELPER_FILE_NAME) {
    return false;
  }

  const descriptor = openRegularNoFollow(helperPath);
  if (descriptor === null) return false;
  try {
    const bytes = fs.readFileSync(descriptor);
    return crypto.createHash("sha256").update(bytes).digest("hex") === registration.helperSha256;
  } finally {
    fs.closeSync(descriptor);
  }
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

module.exports = { spawnCoordinator };
