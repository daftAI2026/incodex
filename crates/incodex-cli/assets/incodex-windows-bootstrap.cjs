"use strict";

const fs = require("node:fs");
const { createHash } = require("node:crypto");
const path = require("node:path");

const REGISTRATION_NAME = "INCODEX_WINDOWS_REGISTRATION_ID";
const BOOTSTRAPPED_NAME = "INCODEX_WINDOWS_BOOTSTRAPPED";
const PACKAGE_NAME = "INCODEX_WINDOWS_PACKAGE_FULL_NAME";
const STATE_PATH_NAME = "INCODEX_WINDOWS_STATE_PATH";
const REGISTRATION_PATTERN = /^[a-f0-9]{32}$/;
const ENABLED_PHASES = new Set(["enabled-unobserved", "enabled-observed"]);
const USER_DATA_PREFIX = "--user-data-dir=";
const ACTIVATION_PIPE_PREFIX = "\\\\.\\pipe\\Incodex-Activation-Environment-";
const ACTIVATION_RESPONSE_LIMIT = 64 * 1024;

function activationToken(argv) {
  let userDataDir = "";
  for (const argument of argv) {
    if (typeof argument !== "string" || !argument.startsWith(USER_DATA_PREFIX)) continue;
    const value = argument.slice(USER_DATA_PREFIX.length);
    if (userDataDir) {
      throw new Error("repeated Windows activation user data directory");
    }
    const normalized = path.win32.normalize(value);
    const session = path.win32.basename(path.win32.dirname(normalized));
    const sessions = path.win32.basename(path.win32.dirname(path.win32.dirname(normalized)));
    const incodex = path.win32.basename(
      path.win32.dirname(path.win32.dirname(path.win32.dirname(normalized))),
    );
    if (
      !path.win32.isAbsolute(value) ||
      path.win32.basename(normalized).toLowerCase() !== "chromium" ||
      !session.startsWith("s-") ||
      session.length <= 2 ||
      sessions.toLowerCase() !== "sessions" ||
      incodex.toLowerCase() !== ".incodex"
    ) {
      continue;
    }
    userDataDir = value;
  }
  return userDataDir
    ? createHash("sha256").update(userDataDir, "utf8").digest("hex").slice(0, 32)
    : "";
}

function readActivationEnvironment(pipeName, io = fs) {
  const descriptor = io.openSync(pipeName, "r+");
  try {
    const request = "environment\n";
    if (io.writeSync(descriptor, request) !== Buffer.byteLength(request)) {
      throw new Error("Windows activation environment request was truncated");
    }
    const response = Buffer.alloc(ACTIVATION_RESPONSE_LIMIT);
    const length = io.readSync(descriptor, response, 0, response.length, null);
    if (length <= 0) {
      throw new Error("Windows activation environment response is empty");
    }
    return JSON.parse(response.subarray(0, length).toString("utf8"));
  } finally {
    io.closeSync(descriptor);
  }
}

function claimActivationEnvironment(options, env, token) {
  const read = options.readActivationEnvironment || readActivationEnvironment;
  const claimed = read(`${ACTIVATION_PIPE_PREFIX}${token}`);
  if (
    !claimed ||
    typeof claimed !== "object" ||
    !["runtime", "cdp"].includes(claimed.mode) ||
    !claimed.environment ||
    typeof claimed.environment !== "object" ||
    Array.isArray(claimed.environment)
  ) {
    throw new Error("invalid Windows activation environment response");
  }
  for (const [name, value] of Object.entries(claimed.environment)) {
    if (!/^[A-Z][A-Z0-9_]*$/.test(name) || typeof value !== "string" || value.includes("\0")) {
      throw new Error("invalid Windows activation environment entry");
    }
    env[name] = value;
  }
  env[BOOTSTRAPPED_NAME] = token;
  return claimed.mode;
}

function readInstallState(statePath) {
  return JSON.parse(fs.readFileSync(statePath, "utf8"));
}

function comparableWindowsPath(value) {
  const normalized = path.win32.normalize(value);
  if (normalized.toLowerCase().startsWith("\\\\?\\unc\\")) {
    return `\\\\${normalized.slice(8)}`.toLowerCase();
  }
  if (normalized.startsWith("\\\\?\\")) {
    return normalized.slice(4).toLowerCase();
  }
  return normalized.toLowerCase();
}

function ownedInstallState(options, registrationId) {
  const env = options.env || process.env;
  const packageFullName = env[PACKAGE_NAME] || "";
  const statePath = env[STATE_PATH_NAME] || "";
  const runtimeRoot = options.runtimeDir || __dirname;
  if (
    !packageFullName ||
    packageFullName.includes("\0") ||
    !path.win32.isAbsolute(statePath) ||
    statePath.includes("\0")
  ) {
    return false;
  }
  let state;
  try {
    const readState = options.readState || readInstallState;
    state = readState(statePath);
  } catch {
    return false;
  }
  const owned = Boolean(
    state &&
      typeof state === "object" &&
      !Array.isArray(state) &&
      state.schemaVersion === 1 &&
      state.desired === "enabled" &&
      ENABLED_PHASES.has(state.phase) &&
      state.registrationId === registrationId &&
      state.packageFullName === packageFullName &&
      typeof state.runtimeRelease === "string" &&
      /^[A-Za-z0-9.-]+$/.test(state.runtimeRelease) &&
      state.runtimeRelease !== "." &&
      state.runtimeRelease !== ".." &&
      comparableWindowsPath(runtimeRoot) ===
        comparableWindowsPath(path.win32.join(path.win32.dirname(statePath), "runtime")),
  );
  return owned ? state : null;
}

function loadRuntimeWhenElectronIsReady(options, runtimeDir) {
  const load = options.load || require;
  const onElectronLoaded =
    options.onElectronLoaded || ((callback) => process.once("loaded", callback));
  onElectronLoaded(() => {
    try {
      load(path.join(runtimeDir, "incodex-main.cjs"));
    } catch {
      console.error("[incodex] Windows Runtime attach failed");
    }
  });
}

function attachWindowsRuntime(options = {}) {
  const env = options.env || process.env;
  const argv = options.argv || process.argv;
  const processType = options.processType || process.type || "";
  const token = activationToken(argv);
  if (processType === "browser" && token && !env[BOOTSTRAPPED_NAME]) {
    const mode = claimActivationEnvironment(options, env, token);
    if (mode === "runtime") {
      const runtimeDir = options.runtimeDir || __dirname;
      loadRuntimeWhenElectronIsReady(options, runtimeDir);
    }
    return true;
  }
  const registrationId = env[REGISTRATION_NAME] || "";
  const runtimeRoot = options.runtimeDir || __dirname;
  const state = ownedInstallState(options, registrationId);
  if (
    processType !== "browser" ||
    !REGISTRATION_PATTERN.test(registrationId) ||
    env[BOOTSTRAPPED_NAME] ||
    !state
  ) {
    return false;
  }

  env[BOOTSTRAPPED_NAME] = registrationId;
  const runtimeDir = path.win32.join(runtimeRoot, "releases", state.runtimeRelease);
  loadRuntimeWhenElectronIsReady(options, runtimeDir);
  return true;
}

attachWindowsRuntime();

module.exports = { attachWindowsRuntime, readActivationEnvironment };
