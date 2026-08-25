// @ts-nocheck
"use strict";

const { execFile } = require("node:child_process");
const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const core = require("./incodex-owner-core.cts");
const recovery = require("./incodex-owner-recovery.cts");

const {
  LOCK_NAME,
  SOCK_NAME,
  targetIdFromExec,
  targetStateDir,
  ownerPortFromExec,
  ownerToken,
  ownerMatchesLive,
  writeOwnerLock,
  writeOwnerLockExclusive,
  readOwnerLockState,
  readOwnerLock,
  readOwnerRecords,
  setOwnerRecordTestHook,
  currentOwner,
  staleOwner,
  staleOwnerRecord,
  OwnerLeaseError,
  ownsOwnerLease,
} = core;
const { clearOwnerLock, releaseOwnerLease, acquireOwnerLease, setRaiseHandler } = recovery;
const processIdentity = core.processIdentity;

function sockPath(stateRoot) {
  return path.join(stateRoot, SOCK_NAME);
}

function connectToOwner(owner, expectedToken, timeoutMs) {
  return new Promise((resolve) => {
    const socket = net.connect({ host: "127.0.0.1", port: ownerPortFromExec(owner.execPath) });
    let done = false;
    let response = "";
    let deadline;
    function finish(ok) {
      if (done) return;
      done = true;
      clearTimeout(deadline);
      socket.destroy();
      resolve(ok);
    }
    socket.setTimeout(timeoutMs);
    deadline = setTimeout(() => finish(false), timeoutMs);
    socket.once("connect", () => {
      try {
        socket.write(`${expectedToken ? `raise ${expectedToken}` : "raise"}\n`);
      } catch {
        finish(false);
      }
    });
    socket.on("data", (buf) => {
      response += String(buf);
      if (Buffer.byteLength(response, "utf8") > 256) {
        finish(false);
        return;
      }
      const lines = response.split("\n").map((line) => line.trim());
      if (lines.includes("ok")) finish(true);
      else if (lines.includes("denied")) finish(false);
    });
    socket.once("error", () => finish(false));
    socket.once("timeout", () => finish(false));
  });
}

async function connectExisting(stateRoot, timeoutMs = 400, token = "") {
  if (typeof timeoutMs !== "number") {
    token = timeoutMs;
    timeoutMs = 400;
  }
  const records = readOwnerRecords(stateRoot).filter(({ state }) => state.kind === "valid");
  const candidates = token
    ? records.filter(({ state }) => ownerToken(state.owner) === token)
    : records;
  for (const { state } of candidates) {
    if (await connectToOwner(state.owner, token || ownerToken(state.owner), timeoutMs)) return true;
  }
  return false;
}

async function connectExistingWithRetry(stateRoot, token, options = {}) {
  const attempts = options.attempts ?? 5;
  const timeoutMs = options.timeoutMs ?? 400;
  const delayMs = options.delayMs ?? 100;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    if (await connectExisting(stateRoot, timeoutMs, token)) return true;
    if (attempt + 1 < attempts && delayMs > 0) await wait(delayMs);
  }
  return false;
}

function listenForRaise(stateRoot, onRaise, lease) {
  if (!lease || !ownerToken(lease)) throw new OwnerLeaseError("OWNER_INVALID", "raise socket requires an owner lease");
  return setRaiseHandler(stateRoot, lease, onRaise);
}

function singleFlight(holder, start) {
  if (holder.current) return holder.current;
  holder.current = Promise.resolve()
    .then(start)
    .finally(function clearSingleFlight() {
      holder.current = null;
    });
  return holder.current;
}

function wait(delayMs) {
  return new Promise((resolve) => setTimeout(resolve, delayMs));
}

function sessionProcessIdsFromPs(snapshot, sessionRoot, currentPid = process.pid) {
  if (
    typeof snapshot !== "string" ||
    typeof sessionRoot !== "string" ||
    !path.isAbsolute(sessionRoot) ||
    /[\0\r\n]/.test(sessionRoot)
  ) {
    return [];
  }
  const marker = `INCODEX_SESSION_ROOT=${sessionRoot}`;
  const pids = new Set();
  for (const line of snapshot.split("\n")) {
    const match = line.match(/^\s*(\d+)\s+(.+)$/);
    if (!match) continue;
    const pid = Number(match[1]);
    if (!Number.isSafeInteger(pid) || pid <= 0 || pid === currentPid) continue;
    const command = match[2];
    let offset = command.indexOf(marker);
    while (offset >= 0) {
      const before = offset === 0 ? "" : command[offset - 1];
      const after = command[offset + marker.length] ?? "";
      if ((!before || /\s/.test(before)) && (!after || /\s/.test(after))) {
        pids.add(pid);
        break;
      }
      offset = command.indexOf(marker, offset + marker.length);
    }
  }
  return [...pids].sort((left, right) => left - right);
}

function processSnapshot() {
  return new Promise((resolve, reject) => {
    execFile(
      "/bin/ps",
      ["axEww", "-o", "pid=,command="],
      {
        encoding: "utf8",
        env: { ...process.env, LC_ALL: "C" },
        maxBuffer: 8 * 1024 * 1024,
      },
      (error, stdout) => {
        if (error) reject(error);
        else resolve(stdout);
      },
    );
  });
}

function signalProcesses(pids, signal) {
  for (const pid of pids) {
    try {
      process.kill(pid, signal);
    } catch (error) {
      if (error?.code !== "ESRCH") throw error;
    }
  }
}

async function markedSessionProcesses(sessionRoot) {
  return sessionProcessIdsFromPs(await processSnapshot(), sessionRoot);
}

async function waitForSessionProcesses(sessionRoot, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let pids = await markedSessionProcesses(sessionRoot);
  while (pids.length > 0 && Date.now() < deadline) {
    await wait(25);
    pids = await markedSessionProcesses(sessionRoot);
  }
  return pids;
}

async function quiesceSessionHelpers(sessionRoot) {
  if (process.platform !== "darwin") return;
  let pids = await markedSessionProcesses(sessionRoot);
  if (pids.length === 0) return;
  signalProcesses(pids, "SIGTERM");
  pids = await waitForSessionProcesses(sessionRoot, 500);
  if (pids.length > 0) signalProcesses(pids, "SIGKILL");
  pids = await waitForSessionProcesses(sessionRoot, 500);
  if (pids.length > 0) throw new Error("isolated helpers survived SIGKILL");

  // Chromium helpers may be reparented just after the main child exits.
  pids = await markedSessionProcesses(sessionRoot);
  if (pids.length > 0) {
    signalProcesses(pids, "SIGKILL");
    pids = await waitForSessionProcesses(sessionRoot, 500);
  }
  if (pids.length > 0) throw new Error("late isolated helpers survived SIGKILL");
}

if (typeof module !== "undefined") {
  module.exports = {
    LOCK_NAME,
    SOCK_NAME,
    targetIdFromExec,
    targetStateDir,
    ownerPortFromExec,
    sockPath,
    processIdentity,
    ownerToken,
    ownerMatchesLive,
    writeOwnerLock,
    writeOwnerLockExclusive,
    readOwnerLockState,
    readOwnerLock,
    readOwnerRecords,
    setOwnerRecordTestHook,
    clearOwnerLock,
    releaseOwnerLease,
    currentOwner,
    staleOwner,
    staleOwnerRecord,
    OwnerLeaseError,
    ownsOwnerLease,
    acquireOwnerLease,
    connectExisting,
    connectExistingWithRetry,
    listenForRaise,
    singleFlight,
    sessionProcessIdsFromPs,
    quiesceSessionHelpers,
  };
}

export {
  LOCK_NAME,
  SOCK_NAME,
  targetIdFromExec,
  targetStateDir,
  ownerPortFromExec,
  sockPath,
  processIdentity,
  ownerToken,
  ownerMatchesLive,
  writeOwnerLock,
  writeOwnerLockExclusive,
  readOwnerLockState,
  readOwnerLock,
  readOwnerRecords,
  setOwnerRecordTestHook,
  clearOwnerLock,
  releaseOwnerLease,
  currentOwner,
  staleOwner,
  staleOwnerRecord,
  OwnerLeaseError,
  ownsOwnerLease,
  acquireOwnerLease,
  connectExisting,
  connectExistingWithRetry,
  listenForRaise,
  singleFlight,
  sessionProcessIdsFromPs,
  quiesceSessionHelpers,
};
