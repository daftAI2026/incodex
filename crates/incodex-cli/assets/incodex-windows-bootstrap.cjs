"use strict";

const path = require("node:path");

const REGISTRATION_NAME = "INCODEX_WINDOWS_REGISTRATION_ID";
const BOOTSTRAPPED_NAME = "INCODEX_WINDOWS_BOOTSTRAPPED";
const REGISTRATION_PATTERN = /^[a-f0-9]{32}$/;

function attachWindowsRuntime(options = {}) {
  const env = options.env || process.env;
  const processType = options.processType || process.type || "";
  const registrationId = env[REGISTRATION_NAME] || "";
  if (
    processType !== "browser" ||
    !REGISTRATION_PATTERN.test(registrationId) ||
    env[BOOTSTRAPPED_NAME]
  ) {
    return false;
  }

  env[BOOTSTRAPPED_NAME] = registrationId;
  const load = options.load || require;
  try {
    load(path.join(__dirname, "incodex-main.cjs"));
    return true;
  } catch {
    console.error("[incodex] Windows Runtime attach failed");
    return false;
  }
}

attachWindowsRuntime();

module.exports = { attachWindowsRuntime };
