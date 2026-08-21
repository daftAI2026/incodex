// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.acquireOwnerLease = exports.ownsOwnerLease = exports.OwnerLeaseError = exports.staleOwner = exports.currentOwner = exports.clearOwnerLock = exports.readOwnerLock = exports.readOwnerLockState = exports.writeOwnerLock = exports.ownerMatchesLive = exports.ownerToken = exports.processIdentity = exports.targetStateDir = exports.targetIdFromExec = exports.SOCK_NAME = exports.LOCK_NAME = void 0;
exports.connectExisting = connectExisting;
exports.connectExistingWithRetry = connectExistingWithRetry;
exports.listenForRaise = listenForRaise;
exports.singleFlight = singleFlight;
const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const core = require("./incodex-owner-core.cjs");
const recovery = require("./incodex-owner-recovery.cjs");
const { LOCK_NAME, SOCK_NAME, OWNER_RETRY_COUNT, OWNER_RETRY_DELAY_MS, targetIdFromExec, targetStateDir, ownerToken, ownerMatchesLive, writeOwnerLock, readOwnerLockState, readOwnerLock, currentOwner, staleOwner, OwnerLeaseError, ownsOwnerLease, sameOwnerToken, } = core;
exports.LOCK_NAME = LOCK_NAME;
exports.SOCK_NAME = SOCK_NAME;
exports.targetIdFromExec = targetIdFromExec;
exports.targetStateDir = targetStateDir;
exports.ownerToken = ownerToken;
exports.ownerMatchesLive = ownerMatchesLive;
exports.writeOwnerLock = writeOwnerLock;
exports.readOwnerLockState = readOwnerLockState;
exports.readOwnerLock = readOwnerLock;
exports.currentOwner = currentOwner;
exports.staleOwner = staleOwner;
exports.OwnerLeaseError = OwnerLeaseError;
exports.ownsOwnerLease = ownsOwnerLease;
const { clearOwnerLock, acquireOwnerLease } = recovery;
exports.clearOwnerLock = clearOwnerLock;
exports.acquireOwnerLease = acquireOwnerLease;
const processIdentity = core.processIdentity;
exports.processIdentity = processIdentity;
function sockPath(stateRoot) {
    return path.join(stateRoot, SOCK_NAME);
}
function removeSocket(stateRoot) {
    const file = sockPath(stateRoot);
    let stats;
    try {
        stats = fs.lstatSync(file);
    }
    catch (error) {
        if (error && error.code === "ENOENT")
            return true;
        return false;
    }
    if (stats.isSymbolicLink())
        return false;
    try {
        fs.rmSync(file);
        return true;
    }
    catch (error) {
        return Boolean(error && error.code === "ENOENT");
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
            if (done)
                return;
            done = true;
            socket.destroy();
            resolve(ok);
        };
        socket.setTimeout(timeoutMs);
        socket.once("connect", () => {
            try {
                socket.write(`${expectedToken ? `raise ${expectedToken}` : "raise"}\n`);
            }
            catch {
                finish(false);
            }
        });
        socket.on("data", (buf) => {
            response += String(buf);
            if (response.split("\n").some((line) => line.trim() === "ok"))
                finish(true);
            else if (response.split("\n").some((line) => line.trim() === "denied"))
                finish(false);
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
        if (await connectExisting(stateRoot, timeoutMs, token))
            return true;
        if (attempt + 1 < attempts && delayMs > 0)
            await new Promise((resolve) => setTimeout(resolve, delayMs));
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
            if (!request.includes("\n"))
                return;
            const line = request.slice(0, request.indexOf("\n")).trim();
            const expectedRequest = expectedToken ? `raise ${expectedToken}` : "raise";
            if (line !== expectedRequest) {
                try {
                    socket.end("denied\n");
                }
                catch {
                    /* EPIPE: peer already closed. */
                }
                return;
            }
            onRaise();
            try {
                if (!socket.destroyed)
                    socket.end("ok\n");
            }
            catch {
                /* EPIPE: peer already closed. */
            }
        });
    });
    server.listen(sockPath(stateRoot));
    return server;
}
function singleFlight(holder, start) {
    if (holder.current)
        return holder.current;
    holder.current = Promise.resolve()
        .then(start)
        .finally(() => {
        holder.current = null;
    });
    return holder.current;
}
