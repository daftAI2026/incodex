"use strict";

const { spawn } = require("node:child_process");
const { writeFileSync } = require("node:fs");
const path = require("node:path");

const READY_TIMEOUT_MS = 35_000;
const CANCEL_EXIT_TIMEOUT_MS = 5_000;
const BOUNDS_PATTERN = /^-?\d{1,10},-?\d{1,10},\d{1,10},\d{1,10}$/;
const READY_PIPE_PATTERN = /^\\\\\.\\pipe\\Incodex-Runtime-Ready-[a-f0-9]{32}$/;

function validAbsolutePath(value) {
  return typeof value === "string" && !value.includes("\0") && path.win32.isAbsolute(value);
}

function markReady(pipeName, write = writeFileSync) {
  if (typeof pipeName !== "string" || !READY_PIPE_PATTERN.test(pipeName)) return false;
  try {
    write(pipeName, "accepted\n");
    return true;
  } catch {
    return false;
  }
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
  const readyTimeoutMs = options.readyTimeoutMs || READY_TIMEOUT_MS;
  return new Promise((resolve) => {
    let child;
    let settled = false;
    let cancellationReason = "";
    let timer;
    let cancelTimer;
    let output = "";
    const done = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      clearTimeout(cancelTimer);
      resolve(result);
    };
    const forceCancel = () => {
      try {
        child?.kill?.();
      } catch {
        done({ ok: false, reason: "cleanup-unproven" });
        return;
      }
      cancelTimer = setTimeout(
        () => done({ ok: false, reason: "cleanup-unproven" }),
        CANCEL_EXIT_TIMEOUT_MS,
      );
    };
    const cancel = (reason) => {
      if (settled || cancellationReason) return;
      cancellationReason = reason;
      clearTimeout(timer);
      try {
        if (child?.stdin?.end) {
          child.stdin.end("cancel\n");
          cancelTimer = setTimeout(forceCancel, CANCEL_EXIT_TIMEOUT_MS);
        } else {
          forceCancel();
        }
      } catch {
        forceCancel();
      }
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
          stdio: ["pipe", "pipe", "ignore"],
        },
      );
    } catch {
      resolve({ ok: false, reason: "spawn-failed" });
      return;
    }
    timer = setTimeout(() => cancel("ready-timeout"), readyTimeoutMs);
    if (!child?.pid || !child.stdout) {
      done({ ok: false, reason: "spawn-failed" });
      return;
    }
    child.once("error", () =>
      done({ ok: false, reason: cancellationReason || "spawn-failed" }),
    );
    child.once("exit", () =>
      done({ ok: false, reason: cancellationReason || "exited-early" }),
    );
    child.stdout.on("data", (chunk) => {
      if (settled) return;
      output = (output + String(chunk)).slice(-32);
      if (!/(^|\r?\n)ready\r?\n/.test(`\n${output}`)) return;
      child.stdin?.destroy?.();
      child.unref();
      child.stdout.destroy();
      done({ ok: true });
    });
  });
}

module.exports = { launchIncognito, markReady };
