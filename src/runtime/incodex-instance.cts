// @ts-nocheck
"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const crypto = require("node:crypto");

const LOCK_NAME = "incognito.lock";
const SOCK_NAME = "incognito.sock";

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
  return { pid: Number(match[1]), startedAt: match[2].trim(), comm: match[3] };
}

function ownerMatchesLive(owner, live) {
  if (!owner || !live) return false;
  return (
    owner.pid === live.pid &&
    owner.startedAt === live.startedAt &&
    owner.execPath === live.execPath
  );
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
  } finally {
    fs.closeSync(fd);
  }
}

function readOwnerLock(stateRoot) {
  const file = lockPath(stateRoot);
  try {
    const stats = fs.lstatSync(file);
    if (stats.isSymbolicLink()) return null;
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch {
    return null;
  }
}

function clearOwnerLock(stateRoot) {
  const file = lockPath(stateRoot);
  try {
    const stats = fs.lstatSync(file);
    if (stats.isSymbolicLink()) return;
    fs.rmSync(file);
  } catch {
    /* ignore */
  }
  try {
    fs.rmSync(sockPath(stateRoot));
  } catch {
    /* ignore */
  }
}

function currentOwner(sessionId, execPath) {
  const live = processIdentity(process.pid);
  return {
    pid: process.pid,
    startedAt: live?.startedAt || "",
    execPath: execPath || process.execPath,
    sessionId: sessionId || "",
    nonce: crypto.randomBytes(16).toString("hex"),
  };
}

function staleOwner(stateRoot) {
  const owner = readOwnerLock(stateRoot);
  if (!owner) return true;
  const live = processIdentity(owner.pid);
  if (!live) return true;
  return !ownerMatchesLive(owner, { ...live, execPath: owner.execPath });
}

function connectExisting(stateRoot, timeoutMs = 400) {
  return new Promise((resolve) => {
    const socket = net.connect(sockPath(stateRoot));
    let done = false;
    const finish = (ok) => {
      if (done) return;
      done = true;
      socket.destroy();
      resolve(ok);
    };
    socket.setTimeout(timeoutMs);
    socket.once("connect", () => {
      socket.write("raise\n");
      finish(true);
    });
    socket.once("error", () => finish(false));
    socket.once("timeout", () => finish(false));
  });
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

function listenForRaise(stateRoot, onRaise) {
  try {
    fs.rmSync(sockPath(stateRoot));
  } catch {
    /* ignore */
  }
  fs.mkdirSync(stateRoot, { recursive: true, mode: 0o700 });
  const server = net.createServer((socket) => {
    socket.on("data", (buf) => {
      if (String(buf).includes("raise")) onRaise();
      socket.end("ok\n");
    });
  });
  server.listen(sockPath(stateRoot));
  return server;
}

export {
  LOCK_NAME,
  SOCK_NAME,
  targetIdFromExec,
  targetStateDir,
  processIdentity,
  ownerMatchesLive,
  writeOwnerLock,
  readOwnerLock,
  clearOwnerLock,
  currentOwner,
  staleOwner,
  connectExisting,
  listenForRaise,
  singleFlight,
};
