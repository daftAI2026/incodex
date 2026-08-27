"use strict";

const fs = require("node:fs");
const path = require("node:path");

const REGISTRATION_NAME = "INCODEX_WINDOWS_REGISTRATION_ID";
const BOOTSTRAPPED_NAME = "INCODEX_WINDOWS_BOOTSTRAPPED";
const PACKAGE_NAME = "INCODEX_WINDOWS_PACKAGE_FULL_NAME";
const STATE_PATH_NAME = "INCODEX_WINDOWS_STATE_PATH";
const REGISTRATION_PATTERN = /^[a-f0-9]{32}$/;
const ENABLED_PHASES = new Set(["enabled-unobserved", "enabled-observed"]);
const ACTIVATION_TOKEN_PREFIX = "--incodex-activation-token=";
const ACTIVATION_TOKEN_PATTERN = /^[a-f0-9]{32}$/;
const ACTIVATION_PIPE_PREFIX = "\\\\.\\pipe\\Incodex-Activation-Environment-";
const ACTIVATION_RESPONSE_LIMIT = 64 * 1024;

function activationToken(argv) {
  let token = "";
  for (const argument of argv) {
    if (typeof argument !== "string" || !argument.startsWith(ACTIVATION_TOKEN_PREFIX)) continue;
    const value = argument.slice(ACTIVATION_TOKEN_PREFIX.length);
    if (!ACTIVATION_TOKEN_PATTERN.test(value) || token) {
      throw new Error("invalid Windows activation token");
    }
    token = value;
  }
  return token;
}

function readActivationEnvironment(pipeName) {
  const descriptor = fs.openSync(pipeName, "r+");
  try {
    fs.writeSync(descriptor, "environment\n");
    const response = fs.readFileSync(descriptor, { encoding: "utf8" });
    if (Buffer.byteLength(response) > ACTIVATION_RESPONSE_LIMIT) {
      throw new Error("Windows activation environment is too large");
    }
    return JSON.parse(response);
  } finally {
    fs.closeSync(descriptor);
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

function ownedInstallState(options, registrationId) {
  const env = options.env || process.env;
  const packageFullName = env[PACKAGE_NAME] || "";
  const statePath = env[STATE_PATH_NAME] || "";
  const runtimeDir = options.runtimeDir || __dirname;
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
  return Boolean(
    state &&
      typeof state === "object" &&
      !Array.isArray(state) &&
      state.schemaVersion === 1 &&
      state.desired === "enabled" &&
      ENABLED_PHASES.has(state.phase) &&
      state.registrationId === registrationId &&
      state.packageFullName === packageFullName &&
      state.runtimeRelease === path.win32.basename(runtimeDir),
  );
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
  const runtimeDir = options.runtimeDir || __dirname;
  if (
    processType !== "browser" ||
    !REGISTRATION_PATTERN.test(registrationId) ||
    env[BOOTSTRAPPED_NAME] ||
    !ownedInstallState(options, registrationId)
  ) {
    return false;
  }

  env[BOOTSTRAPPED_NAME] = registrationId;
  loadRuntimeWhenElectronIsReady(options, runtimeDir);
  return true;
}

attachWindowsRuntime();

module.exports = { attachWindowsRuntime };
