// 该 fixture 保留 main 上稳定 Unix Runtime 的真实所有权顺序：O_EXCL 写 lock，然后无条件 unlink/listen socket。
"use strict";

const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const crypto = require("node:crypto");
const { spawnSync } = require("node:child_process");

const lockPath = (root) => path.join(root, "incognito.lock");
const socketPath = (root) => path.join(root, "incognito.sock");
const owner = () => {
  const listed = spawnSync("ps", ["-p", String(process.pid), "-o", "pid=,lstart=,comm="], { encoding: "utf8" });
  const match = listed.stdout.trim().match(/^\d+\s+(.+?)\s+(\S+)$/);
  const startedAt = match?.[1]?.trim() || "fixture";
  const execIdentity = path.basename(match?.[2] || process.execPath);
  const nonce = crypto.randomBytes(16).toString("hex");
  return {
    pid: process.pid,
    startedAt,
    processStartIdentity: startedAt,
    execPath: process.execPath,
    execIdentity,
    sessionId: "legacy-fixture",
    token: nonce,
    nonce,
  };
};

function writeOwnerLock(root, value) {
  fs.mkdirSync(root, { recursive: true, mode: 0o700 });
  const fd = fs.openSync(lockPath(root), fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL, 0o600);
  try { fs.writeSync(fd, `${JSON.stringify(value)}\n`); } finally { fs.closeSync(fd); }
}

function listenForRaise(root) {
  try { fs.rmSync(socketPath(root)); } catch { /* stable Runtime ignored unlink failure */ }
  fs.mkdirSync(root, { recursive: true, mode: 0o700 });
  const server = net.createServer((socket) => socket.end("ok\n"));
  server.listen(socketPath(root));
  return server;
}

const root = process.env.INCODEX_LEGACY_ROOT;
const result = process.env.INCODEX_LEGACY_RESULT;
const release = process.env.INCODEX_LEGACY_RELEASE;
const ready = process.env.INCODEX_LEGACY_READY;
let server;
try { writeOwnerLock(root, owner()); } catch { /* stable Runtime continued after O_EXCL failure */ }
if (ready) {
  fs.writeFileSync(ready, "owner-record-published\n");
  const timer = setInterval(() => {
    if (!fs.existsSync(release)) return;
    clearInterval(timer);
    try { fs.rmSync(lockPath(root)); } catch {}
    process.exit(0);
  }, 5);
} else try {
  server = listenForRaise(root);
  server.once("listening", () => {
    fs.writeFileSync(result, "UNIX_WON\n");
    const timer = setInterval(() => {
      if (!fs.existsSync(release)) return;
      clearInterval(timer);
      server.close(() => {
        try { fs.rmSync(socketPath(root)); } catch {}
        try { fs.rmSync(lockPath(root)); } catch {}
        process.exit(0);
      });
    }, 5);
  });
  server.once("error", () => {
    fs.writeFileSync(result, "UNIX_BLOCKED\n");
    process.exit(2);
  });
} catch {
  fs.writeFileSync(result, "UNIX_BLOCKED\n");
  process.exit(2);
}
