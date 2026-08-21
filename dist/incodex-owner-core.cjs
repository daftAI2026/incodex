// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const LOCK_NAME = "incognito.lock";
const SOCK_NAME = "incognito.sock";
const OWNER_RETRY_COUNT = 5;
const OWNER_RETRY_DELAY_MS = 100;
const TAKEOVER_CLAIM_NAME = ".incognito.lock.takeover";
const TAKEOVER_CLAIM_OWNER_NAME = "owner";
const TAKEOVER_CLAIM_RECLAIM_NAME = ".reclaim";
const RECLAIM_MARKER_PREFIX = "marker.";
const RECLAIM_RELEASED_STATE = "released";
const RECLAIM_GENERATION_WIDTH = 16;
const RECLAIM_GENERATION_MAX = Number.MAX_SAFE_INTEGER - 1;
function targetIdFromExec(execPath) {
    return crypto.createHash("sha256").update(execPath || "unknown").digest("hex").slice(0, 12);
}
function targetStateDir(userRoot, execPath) {
    return path.join(userRoot, "targets", targetIdFromExec(execPath));
}
function lockPath(stateRoot) {
    return path.join(stateRoot, LOCK_NAME);
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
function hasOwnerProcessIdentity(owner) {
    return Boolean(owner && (nonEmptyString(owner.processStartIdentity) || nonEmptyString(owner.startedAt)));
}
function hasOwnerExecutableIdentity(owner) {
    return Boolean(owner && (nonEmptyString(owner.execIdentity) || nonEmptyString(owner.execPath)));
}
function nonEmptyString(value) {
    return typeof value === "string" && value.trim().length > 0;
}
function hasReliableOwnerIdentity(owner) {
    return hasOwnerProcessIdentity(owner) && hasOwnerExecutableIdentity(owner);
}
function executableIdentity(owner) {
    if (nonEmptyString(owner?.execIdentity))
        return owner.execIdentity;
    if (nonEmptyString(owner?.comm))
        return owner.comm;
    return nonEmptyString(owner?.execPath) ? path.basename(owner.execPath) : "";
}
function ownerMatchesLive(owner, live) {
    if (!hasReliableOwnerIdentity(owner) || !hasReliableOwnerIdentity(live))
        return false;
    const ownerStart = owner.processStartIdentity || owner.startedAt;
    const liveStart = live.processStartIdentity || live.startedAt;
    if (owner.pid !== live.pid || ownerStart !== liveStart)
        return false;
    const ownerExec = executableIdentity(owner);
    const liveExec = executableIdentity(live);
    return Boolean(ownerExec && liveExec && ownerExec === liveExec);
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
        if (!hasReliableOwnerIdentity(owner)) {
            return { kind: "unverifiable", owner, reason: "owner lock has no reliable process and executable identity" };
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
    if (!hasReliableOwnerIdentity(owner))
        return false;
    const live = processIdentity(owner.pid);
    if (!live)
        return !pidAlive(owner.pid);
    return !ownerMatchesLive(owner, live);
}
function staleOwner(stateRoot) {
    const state = readOwnerLockState(stateRoot);
    if (state.kind !== "valid")
        return state.kind === "missing";
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
function ownsOwnerLease(stateRoot, expectedOwner) {
    const current = readOwnerLock(stateRoot);
    return sameOwnerToken(current, expectedOwner);
}
module.exports = {
    LOCK_NAME,
    SOCK_NAME,
    OWNER_RETRY_COUNT,
    OWNER_RETRY_DELAY_MS,
    TAKEOVER_CLAIM_NAME,
    TAKEOVER_CLAIM_OWNER_NAME,
    TAKEOVER_CLAIM_RECLAIM_NAME,
    RECLAIM_MARKER_PREFIX,
    RECLAIM_RELEASED_STATE,
    RECLAIM_GENERATION_WIDTH,
    RECLAIM_GENERATION_MAX,
    targetIdFromExec,
    targetStateDir,
    lockPath,
    processIdentity,
    ownerToken,
    hasReliableOwnerIdentity,
    ownerMatchesLive,
    sameOwnerToken,
    pidAlive,
    sleepForOwnerRecovery,
    writeAtomicRecord,
    writeOwnerLock,
    readOwnerLockStateAt,
    readOwnerLockState,
    readOwnerLock,
    ownerLockMetadata,
    sameOwnerLockMetadata,
    currentOwner,
    staleOwnerRecord,
    staleOwner,
    OwnerLeaseError,
    ownsOwnerLease,
};
