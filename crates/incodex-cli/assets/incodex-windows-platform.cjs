"use strict";

const { spawn } = require("node:child_process");
const path = require("node:path");

const READY_TIMEOUT_MS = 15_000;
const BOUNDS_PATTERN = /^-?\d{1,10},-?\d{1,10},\d{1,10},\d{1,10}$/;

function validAbsolutePath(value) {
  return typeof value === "string" && !value.includes("\0") && path.win32.isAbsolute(value);
}

function launchIncognito(options = {}) {
  const helperPath = options.helperPath || "";
  const sourceHome = options.sourceHome || "";
  const sourceBounds = options.sourceBounds || "";
  if (!validAbsolutePath(helperPath)) {
    return Promise.resolve({ ok: false, reason: "invalid-helper" });
  }
  if (!validAbsolutePath(sourceHome)) {
    return Promise.resolve({ ok: false, reason: "invalid-source-home" });
  }
  if (sourceBounds && !BOUNDS_PATTERN.test(sourceBounds)) {
    return Promise.resolve({ ok: false, reason: "invalid-source-bounds" });
  }

  const spawnProcess = options.spawnProcess || spawn;
  return new Promise((resolve) => {
    let child;
    let settled = false;
    let output = "";
    const done = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(result);
    };
    try {
      child = spawnProcess(
        helperPath,
        [
          "__incodex_windows_runtime_open",
          "--source-home",
          sourceHome,
          "--source-bounds",
          sourceBounds,
        ],
        {
          detached: true,
          windowsHide: true,
          stdio: ["ignore", "pipe", "ignore"],
        },
      );
    } catch {
      resolve({ ok: false, reason: "spawn-failed" });
      return;
    }
    const timer = setTimeout(() => done({ ok: false, reason: "ready-timeout" }), READY_TIMEOUT_MS);
    if (!child?.pid || !child.stdout) {
      done({ ok: false, reason: "spawn-failed" });
      return;
    }
    child.once("error", () => done({ ok: false, reason: "spawn-failed" }));
    child.once("exit", () => done({ ok: false, reason: "exited-early" }));
    child.stdout.on("data", (chunk) => {
      if (settled) return;
      output = (output + String(chunk)).slice(-32);
      if (!/(^|\r?\n)ready\r?\n/.test(`\n${output}`)) return;
      child.unref();
      child.stdout.destroy();
      done({ ok: true });
    });
  });
}

module.exports = { launchIncognito };
