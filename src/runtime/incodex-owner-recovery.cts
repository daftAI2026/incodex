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
  staleOwnerRecord,
  OwnerLeaseError,
  lockPath,
} = core;

const activeLeases = new Map();
const PROTOCOL_MAX_BYTES = 256;
const PROTOCOL_IDLE_TIMEOUT_MS = 1_000;
const MAX_LEASE_CONNECTIONS = 32;

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
  socket.setTimeout(PROTOCOL_IDLE_TIMEOUT_MS, () => socket.destroy());
  socket.on("data", (chunk) => {
    input += chunk;
    if (Buffer.byteLength(input, "utf8") > PROTOCOL_MAX_BYTES) {
      socket.destroy();
      return;
    }
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
  let connections = 0;
  lease.server.on("connection", (socket) => {
    if (connections >= MAX_LEASE_CONNECTIONS) {
      socket.destroy();
      return;
    }
    connections += 1;
    socket.once("close", () => {
      connections -= 1;
    });
    protocolLine(socket, (line, connection) => {
      if (line === "probe") {
        connection.end("owner-ready\n");
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

async function validateDiagnosticBeforePublication(stateRoot, owner, beforeBind = false) {
  const state = readOwnerLockState(stateRoot);
  if (state.kind === "missing" || state.kind === "invalid") return;
  if (state.kind === "unverifiable") {
    throw new OwnerLeaseError("OWNER_UNVERIFIABLE", "existing owner record cannot be verified; refusing publication", state.owner);
  }
  if (sameOwnerToken(state.owner, owner)) return;
  if (beforeBind) {
    const probe = await probeOwnerPort(owner);
    if (probe.kind === "owner") return;
    if (probe.kind === "foreign") {
      throw new OwnerLeaseError("OWNER_FOREIGN_PORT", "target port is held by an unknown listener", state.owner);
    }
  }
  if (!staleOwnerRecord(state.owner)) {
    throw new OwnerLeaseError("OWNER_LEGACY_OWNER", "live legacy owner record has no kernel handshake; refusing takeover", state.owner);
  }
}

function removeOwnedDiagnosticRecord(stateRoot, expectedOwner) {
  if (!sameOwnerToken(readOwnerLock(stateRoot), expectedOwner)) return false;
  try {
    fs.rmSync(lockPath(stateRoot), { force: true });
    return true;
  } catch {
    return false;
  }
}

function closeLeaseServer(server) {
  if (!server?.listening) return Promise.resolve();
  return new Promise((resolve) => {
    try {
      server.close(() => resolve());
    } catch {
      resolve();
    }
  });
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
        if (line === "owner-ready") finish({ kind: "owner" });
        else finish({ kind: "foreign" });
      }
    });
    socket.on("error", () => finish({ kind: "unavailable" }));
    socket.on("close", () => finish({ kind: "unavailable" }));
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
  await validateDiagnosticBeforePublication(stateRoot, owner, true);

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
    await validateDiagnosticBeforePublication(stateRoot, owner);
    writeOwnerLock(stateRoot, owner);
    return owner;
  } catch (error) {
    activeLeases.delete(ownerToken(owner));
    try {
      lease.server.close();
    } catch {
      /* The listener is best effort after publication refusal. */
    }
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
  const token = ownerToken(expectedOwner);
  const lease = activeLeases.get(token);
  if (!lease) return false;
  if (!removeOwnedDiagnosticRecord(stateRoot, expectedOwner)) return false;
  activeLeases.delete(token);
  try { lease.server.close(); } catch { /* The kernel listener is already gone. */ }
  return true;
}

async function releaseOwnerLease(stateRoot, expectedOwner) {
  const token = ownerToken(expectedOwner);
  const lease = activeLeases.get(token);
  if (!lease || !removeOwnedDiagnosticRecord(stateRoot, expectedOwner)) return false;
  activeLeases.delete(token);
  await closeLeaseServer(lease.server);
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
  releaseOwnerLease,
  setRaiseHandler,
  ownsActiveLease,
};
