// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const core = require("./incodex-owner-core.cjs");
const { LOCK_NAME, SOCK_NAME, ownerPortFromExec, ownerToken, writeOwnerLockExclusive, writeOwnerRecordExclusive, readOwnerLockState, readOwnerLockStateAt, readOwnerRecords, isOwnerQuarantinePath, sameOwnerToken, staleOwnerRecord, OwnerLeaseError, lockPath, activeOwnerPath, } = core;
const activeLeases = new Map();
const PROTOCOL_MAX_BYTES = 256;
const PROTOCOL_IDLE_TIMEOUT_MS = 1_000;
const PROTOCOL_DEADLINE_MS = 1_000;
const MAX_LEASE_CONNECTIONS = 32;
function claimPath(stateRoot) {
    return path.join(stateRoot, ".incognito.lock.takeover");
}
function hasForeignClaim(stateRoot) {
    try {
        fs.lstatSync(claimPath(stateRoot));
        return true;
    }
    catch (error) {
        return error?.code !== "ENOENT";
    }
}
function hasLegacySocket(stateRoot) {
    try {
        fs.lstatSync(path.join(stateRoot, SOCK_NAME));
        return true;
    }
    catch (error) {
        return error?.code !== "ENOENT";
    }
}
function protocolLine(socket, onLine) {
    let input = "";
    socket.setEncoding("utf8");
    const deadline = setTimeout(() => socket.destroy(), PROTOCOL_DEADLINE_MS);
    socket.setTimeout(PROTOCOL_IDLE_TIMEOUT_MS, () => socket.destroy());
    socket.once("close", () => clearTimeout(deadline));
    socket.on("data", (chunk) => {
        input += chunk;
        if (Buffer.byteLength(input, "utf8") > PROTOCOL_MAX_BYTES) {
            socket.destroy();
            return;
        }
        const newline = input.indexOf("\n");
        if (newline < 0)
            return;
        const line = input.slice(0, newline).trim();
        onLine(line, socket);
    });
    socket.on("error", () => { });
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
                }
                catch {
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
    if (state.kind === "missing" || state.kind === "invalid")
        return;
    if (state.kind === "unverifiable") {
        throw new OwnerLeaseError("OWNER_UNVERIFIABLE", "existing owner record cannot be verified; refusing publication", state.owner);
    }
    if (sameOwnerToken(state.owner, owner))
        return;
    if (beforeBind) {
        const probe = await probeOwnerPort(owner);
        if (probe.kind === "owner") {
            throw new OwnerLeaseError("OWNER_BUSY", "another Incognito owner holds the target port", state.owner);
        }
        if (probe.kind === "foreign") {
            throw new OwnerLeaseError("OWNER_FOREIGN_PORT", "target port is held by an unknown listener", state.owner);
        }
    }
    if (!staleOwnerRecord(state.owner)) {
        throw new OwnerLeaseError("OWNER_LEGACY_OWNER", "live legacy owner record has no kernel handshake; refusing takeover", state.owner);
    }
}
function prepareInitialDiagnostic(stateRoot) {
    const state = readOwnerLockState(stateRoot);
    if (state.kind === "missing")
        return;
    if (state.kind === "valid" && !staleOwnerRecord(state.owner)) {
        throw new OwnerLeaseError("OWNER_LEGACY_OWNER", "live owner record won diagnostic publication", state.owner);
    }
    if (state.kind === "unverifiable") {
        throw new OwnerLeaseError("OWNER_UNVERIFIABLE", "owner record cannot be verified for replacement", state.owner);
    }
}
function removeOwnedDiagnosticRecord(stateRoot, expectedOwner, diagnosticPath = lockPath(stateRoot)) {
    if (!sameOwnerToken(readOwnerLockStateAt(diagnosticPath).owner, expectedOwner))
        return false;
    try {
        fs.rmSync(diagnosticPath, { force: true });
        return true;
    }
    catch {
        return false;
    }
}
function closeLeaseServer(server) {
    if (!server?.listening)
        return Promise.resolve();
    return new Promise((resolve) => {
        try {
            server.close(() => resolve());
        }
        catch {
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
            }
            catch {
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
        let deadline;
        const finish = (result) => {
            if (done)
                return;
            done = true;
            clearTimeout(deadline);
            socket.destroy();
            resolve(result);
        };
        socket.setTimeout(250);
        deadline = setTimeout(() => finish({ kind: "unavailable" }), 250);
        socket.on("connect", () => socket.write("probe\n"));
        socket.on("data", (chunk) => {
            response += String(chunk);
            if (Buffer.byteLength(response, "utf8") > 256) {
                finish({ kind: "unavailable" });
                return;
            }
            if (response.includes("\n")) {
                const line = response.trim();
                if (line === "owner-ready")
                    finish({ kind: "owner" });
                else
                    finish({ kind: "foreign" });
            }
        });
        socket.on("error", () => finish({ kind: "unavailable" }));
        socket.on("close", () => finish({ kind: "unavailable" }));
        socket.on("timeout", () => finish({ kind: "unavailable" }));
    });
}
async function acquireOwnerLease(stateRoot, owner) {
    if (!owner || !ownerToken(owner))
        throw new OwnerLeaseError("OWNER_INVALID", "owner lease requires a token");
    const records = readOwnerRecords(stateRoot);
    if (records.some(({ path: recordPath, state }) => isOwnerQuarantinePath(recordPath) || state.kind === "unverifiable")) {
        throw new OwnerLeaseError("OWNER_UNVERIFIABLE", "existing owner record cannot be verified; refusing publication");
    }
    if (hasForeignClaim(stateRoot)) {
        throw new OwnerLeaseError("OWNER_FOREIGN_CLAIM", "foreign takeover claim is present; refusing cleanup");
    }
    if (hasLegacySocket(stateRoot)) {
        throw new OwnerLeaseError("OWNER_LEGACY_SOCKET", "legacy Unix owner socket is present; quiesce the old Runtime before TCP cutover");
    }
    await validateDiagnosticBeforePublication(stateRoot, owner, true);
    let lease;
    let diagnosticPath = lockPath(stateRoot);
    try {
        prepareInitialDiagnostic(stateRoot);
        try {
            writeOwnerLockExclusive(stateRoot, owner);
        }
        catch (error) {
            if (error?.code !== "EEXIST")
                throw error;
            diagnosticPath = activeOwnerPath(stateRoot, ownerToken(owner));
            writeOwnerRecordExclusive(diagnosticPath, owner);
        }
        lease = await bindOwnerPort(owner);
        lease.diagnosticPath = diagnosticPath;
    }
    catch (error) {
        removeOwnedDiagnosticRecord(stateRoot, owner, diagnosticPath);
        if (error?.code !== "EADDRINUSE")
            throw error;
        const probe = await probeOwnerPort(owner);
        if (probe.kind === "owner")
            throw new OwnerLeaseError("OWNER_BUSY", "another Incognito owner holds the target port");
        if (probe.kind === "foreign")
            throw new OwnerLeaseError("OWNER_FOREIGN_PORT", "target port is held by an unknown listener");
        throw new OwnerLeaseError("OWNER_PORT_UNAVAILABLE", "target port is occupied but does not answer the owner handshake");
    }
    try {
        if (!lease?.server?.listening)
            throw new OwnerLeaseError("OWNER_NOT_HELD", "kernel owner listener is not active");
        return owner;
    }
    catch (error) {
        activeLeases.delete(ownerToken(owner));
        try {
            lease.server.close();
        }
        catch {
            /* The listener is best effort after publication refusal. */
        }
        throw error;
    }
}
function setRaiseHandler(stateRoot, owner, onRaise) {
    const lease = activeLeases.get(ownerToken(owner));
    if (!lease || !lease.server?.listening) {
        throw new OwnerLeaseError("OWNER_NOT_HELD", "cannot listen without the current owner lease");
    }
    lease.onRaise = onRaise;
    return lease.server;
}
function clearOwnerLock(stateRoot, expectedOwner) {
    const token = ownerToken(expectedOwner);
    const lease = activeLeases.get(token);
    if (!lease)
        return false;
    if (!removeOwnedDiagnosticRecord(stateRoot, expectedOwner, lease.diagnosticPath))
        return false;
    activeLeases.delete(token);
    try {
        lease.server.close();
    }
    catch { /* The kernel listener is already gone. */ }
    return true;
}
async function releaseOwnerLease(stateRoot, expectedOwner) {
    const token = ownerToken(expectedOwner);
    const lease = activeLeases.get(token);
    if (!lease || !removeOwnedDiagnosticRecord(stateRoot, expectedOwner, lease.diagnosticPath))
        return false;
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
