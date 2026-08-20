// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.OwnerLeaseError = exports.SOCK_NAME = exports.LOCK_NAME = void 0;
exports.targetIdFromExec = targetIdFromExec;
exports.targetStateDir = targetStateDir;
exports.processIdentity = processIdentity;
exports.ownerToken = ownerToken;
exports.ownerMatchesLive = ownerMatchesLive;
exports.writeOwnerLock = writeOwnerLock;
exports.readOwnerLockState = readOwnerLockState;
exports.readOwnerLock = readOwnerLock;
exports.clearOwnerLock = clearOwnerLock;
exports.currentOwner = currentOwner;
exports.staleOwner = staleOwner;
exports.ownsOwnerLease = ownsOwnerLease;
exports.acquireOwnerLease = acquireOwnerLease;
exports.connectExisting = connectExisting;
exports.connectExistingWithRetry = connectExistingWithRetry;
exports.listenForRaise = listenForRaise;
exports.singleFlight = singleFlight;
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const crypto = require("node:crypto");
const LOCK_NAME = "incognito.lock";
exports.LOCK_NAME = LOCK_NAME;
const SOCK_NAME = "incognito.sock";
exports.SOCK_NAME = SOCK_NAME;
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
    if (!Number.isInteger(pid) || pid <= 0)
        return null;
    const listed = spawnSync("ps", ["-p", String(pid), "-o", "pid=,lstart=,comm="], { encoding: "utf8" });
    if (listed.status !== 0 || !listed.stdout.trim())
        return null;
    const line = listed.stdout.trim();
    const match = line.match(/^(\d+)\s+(.+?)\s+(\S+)$/);
    if (!match)
        return null;
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
    if (!owner || typeof owner !== "object")
        return "";
    if (typeof owner.token === "string" && owner.token)
        return owner.token;
    if (typeof owner.nonce === "string" && owner.nonce)
        return owner.nonce;
    return "";
}
function ownerMatchesLive(owner, live) {
    if (!owner || !live)
        return false;
    const ownerStart = owner.processStartIdentity || owner.startedAt;
    const liveStart = live.processStartIdentity || live.startedAt;
    if (owner.pid !== live.pid || ownerStart !== liveStart)
        return false;
    const ownerExec = owner.execIdentity || owner.comm;
    const liveExec = live.execIdentity || live.comm;
    if (ownerExec && liveExec)
        return ownerExec === liveExec;
    if (owner.execPath && live.execPath)
        return owner.execPath === live.execPath;
    return true;
}
function sameOwnerToken(left, right) {
    const leftToken = ownerToken(left);
    const rightToken = ownerToken(right);
    return Boolean(leftToken && rightToken && leftToken === rightToken);
}
function pidAlive(pid) {
    if (!Number.isInteger(pid) || pid <= 0)
        return false;
    try {
        process.kill(pid, 0);
        return true;
    }
    catch (error) {
        return error?.code === "EPERM";
    }
}
function sleepForOwnerRecovery(ms) {
    if (ms <= 0)
        return;
    const waiter = new Int32Array(new SharedArrayBuffer(4));
    Atomics.wait(waiter, 0, 0, ms);
}
function writeAtomicRecord(file, value) {
    const temp = path.join(path.dirname(file), `.${path.basename(file)}.tmp.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`);
    let fd = null;
    try {
        fd = fs.openSync(temp, fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL | (fs.constants.O_NOFOLLOW || 0), 0o600);
        const contents = Buffer.from(`${JSON.stringify(value)}\n`);
        let offset = 0;
        while (offset < contents.length) {
            offset += fs.writeSync(fd, contents, offset, contents.length - offset, offset);
        }
        try {
            fs.fsyncSync(fd);
        }
        catch {
            /* Some test filesystems do not support fsync; the complete temp file remains private. */
        }
    }
    finally {
        if (fd !== null)
            fs.closeSync(fd);
    }
    try {
        // A hard-link publish makes the canonical path either absent or complete.
        // A crash before this point leaves only an ignored temp file, never a
        // truncated record that can poison the next launch.
        fs.linkSync(temp, file);
    }
    finally {
        try {
            fs.rmSync(temp, { force: true });
        }
        catch {
            /* The published hard link is still the authoritative record. */
        }
    }
}
function writeOwnerLock(stateRoot, owner) {
    fs.mkdirSync(stateRoot, { recursive: true, mode: 0o700 });
    return writeAtomicRecord(lockPath(stateRoot), owner);
}
function readOwnerLockStateAt(file) {
    let stats;
    try {
        stats = fs.lstatSync(file);
    }
    catch (error) {
        if (error && error.code === "ENOENT")
            return { kind: "missing", owner: null };
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
    }
    catch (error) {
        return { kind: "invalid", owner: null, reason: String(error) };
    }
}
function readOwnerLockState(stateRoot) {
    return readOwnerLockStateAt(lockPath(stateRoot));
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
function ownerLockMetadata(file) {
    try {
        const stats = fs.lstatSync(file);
        return { dev: stats.dev, ino: stats.ino, size: stats.size, mtimeMs: stats.mtimeMs };
    }
    catch {
        return null;
    }
}
function sameOwnerLockMetadata(left, right) {
    return Boolean(left &&
        right &&
        left.dev === right.dev &&
        left.ino === right.ino &&
        left.size === right.size &&
        left.mtimeMs === right.mtimeMs);
}
function clearOwnerLock(stateRoot, expectedOwner) {
    if (!expectedOwner || !ownerToken(expectedOwner))
        return false;
    const file = lockPath(stateRoot);
    const current = readOwnerLockState(stateRoot);
    const beforeMetadata = ownerLockMetadata(file);
    if (current.kind !== "valid" || !sameOwnerToken(current.owner, expectedOwner) || !beforeMetadata)
        return false;
    const candidate = path.join(stateRoot, `.${LOCK_NAME}.releasing.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`);
    try {
        // Pin the inode before the second read. A replacement owner gets a new
        // inode, so its canonical path is never eligible for this cleanup.
        fs.linkSync(file, candidate);
        const pinned = readOwnerLockStateAt(candidate);
        const canonicalMetadata = ownerLockMetadata(file);
        if (pinned.kind !== "valid" ||
            !sameOwnerToken(pinned.owner, expectedOwner) ||
            !sameOwnerLockMetadata(beforeMetadata, canonicalMetadata)) {
            return false;
        }
        fs.rmSync(file);
        return true;
    }
    catch (error) {
        return false;
    }
    finally {
        try {
            fs.rmSync(candidate, { force: true });
        }
        catch {
            /* The candidate is only a temporary inode pin. */
        }
    }
}
function currentOwner(sessionId, execPath) {
    const live = processIdentity(process.pid);
    if (!live?.processStartIdentity || !live?.execIdentity) {
        throw new OwnerLeaseError("IDENTITY_UNAVAILABLE", "cannot acquire owner lease without process identity");
    }
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
    if (!owner || !Number.isInteger(owner.pid) || owner.pid <= 0)
        return true;
    const live = processIdentity(owner.pid);
    if (!live)
        return !pidAlive(owner.pid);
    return !ownerMatchesLive(owner, live);
}
function staleOwner(stateRoot) {
    const state = readOwnerLockState(stateRoot);
    if (state.kind !== "valid")
        return state.kind === "missing" || state.kind === "invalid";
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
exports.OwnerLeaseError = OwnerLeaseError;
function ownsOwnerLease(stateRoot, expectedOwner) {
    const current = readOwnerLock(stateRoot);
    return sameOwnerToken(current, expectedOwner);
}
function quarantineInvalidOwnerLock(stateRoot) {
    const file = lockPath(stateRoot);
    const before = readOwnerLockState(stateRoot);
    const beforeMetadata = ownerLockMetadata(file);
    if (before.kind !== "invalid" || !beforeMetadata)
        return false;
    // Give a legacy writer one recovery interval to finish its record. New
    // writers never expose a partial canonical file because they publish via a
    // hard link, but this grace protects an older process still holding its fd.
    sleepForOwnerRecovery(OWNER_RETRY_DELAY_MS);
    const settled = readOwnerLockState(stateRoot);
    const settledMetadata = ownerLockMetadata(file);
    if (settled.kind !== "invalid" || !sameOwnerLockMetadata(beforeMetadata, settledMetadata))
        return false;
    const quarantine = path.join(stateRoot, `.${LOCK_NAME}.invalid.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`);
    let preserved = false;
    try {
        // Pin the malformed inode, then re-check the canonical pathname before
        // removing it. A replacement inode is never touched by this recovery.
        fs.linkSync(file, quarantine);
        const pinned = readOwnerLockStateAt(quarantine);
        const canonicalMetadata = ownerLockMetadata(file);
        if (pinned.kind !== "invalid" || !sameOwnerLockMetadata(settledMetadata, canonicalMetadata))
            return false;
        try {
            fs.rmSync(file);
            preserved = true;
            return true;
        }
        catch (error) {
            if (error && error.code === "ENOENT") {
                preserved = true;
                return true;
            }
            return false;
        }
    }
    catch (error) {
        return false;
    }
    finally {
        if (!preserved) {
            try {
                fs.rmSync(quarantine, { force: true });
            }
            catch {
                /* Keep recovery best effort and never remove the canonical path here. */
            }
        }
    }
}
function acquireOwnerLease(stateRoot, owner) {
    if (!owner || !ownerToken(owner)) {
        throw new OwnerLeaseError("OWNER_INVALID", "owner lease requires a token");
    }
    let sawUnreadable = false;
    for (let attempt = 0; attempt < OWNER_RETRY_COUNT; attempt += 1) {
        try {
            writeOwnerLock(stateRoot, owner);
            const current = readOwnerLock(stateRoot);
            if (!sameOwnerToken(current, owner)) {
                throw new OwnerLeaseError("OWNER_VERIFY_FAILED", "owner lease verification failed", current);
            }
            return owner;
        }
        catch (error) {
            if (error?.code !== "EEXIST")
                throw error;
        }
        const current = readOwnerLockState(stateRoot);
        if (current.kind === "missing")
            continue;
        if (current.kind === "invalid") {
            sawUnreadable = true;
            if (quarantineInvalidOwnerLock(stateRoot))
                continue;
            continue;
        }
        if (!staleOwnerRecord(current.owner)) {
            throw new OwnerLeaseError("OWNER_BUSY", "another Incognito owner is active", current.owner);
        }
        if (!clearOwnerLock(stateRoot, current.owner))
            continue;
    }
    const finalState = readOwnerLockState(stateRoot);
    if (finalState.kind === "valid" && !staleOwnerRecord(finalState.owner)) {
        throw new OwnerLeaseError("OWNER_BUSY", "another Incognito owner won the lease race", finalState.owner);
    }
    if (finalState.kind === "invalid" || sawUnreadable) {
        throw new OwnerLeaseError("OWNER_UNREADABLE", "owner lease is not readable");
    }
    throw new OwnerLeaseError("OWNER_RACE", "owner lease changed during acquisition");
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
