// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.acquireOwnerLease = exports.ownsOwnerLease = exports.OwnerLeaseError = exports.staleOwnerRecord = exports.staleOwner = exports.currentOwner = exports.releaseOwnerLease = exports.clearOwnerLock = exports.readOwnerRecords = exports.readOwnerLock = exports.readOwnerLockState = exports.writeOwnerLockExclusive = exports.writeOwnerLock = exports.ownerMatchesLive = exports.ownerToken = exports.processIdentity = exports.ownerPortFromExec = exports.targetStateDir = exports.targetIdFromExec = exports.SOCK_NAME = exports.LOCK_NAME = void 0;
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
const { LOCK_NAME, SOCK_NAME, targetIdFromExec, targetStateDir, ownerPortFromExec, ownerToken, ownerMatchesLive, writeOwnerLock, writeOwnerLockExclusive, readOwnerLockState, readOwnerLock, readOwnerRecords, currentOwner, staleOwner, staleOwnerRecord, OwnerLeaseError, ownsOwnerLease, } = core;
exports.LOCK_NAME = LOCK_NAME;
exports.SOCK_NAME = SOCK_NAME;
exports.targetIdFromExec = targetIdFromExec;
exports.targetStateDir = targetStateDir;
exports.ownerPortFromExec = ownerPortFromExec;
exports.ownerToken = ownerToken;
exports.ownerMatchesLive = ownerMatchesLive;
exports.writeOwnerLock = writeOwnerLock;
exports.writeOwnerLockExclusive = writeOwnerLockExclusive;
exports.readOwnerLockState = readOwnerLockState;
exports.readOwnerLock = readOwnerLock;
exports.readOwnerRecords = readOwnerRecords;
exports.currentOwner = currentOwner;
exports.staleOwner = staleOwner;
exports.staleOwnerRecord = staleOwnerRecord;
exports.OwnerLeaseError = OwnerLeaseError;
exports.ownsOwnerLease = ownsOwnerLease;
const { clearOwnerLock, releaseOwnerLease, acquireOwnerLease, setRaiseHandler } = recovery;
exports.clearOwnerLock = clearOwnerLock;
exports.releaseOwnerLease = releaseOwnerLease;
exports.acquireOwnerLease = acquireOwnerLease;
const processIdentity = core.processIdentity;
exports.processIdentity = processIdentity;
function sockPath(stateRoot) {
    return path.join(stateRoot, SOCK_NAME);
}
function connectToOwner(owner, expectedToken, timeoutMs) {
    return new Promise((resolve) => {
        const socket = net.connect({ host: "127.0.0.1", port: ownerPortFromExec(owner.execPath) });
        let done = false;
        let response = "";
        let deadline;
        const finish = (ok) => {
            if (done)
                return;
            done = true;
            clearTimeout(deadline);
            socket.destroy();
            resolve(ok);
        };
        socket.setTimeout(timeoutMs);
        deadline = setTimeout(() => finish(false), timeoutMs);
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
            if (Buffer.byteLength(response, "utf8") > 256) {
                finish(false);
                return;
            }
            if (response.split("\n").some((line) => line.trim() === "ok"))
                finish(true);
            else if (response.split("\n").some((line) => line.trim() === "denied"))
                finish(false);
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
        if (await connectToOwner(state.owner, token || ownerToken(state.owner), timeoutMs))
            return true;
    }
    return false;
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
        writeOwnerLockExclusive,
        readOwnerLockState,
        readOwnerLock,
        readOwnerRecords,
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
    };
