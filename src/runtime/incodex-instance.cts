// @ts-nocheck
"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const crypto = require("node:crypto");

const LOCK_NAME = "incognito.lock";
const SOCK_NAME = "incognito.sock";
const OWNER_RETRY_COUNT = 5;
const OWNER_RETRY_DELAY_MS = 100;

function targetIdFromExec(execPath) {
  return crypto.createHash("sha256").update(execPath || "unknown").digest("hex").slice(0, 12);
}

function targetStateDir(userRoot, execPath) {
  return path.join(userRoot, "targets", targetIdFromExec(execPath));
}

function lockPath(stateRoot) {
  return path.join(stateRoot, LOCK_NAME);
}

function sockPath(stateRoot) {
  return path.join(stateRoot, SOCK_NAME);
}

function processIdentity(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return null;
  const listed = spawnSync("ps", ["-p", String(pid), "-o", "pid=,lstart=,comm="], { encoding: "utf8" });
  if (listed.status !== 0 || !listed.stdout.trim()) return null;
  const line = listed.stdout.trim();
  const match = line.match(/^(\d+)\s+(.+?)\s+(\S+)$/);
  if (!match) return null;
  const comm = match[3];
  return {
    pid: Number(match[1]),
    startedAt: match[2].trim(),
    processStartIdentity: match[2].trim(),
    comm,
    execIdentity: comm,
  };
}

function ownerToken(owner) {
  if (!owner || typeof owner !== "object") return "";
  if (typeof owner.token === "string" && owner.token) return owner.token;
  if (typeof owner.nonce === "string" && owner.nonce) return owner.nonce;
  return "";
}

function ownerMatchesLive(owner, live) {
  if (!owner || !live) return false;
  const ownerStart = owner.processStartIdentity || owner.startedAt;
  const liveStart = live.processStartIdentity || live.startedAt;
  if (owner.pid !== live.pid || ownerStart !== liveStart) return false;
  const ownerExec = owner.execIdentity || owner.comm;
  const liveExec = live.execIdentity || live.comm;
  if (ownerExec && liveExec) return ownerExec === liveExec;
  if (owner.execPath && live.execPath) return owner.execPath === live.execPath;
  return true;
}

function sameOwnerToken(left, right) {
  const leftToken = ownerToken(left);
  const rightToken = ownerToken(right);
  return Boolean(leftToken && rightToken && leftToken === rightToken);
}

function pidAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code === "EPERM";
  }
}

function writeOwnerLock(stateRoot, owner) {
  fs.mkdirSync(stateRoot, { recursive: true, mode: 0o700 });
  const file = lockPath(stateRoot);
  const fd = fs.openSync(
    file,
    fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL | (fs.constants.O_NOFOLLOW || 0),
    0o600,
  );
  try {
    fs.writeSync(fd, `${JSON.stringify(owner)}\n`);
    try {
      fs.fsyncSync(fd);
    } catch {
      /* Some test filesystems do not support fsync; the exclusive claim remains valid. */
    }
  } finally {
    fs.closeSync(fd);
  }
}

function readOwnerLockState(stateRoot) {
  const file = lockPath(stateRoot);
  let stats;
  try {
    stats = fs.lstatSync(file);
  } catch (error) {
    if (error && error.code === "ENOENT") return { kind: "missing", owner: null };
    return { kind: "invalid", owner: null, reason: String(error) };
  }
  if (stats.isSymbolicLink() || !stats.isFile()) {
    return { kind: "invalid", owner: null, reason: "owner lock is not a regular file" };
  }
  try {
    const owner = JSON.parse(fs.readFileSync(file, "utf8"));
    if (!owner || typeof owner !== "object" || !Number.isInteger(owner.pid) || !ownerToken(owner)) {
      return { kind: "invalid", owner: null, reason: "owner lock has no valid identity or token" };
    }
    return { kind: "valid", owner };
  } catch (error) {
    return { kind: "invalid", owner: null, reason: String(error) };
  }
}

function readOwnerLock(stateRoot) {
  const state = readOwnerLockState(stateRoot);
  return state.kind === "valid" ? state.owner : null;
}

function removeSocket(stateRoot) {
  const file = sockPath(stateRoot);
  let stats;
  try {
    stats = fs.lstatSync(file);
  } catch (error) {
    if (error && error.code === "ENOENT") return true;
    return false;
  }
  if (stats.isSymbolicLink()) return false;
  try {
    fs.rmSync(file);
    return true;
  } catch (error) {
    return Boolean(error && error.code === "ENOENT");
  }
}

function clearOwnerLock(stateRoot, expectedOwner) {
  if (!expectedOwner || !ownerToken(expectedOwner)) return false;
  const current = readOwnerLock(stateRoot);
  if (!sameOwnerToken(current, expectedOwner)) return false;

  // Remove the socket while the matching lock still prevents a new owner from
  // taking the path. Re-check the token before deleting the lock itself.
  if (!removeSocket(stateRoot)) return false;
  const stillCurrent = readOwnerLock(stateRoot);
  if (!sameOwnerToken(stillCurrent, expectedOwner)) return false;
  try {
    fs.rmSync(lockPath(stateRoot));
    return true;
  } catch (error) {
    return Boolean(error && error.code === "ENOENT");
  }
}

function currentOwner(sessionId, execPath) {
  const live = processIdentity(process.pid);
  const token = crypto.randomBytes(16).toString("hex");
  return {
    pid: process.pid,
    startedAt: live?.startedAt || "",
    processStartIdentity: live?.startedAt || "",
    execPath: execPath || process.execPath,
    execIdentity: live?.execIdentity || "",
    sessionId: sessionId || "",
    token,
    nonce: token,
  };
}

function staleOwnerRecord(owner) {
  if (!owner || !Number.isInteger(owner.pid) || owner.pid <= 0) return true;
  const live = processIdentity(owner.pid);
  if (!live) return !pidAlive(owner.pid);
  return !ownerMatchesLive(owner, live);
}

function staleOwner(stateRoot) {
  const state = readOwnerLockState(stateRoot);
  if (state.kind !== "valid") return state.kind === "missing" || state.kind === "invalid";
  return staleOwnerRecord(state.owner);
}

class OwnerLeaseError extends Error {
  constructor(code, message, owner = null) {
    super(message);
    this.name = "OwnerLeaseError";
    this.code = code;
    this.owner = owner;
  }
}

function ownsOwnerLease(stateRoot, expectedOwner) {
  const current = readOwnerLock(stateRoot);
  return sameOwnerToken(current, expectedOwner);
}

function acquireOwnerLease(stateRoot, owner) {
  if (!owner || !ownerToken(owner)) {
    throw new OwnerLeaseError("OWNER_INVALID", "owner lease requires a token");
  }
  try {
    writeOwnerLock(stateRoot, owner);
    const current = readOwnerLock(stateRoot);
    if (!sameOwnerToken(current, owner)) {
      throw new OwnerLeaseError("OWNER_VERIFY_FAILED", "owner lease verification failed", current);
    }
    return owner;
  } catch (error) {
    if (error?.code !== "EEXIST") throw error;
  }

  const current = readOwnerLockState(stateRoot);
  if (current.kind === "missing") {
    throw new OwnerLeaseError("OWNER_RACE", "owner lease disappeared during acquisition");
  }
  if (current.kind !== "valid") {
    throw new OwnerLeaseError("OWNER_UNREADABLE", "owner lease is not readable");
  }
  if (!staleOwnerRecord(current.owner)) {
    throw new OwnerLeaseError("OWNER_BUSY", "another Incognito owner is active", current.owner);
  }
  if (!clearOwnerLock(stateRoot, current.owner)) {
    throw new OwnerLeaseError("OWNER_RACE", "owner lease changed during stale-owner cleanup");
  }
  try {
    writeOwnerLock(stateRoot, owner);
    const verified = readOwnerLock(stateRoot);
    if (!sameOwnerToken(verified, owner)) {
      throw new OwnerLeaseError("OWNER_VERIFY_FAILED", "owner lease verification failed", verified);
    }
    return owner;
  } catch (error) {
    if (error?.code === "EEXIST") {
      throw new OwnerLeaseError("OWNER_BUSY", "another Incognito owner won the lease race", readOwnerLock(stateRoot));
    }
    throw error;
  }
}

function connectExisting(stateRoot, timeoutMs = 400, token = "") {
  if (typeof timeoutMs !== "number") {
    token = timeoutMs;
    timeoutMs = 400;
  }
  const expectedToken = token || ownerToken(readOwnerLock(stateRoot));
  return new Promise((resolve) => {
    const socket = net.connect(sockPath(stateRoot));
    let done = false;
    let response = "";
    const finish = (ok) => {
      if (done) return;
      done = true;
      socket.destroy();
      resolve(ok);
    };
    socket.setTimeout(timeoutMs);
    socket.once("connect", () => {
      try {
        socket.write(`${expectedToken ? `raise ${expectedToken}` : "raise"}\n`);
      } catch {
        finish(false);
      }
    });
    socket.on("data", (buf) => {
      response += String(buf);
      if (response.split("\n").some((line) => line.trim() === "ok")) finish(true);
      else if (response.split("\n").some((line) => line.trim() === "denied")) finish(false);
    });
    socket.once("error", () => finish(false));
    socket.once("timeout", () => finish(false));
  });
}

async function connectExistingWithRetry(stateRoot, token, options = {}) {
  const attempts = options.attempts ?? OWNER_RETRY_COUNT;
  const timeoutMs = options.timeoutMs ?? 400;
  const delayMs = options.delayMs ?? OWNER_RETRY_DELAY_MS;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    if (await connectExisting(stateRoot, timeoutMs, token)) return true;
    if (attempt + 1 < attempts && delayMs > 0) await new Promise((resolve) => setTimeout(resolve, delayMs));
  }
  return false;
}

function listenForRaise(stateRoot, onRaise, lease) {
  if (!lease || !ownerToken(lease)) {
    throw new OwnerLeaseError("OWNER_INVALID", "raise socket requires an owner lease");
  }
  if (!ownsOwnerLease(stateRoot, lease)) {
    throw new OwnerLeaseError("OWNER_NOT_HELD", "cannot listen without the current owner lease");
  }
  if (!removeSocket(stateRoot)) {
    throw new Error("cannot replace the existing raise socket");
  }
  fs.mkdirSync(stateRoot, { recursive: true, mode: 0o700 });
  const expectedToken = ownerToken(lease);
  const server = net.createServer((socket) => {
    let request = "";
    socket.on("error", () => {
      /* Client may already be gone; do not crash the main process. */
    });
    socket.on("data", (buf) => {
      request += String(buf);
      if (!request.includes("\n")) return;
      const line = request.slice(0, request.indexOf("\n")).trim();
      const expectedRequest = expectedToken ? `raise ${expectedToken}` : "raise";
      if (line !== expectedRequest) {
        try {
          socket.end("denied\n");
        } catch {
          /* EPIPE: peer already closed. */
        }
        return;
      }
      onRaise();
      try {
        if (!socket.destroyed) socket.end("ok\n");
      } catch {
        /* EPIPE: peer already closed. */
      }
    });
  });
  server.listen(sockPath(stateRoot));
  return server;
}

function singleFlight(holder, start) {
  if (holder.current) return holder.current;
  holder.current = Promise.resolve()
    .then(start)
    .finally(() => {
      holder.current = null;
    });
  return holder.current;
}

export {
  LOCK_NAME,
  SOCK_NAME,
  targetIdFromExec,
  targetStateDir,
  processIdentity,
  ownerToken,
  ownerMatchesLive,
  writeOwnerLock,
  readOwnerLockState,
  readOwnerLock,
  clearOwnerLock,
  currentOwner,
  staleOwner,
  OwnerLeaseError,
  ownsOwnerLease,
  acquireOwnerLease,
  connectExisting,
  connectExistingWithRetry,
  listenForRaise,
  singleFlight,
};
