// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.acquireOwnerLease = exports.ownsOwnerLease = exports.OwnerLeaseError = exports.staleOwner = exports.currentOwner = exports.clearOwnerLock = exports.readOwnerLock = exports.readOwnerLockState = exports.writeOwnerLock = exports.ownerMatchesLive = exports.ownerToken = exports.processIdentity = exports.ownerPortFromExec = exports.targetStateDir = exports.targetIdFromExec = exports.SOCK_NAME = exports.LOCK_NAME = void 0;
exports.sockPath = sockPath;
exports.connectExisting = connectExisting;
exports.connectExistingWithRetry = connectExistingWithRetry;
exports.listenForRaise = listenForRaise;
exports.singleFlight = singleFlight;
const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const core = require("./incodex-owner-core.cjs");
const recovery = require("./incodex-owner-recovery.cjs");
const { LOCK_NAME, SOCK_NAME, targetIdFromExec, targetStateDir, ownerPortFromExec, ownerToken, ownerMatchesLive, writeOwnerLock, readOwnerLockState, readOwnerLock, currentOwner, staleOwner, OwnerLeaseError, ownsOwnerLease, } = core;
exports.LOCK_NAME = LOCK_NAME;
exports.SOCK_NAME = SOCK_NAME;
exports.targetIdFromExec = targetIdFromExec;
exports.targetStateDir = targetStateDir;
exports.ownerPortFromExec = ownerPortFromExec;
exports.ownerToken = ownerToken;
exports.ownerMatchesLive = ownerMatchesLive;
exports.writeOwnerLock = writeOwnerLock;
exports.readOwnerLockState = readOwnerLockState;
exports.readOwnerLock = readOwnerLock;
exports.currentOwner = currentOwner;
exports.staleOwner = staleOwner;
exports.OwnerLeaseError = OwnerLeaseError;
exports.ownsOwnerLease = ownsOwnerLease;
const { clearOwnerLock, acquireOwnerLease, setRaiseHandler } = recovery;
exports.clearOwnerLock = clearOwnerLock;
exports.acquireOwnerLease = acquireOwnerLease;
const processIdentity = core.processIdentity;
exports.processIdentity = processIdentity;
function sockPath(stateRoot) {
    return path.join(stateRoot, SOCK_NAME);
}
function connectExisting(stateRoot, timeoutMs = 400, token = "") {
    if (typeof timeoutMs !== "number") {
        token = timeoutMs;
        timeoutMs = 400;
    }
    const owner = readOwnerLock(stateRoot);
    if (!owner)
        return Promise.resolve(false);
    const expectedToken = token || ownerToken(owner);
    return new Promise((resolve) => {
        const socket = net.connect({ host: "127.0.0.1", port: ownerPortFromExec(owner.execPath) });
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
    const attempts = options.attempts ?? 5;
    const timeoutMs = options.timeoutMs ?? 400;
    const delayMs = options.delayMs ?? 100;
    for (let attempt = 0; attempt < attempts; attempt += 1) {
        if (await connectExisting(stateRoot, timeoutMs, token))
            return true;
        if (attempt + 1 < attempts && delayMs > 0)
            await new Promise((resolve) => setTimeout(resolve, delayMs));
    }
    return false;
}
function listenForRaise(stateRoot, onRaise, lease) {
    if (!lease || !ownerToken(lease))
        throw new OwnerLeaseError("OWNER_INVALID", "raise socket requires an owner lease");
    return setRaiseHandler(stateRoot, lease, onRaise);
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
if (typeof module !== "undefined")
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
