"use strict";

const fs = require("node:fs");
const path = require("node:path");

const REGISTRATION_NAME = "INCODEX_WINDOWS_REGISTRATION_ID";
const BOOTSTRAPPED_NAME = "INCODEX_WINDOWS_BOOTSTRAPPED";
const PACKAGE_NAME = "INCODEX_WINDOWS_PACKAGE_FULL_NAME";
const STATE_PATH_NAME = "INCODEX_WINDOWS_STATE_PATH";
const REGISTRATION_PATTERN = /^[a-f0-9]{32}$/;
const ENABLED_PHASES = new Set(["enabled-unobserved", "enabled-observed"]);

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

function attachWindowsRuntime(options = {}) {
  const env = options.env || process.env;
  const processType = options.processType || process.type || "";
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
  const load = options.load || require;
  try {
    load(path.join(runtimeDir, "incodex-main.cjs"));
    return true;
  } catch {
    console.error("[incodex] Windows Runtime attach failed");
    return false;
  }
}

attachWindowsRuntime();

module.exports = { attachWindowsRuntime };
