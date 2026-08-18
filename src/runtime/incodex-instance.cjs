"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const crypto = require("node:crypto");

const LOCK_NAME = "incognito.lock";
const SOCK_NAME = "incognito.sock";

function lockPath(userRoot) {
  return path.join(userRoot, LOCK_NAME);
}

function sockPath(userRoot) {
  return path.join(userRoot, SOCK_NAME);
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

function writeOwnerLock(userRoot, owner) {
  const file = lockPath(userRoot);
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

function readOwnerLock(userRoot) {
  const file = lockPath(userRoot);
  try {
    const stats = fs.lstatSync(file);
    if (stats.isSymbolicLink()) return null;
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch {
    return null;
  }
}

function clearOwnerLock(userRoot) {
  const file = lockPath(userRoot);
  try {
    const stats = fs.lstatSync(file);
    if (stats.isSymbolicLink()) return;
    fs.rmSync(file);
  } catch {
    /* ignore */
  }
  try {
    fs.rmSync(sockPath(userRoot));
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

function staleOwner(userRoot) {
  const owner = readOwnerLock(userRoot);
  if (!owner) return true;
  const live = processIdentity(owner.pid);
  if (!live) return true;
  return !ownerMatchesLive(owner, { ...live, execPath: owner.execPath });
}

function connectExisting(userRoot, timeoutMs = 400) {
  return new Promise((resolve) => {
    const socket = net.connect(sockPath(userRoot));
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

function listenForRaise(userRoot, onRaise) {
  try {
    fs.rmSync(sockPath(userRoot));
  } catch {
    /* ignore */
  }
  const server = net.createServer((socket) => {
    socket.on("data", (buf) => {
      if (String(buf).includes("raise")) onRaise();
      socket.end("ok\n");
    });
  });
  server.listen(sockPath(userRoot));
  return server;
}

module.exports = {
  LOCK_NAME,
  SOCK_NAME,
  processIdentity,
  ownerMatchesLive,
  writeOwnerLock,
  readOwnerLock,
  clearOwnerLock,
  currentOwner,
  staleOwner,
  connectExisting,
  listenForRaise,
};
