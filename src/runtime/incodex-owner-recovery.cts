// @ts-nocheck
"use strict";

const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const core = require("./incodex-owner-core.cts");

const {
  LOCK_NAME,
  SOCK_NAME,
  ownerPortFromExec,
  ownerToken,
  writeOwnerLock,
  readOwnerLockState,
  readOwnerLock,
  sameOwnerToken,
  OwnerLeaseError,
  lockPath,
} = core;

const activeLeases = new Map();

function claimPath(stateRoot) {
  return path.join(stateRoot, ".incognito.lock.takeover");
}

function hasForeignClaim(stateRoot) {
  try {
    fs.lstatSync(claimPath(stateRoot));
    return true;
  } catch (error) {
    return error?.code !== "ENOENT";
  }
}

function hasLegacySocket(stateRoot) {
  try {
    fs.lstatSync(path.join(stateRoot, SOCK_NAME));
    return true;
  } catch (error) {
    return error?.code !== "ENOENT";
  }
}

function protocolLine(socket, onLine) {
  let input = "";
  socket.setEncoding("utf8");
  socket.on("data", (chunk) => {
    input += chunk;
    const newline = input.indexOf("\n");
    if (newline < 0) return;
    const line = input.slice(0, newline).trim();
    onLine(line, socket);
  });
  socket.on("error", () => {});
}

function listenForLease(owner, server) {
  protocolLine(server, () => {});
  return server;
}

function createLeaseServer(owner) {
  const lease = { owner, onRaise: null, server: net.createServer() };
  lease.server.on("connection", (socket) => {
    protocolLine(socket, (line, connection) => {
      if (line === "probe") {
        connection.end(`owner ${ownerToken(owner)}\n`);
        return;
      }
      if (line === `raise ${ownerToken(owner)}`) {
        try {
          lease.onRaise?.();
          connection.end("ok\n");
        } catch {
          connection.end("denied\n");
        }
        return;
      }
      connection.end("denied\n");
    });
  });
  return lease;
}

function bindOwnerPort(owner) {
  const port = ownerPortFromExec(owner.execPath);
  const lease = createLeaseServer(owner);
  return new Promise((resolve, reject) => {
    const fail = (error) => {
      lease.server.removeAllListeners();
      try {
        lease.server.close();
      } catch {
        /* The server never reached listening. */
      }
      reject(error);
    };
    lease.server.once("error", fail);
    lease.server.listen({ host: "127.0.0.1", port, exclusive: true }, () => {
      lease.server.removeListener("error", fail);
      activeLeases.set(ownerToken(owner), lease);
      resolve(lease);
    });
  });
}

function probeOwnerPort(owner) {
  return new Promise((resolve) => {
    const socket = net.connect({ host: "127.0.0.1", port: ownerPortFromExec(owner.execPath) });
    let response = "";
    let done = false;
    const finish = (result) => {
      if (done) return;
      done = true;
      socket.destroy();
      resolve(result);
    };
    socket.setTimeout(250);
    socket.on("connect", () => socket.write("probe\n"));
    socket.on("data", (chunk) => {
      response += String(chunk);
      if (response.includes("\n")) {
        const line = response.trim();
        if (/^owner [a-f0-9]+$/.test(line)) finish({ kind: "owner" });
        else finish({ kind: "foreign" });
      }
    });
    socket.on("error", () => finish({ kind: "unavailable" }));
    socket.on("timeout", () => finish({ kind: "unavailable" }));
  });
}

async function acquireOwnerLease(stateRoot, owner) {
  if (!owner || !ownerToken(owner)) throw new OwnerLeaseError("OWNER_INVALID", "owner lease requires a token");
  if (hasForeignClaim(stateRoot)) {
    throw new OwnerLeaseError("OWNER_FOREIGN_CLAIM", "foreign takeover claim is present; refusing cleanup");
  }
  if (hasLegacySocket(stateRoot)) {
    throw new OwnerLeaseError("OWNER_LEGACY_SOCKET", "legacy Unix owner socket is present; refusing a second runtime");
  }

  let lease;
  try {
    lease = await bindOwnerPort(owner);
  } catch (error) {
    if (error?.code !== "EADDRINUSE") throw error;
    const probe = await probeOwnerPort(owner);
    if (probe.kind === "owner") throw new OwnerLeaseError("OWNER_BUSY", "another Incognito owner holds the target port");
    if (probe.kind === "foreign") throw new OwnerLeaseError("OWNER_FOREIGN_PORT", "target port is held by an unknown listener");
    throw new OwnerLeaseError("OWNER_PORT_UNAVAILABLE", "target port is occupied but does not answer the owner handshake");
  }

  try {
    writeOwnerLock(stateRoot, owner);
    return owner;
  } catch (error) {
    activeLeases.delete(ownerToken(owner));
    lease.server.close();
    throw error;
  }
}

function setRaiseHandler(stateRoot, owner, onRaise) {
  const lease = activeLeases.get(ownerToken(owner));
  if (!lease || !sameOwnerToken(readOwnerLock(stateRoot), owner)) {
    throw new OwnerLeaseError("OWNER_NOT_HELD", "cannot listen without the current owner lease");
  }
  lease.onRaise = onRaise;
  return lease.server;
}

function clearOwnerLock(stateRoot, expectedOwner) {
  const current = readOwnerLock(stateRoot);
  if (!sameOwnerToken(current, expectedOwner)) return false;
  const token = ownerToken(expectedOwner);
  const lease = activeLeases.get(token);
  if (!lease) return false;
  activeLeases.delete(token);
  try {
    lease.server.close();
  } catch {
    /* The kernel listener is already gone. */
  }
  try {
    fs.rmSync(lockPath(stateRoot), { force: true });
  } catch {
    /* The diagnostic record is non-authoritative. */
  }
  return true;
}

function ownsActiveLease(owner) {
  return Boolean(activeLeases.get(ownerToken(owner)));
}

module.exports = {
  LOCK_NAME,
  SOCK_NAME,
  acquireOwnerLease,
  clearOwnerLock,
  setRaiseHandler,
  ownsActiveLease,
};
