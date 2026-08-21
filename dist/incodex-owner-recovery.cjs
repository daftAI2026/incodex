// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const core = require("./incodex-owner-core.cjs");
const { LOCK_NAME, OWNER_RETRY_COUNT, OWNER_RETRY_DELAY_MS, TAKEOVER_CLAIM_NAME, TAKEOVER_CLAIM_OWNER_NAME, TAKEOVER_CLAIM_RECLAIM_NAME, RECLAIM_MARKER_PREFIX, RECLAIM_RELEASED_STATE, RECLAIM_GENERATION_WIDTH, RECLAIM_GENERATION_MAX, processIdentity, ownerToken, hasReliableOwnerIdentity, ownerMatchesLive, sameOwnerToken, pidAlive, sleepForOwnerRecovery, writeAtomicRecord, writeOwnerLock, readOwnerLockStateAt, readOwnerLockState, readOwnerLock, ownerLockMetadata, sameOwnerLockMetadata, staleOwnerRecord, OwnerLeaseError, lockPath, } = core;
function pauseBeforeTakeoverUnlink() {
    const pauseFile = process.env.INCODEX_TEST_TAKEOVER_PAUSE_FILE;
    const releaseFile = process.env.INCODEX_TEST_TAKEOVER_RELEASE_FILE;
    if (!pauseFile || !releaseFile)
        return;
    try {
        fs.writeFileSync(pauseFile, `${process.pid}\n`, { flag: "wx", mode: 0o600 });
    }
    catch {
        return;
    }
    const deadline = Date.now() + 5000;
    const waiter = new Int32Array(new SharedArrayBuffer(4));
    while (!fs.existsSync(releaseFile) && Date.now() < deadline) {
        Atomics.wait(waiter, 0, 0, 5);
    }
}
function pauseBeforeReclaimHandoff() {
    const pauseFile = process.env.INCODEX_TEST_RECLAIM_HANDOFF_PAUSE_FILE;
    const releaseFile = process.env.INCODEX_TEST_RECLAIM_HANDOFF_RELEASE_FILE;
    if (!pauseFile || !releaseFile)
        return;
    try {
        fs.writeFileSync(pauseFile, `${process.pid}\n`, { flag: "wx", mode: 0o600 });
    }
    catch {
        return;
    }
    const deadline = Date.now() + 5000;
    const waiter = new Int32Array(new SharedArrayBuffer(4));
    while (!fs.existsSync(releaseFile) && Date.now() < deadline) {
        Atomics.wait(waiter, 0, 0, 5);
    }
}
function takeoverClaimPath(stateRoot) {
    return path.join(stateRoot, TAKEOVER_CLAIM_NAME);
}
function takeoverClaimOwnerPath(stateRoot) {
    return path.join(takeoverClaimPath(stateRoot), TAKEOVER_CLAIM_OWNER_NAME);
}
function takeoverClaimReclaimPath(stateRoot) {
    return path.join(takeoverClaimPath(stateRoot), TAKEOVER_CLAIM_RECLAIM_NAME);
}
function readTakeoverClaimState(stateRoot) {
    const file = takeoverClaimPath(stateRoot);
    let stats;
    try {
        stats = fs.lstatSync(file);
    }
    catch (error) {
        if (error && error.code === "ENOENT")
            return { kind: "missing", owner: null };
        return { kind: "invalid", owner: null, reason: String(error) };
    }
    if (stats.isDirectory()) {
        const owner = readOwnerLockStateAt(takeoverClaimOwnerPath(stateRoot));
        return owner.kind === "missing"
            ? { kind: "invalid", owner: null, reason: "takeover claim has no owner record" }
            : owner;
    }
    return {
        kind: "foreign",
        owner: null,
        reason: "takeover claim is a foreign regular file; refusing cleanup",
    };
}
function takeoverClaimMetadata(stateRoot) {
    const file = takeoverClaimPath(stateRoot);
    try {
        const stats = fs.lstatSync(file);
        return { dev: stats.dev, ino: stats.ino };
    }
    catch {
        return null;
    }
}
function sameTakeoverClaimMetadata(left, right) {
    return Boolean(left && right && left.dev === right.dev && left.ino === right.ino);
}
function takeoverClaimOwner() {
    const live = processIdentity(process.pid);
    return {
        pid: process.pid,
        startedAt: live?.startedAt || "",
        processStartIdentity: live?.processStartIdentity || "",
        execIdentity: live?.execIdentity || "",
        token: crypto.randomBytes(16).toString("hex"),
    };
}
function takeoverClaimIsStale(owner) {
    if (!owner || !Number.isInteger(owner.pid) || owner.pid <= 0)
        return false;
    if (!hasReliableOwnerIdentity(owner))
        return false;
    const live = processIdentity(owner.pid);
    if (!live)
        return !pidAlive(owner.pid);
    return !ownerMatchesLive(owner, live);
}
function reclaimMarkerPath(stateRoot, generation) {
    return path.join(takeoverClaimReclaimPath(stateRoot), `${RECLAIM_MARKER_PREFIX}${String(generation).padStart(RECLAIM_GENERATION_WIDTH, "0")}`);
}
function parseReclaimGeneration(name) {
    if (name === TAKEOVER_CLAIM_OWNER_NAME) {
        return { error: "foreign reclaim marker record; refusing cleanup" };
    }
    if (!name.startsWith(RECLAIM_MARKER_PREFIX))
        return null;
    const digits = name.slice(RECLAIM_MARKER_PREFIX.length);
    if (!/^\d{1,16}$/.test(digits)) {
        return { error: `reclaim marker generation is malformed: ${name}` };
    }
    const generation = Number(digits);
    if (!Number.isSafeInteger(generation) || generation < 1 || generation > RECLAIM_GENERATION_MAX) {
        return { error: `reclaim marker generation is out of bounds: ${name}` };
    }
    return { generation };
}
function reclaimMarkerEntries(stateRoot) {
    const root = takeoverClaimReclaimPath(stateRoot);
    let names;
    try {
        names = fs.readdirSync(root);
    }
    catch (error) {
        if (error?.code === "ENOENT")
            return [];
        return null;
    }
    const entries = [];
    for (const name of names) {
        const parsed = parseReclaimGeneration(name);
        if (parsed?.error)
            return { error: parsed.error };
        if (!parsed)
            continue;
        const file = path.join(root, name);
        entries.push({ file, generation: parsed.generation, state: readOwnerLockStateAt(file) });
    }
    return entries;
}
function reclaimMarkerEntriesError(entries) {
    if (Array.isArray(entries))
        return null;
    return entries?.error || "reclaim markers are unreadable";
}
function reclaimMarkerIsReleased(entry) {
    return entry.state.kind === "valid" && entry.state.owner?.leaseState === RECLAIM_RELEASED_STATE;
}
function acquireReclaimMarker(stateRoot) {
    const owner = takeoverClaimOwner();
    if (!hasReliableOwnerIdentity(owner))
        return null;
    const root = takeoverClaimReclaimPath(stateRoot);
    fs.mkdirSync(root, { recursive: true, mode: 0o700 });
    for (let attempt = 0; attempt < OWNER_RETRY_COUNT; attempt += 1) {
        const entries = reclaimMarkerEntries(stateRoot);
        const entriesError = reclaimMarkerEntriesError(entries);
        if (entriesError) {
            throw new OwnerLeaseError("OWNER_RECLAIM_UNREADABLE", entriesError);
        }
        let generation = 0;
        for (const entry of entries) {
            generation = Math.max(generation, entry.generation);
            if (reclaimMarkerIsReleased(entry))
                continue;
            if (entry.state.kind === "invalid" || entry.state.kind === "unverifiable")
                return null;
            if (entry.state.kind === "valid" && !takeoverClaimIsStale(entry.state.owner))
                return null;
        }
        // Markers are append-only generations. A stale generation is never
        // renamed or deleted, so a delayed reclaimer can only publish the next
        // unique slot; it cannot move a newer live marker out from under it.
        pauseBeforeReclaimHandoff();
        const settled = reclaimMarkerEntries(stateRoot);
        const settledError = reclaimMarkerEntriesError(settled);
        if (settledError) {
            throw new OwnerLeaseError("OWNER_RECLAIM_UNREADABLE", settledError);
        }
        let settledGeneration = generation;
        for (const entry of settled) {
            settledGeneration = Math.max(settledGeneration, entry.generation);
            if (reclaimMarkerIsReleased(entry))
                continue;
            if (entry.state.kind === "invalid" || entry.state.kind === "unverifiable")
                return null;
            if (entry.state.kind === "valid" && !takeoverClaimIsStale(entry.state.owner))
                return null;
        }
        if (settledGeneration !== generation)
            continue;
        try {
            writeAtomicRecord(reclaimMarkerPath(stateRoot, generation + 1), owner);
            return owner;
        }
        catch (error) {
            if (error?.code === "EEXIST")
                continue;
            return null;
        }
    }
    return null;
}
function releaseReclaimMarker(stateRoot, expectedOwner) {
    if (!expectedOwner || !ownerToken(expectedOwner))
        return false;
    const entries = reclaimMarkerEntries(stateRoot);
    if (!Array.isArray(entries))
        return false;
    const entry = entries.find((candidate) => candidate.state.kind === "valid" && sameOwnerToken(candidate.state.owner, expectedOwner));
    if (!entry)
        return false;
    const released = {
        ...entry.state.owner,
        leaseState: RECLAIM_RELEASED_STATE,
    };
    const temporary = path.join(path.dirname(entry.file), `.${path.basename(entry.file)}.released.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`);
    try {
        writeAtomicRecord(temporary, released);
        fs.renameSync(temporary, entry.file);
        return true;
    }
    catch (error) {
        try {
            fs.rmSync(temporary, { force: true });
        }
        catch {
            /* Keep the live marker when an atomic release cannot be published. */
        }
        return Boolean(error?.code === "ENOENT");
    }
}
function publishTakeoverClaim(stateRoot, owner) {
    const file = takeoverClaimPath(stateRoot);
    const temporary = path.join(stateRoot, `.${TAKEOVER_CLAIM_NAME}.tmp.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`);
    fs.mkdirSync(temporary, { recursive: false, mode: 0o700 });
    try {
        writeAtomicRecord(path.join(temporary, TAKEOVER_CLAIM_OWNER_NAME), owner);
        try {
            // The destination is either absent or a non-empty claim directory. A
            // directory rename therefore gives us an atomic no-replace publish.
            fs.renameSync(temporary, file);
            return owner;
        }
        catch (error) {
            if (["EEXIST", "ENOTEMPTY", "ENOTDIR"].includes(error?.code)) {
                const conflict = new Error("takeover claim already exists");
                conflict.code = "EEXIST";
                throw conflict;
            }
            throw error;
        }
    }
    finally {
        try {
            fs.rmSync(temporary, { recursive: true, force: true });
        }
        catch {
            /* The temporary claim is private and best-effort after publication. */
        }
    }
}
function removeTakeoverClaimIfStale(stateRoot, expectedState) {
    const file = takeoverClaimPath(stateRoot);
    const current = readTakeoverClaimState(stateRoot);
    if (current.kind !== expectedState.kind)
        return false;
    if (current.kind === "valid" && !takeoverClaimIsStale(current.owner))
        return false;
    if (!fs.existsSync(file))
        return false;
    let stats;
    try {
        stats = fs.lstatSync(file);
    }
    catch {
        return false;
    }
    if (!stats.isDirectory())
        return false;
    const beforeMetadata = takeoverClaimMetadata(stateRoot);
    if (!beforeMetadata)
        return false;
    const markerOwner = acquireReclaimMarker(stateRoot);
    if (!markerOwner)
        return false;
    try {
        const claimed = readTakeoverClaimState(stateRoot);
        const claimedMetadata = takeoverClaimMetadata(stateRoot);
        if (claimed.kind !== expectedState.kind ||
            (claimed.kind === "valid" && !takeoverClaimIsStale(claimed.owner)) ||
            !sameTakeoverClaimMetadata(beforeMetadata, claimedMetadata)) {
            return false;
        }
        pauseBeforeTakeoverUnlink();
        const finalState = readTakeoverClaimState(stateRoot);
        const finalMetadata = takeoverClaimMetadata(stateRoot);
        if (finalState.kind !== expectedState.kind ||
            (finalState.kind === "valid" && !takeoverClaimIsStale(finalState.owner)) ||
            !sameTakeoverClaimMetadata(beforeMetadata, finalMetadata)) {
            return false;
        }
        const quarantine = path.join(stateRoot, `.${TAKEOVER_CLAIM_NAME}.stale.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`);
        try {
            // Only the process that created the fixed marker may move this
            // non-empty directory. A new claim is published at a fresh inode.
            fs.renameSync(file, quarantine);
            fs.rmSync(quarantine, { recursive: true, force: true });
            return true;
        }
        catch {
            try {
                fs.rmSync(quarantine, { recursive: true, force: true });
            }
            catch {
                /* Never touch the canonical claim after a failed handoff. */
            }
            return false;
        }
    }
    finally {
        releaseReclaimMarker(stateRoot, markerOwner);
    }
}
function acquireTakeoverClaim(stateRoot) {
    fs.mkdirSync(stateRoot, { recursive: true, mode: 0o700 });
    for (let attempt = 0; attempt < OWNER_RETRY_COUNT; attempt += 1) {
        try {
            const owner = takeoverClaimOwner();
            if (!hasReliableOwnerIdentity(owner))
                return null;
            return publishTakeoverClaim(stateRoot, owner);
        }
        catch (error) {
            if (error?.code !== "EEXIST")
                throw error;
        }
        const state = readTakeoverClaimState(stateRoot);
        if (state.kind === "missing")
            continue;
        if (state.kind === "foreign") {
            throw new OwnerLeaseError("OWNER_FOREIGN_CLAIM", state.reason);
        }
        if (state.kind === "valid" && !takeoverClaimIsStale(state.owner))
            return null;
        if (state.kind === "unverifiable")
            return null;
        if (state.kind === "invalid") {
            const before = takeoverClaimMetadata(stateRoot);
            sleepForOwnerRecovery(OWNER_RETRY_DELAY_MS);
            const settled = readTakeoverClaimState(stateRoot);
            const after = takeoverClaimMetadata(stateRoot);
            if (settled.kind === "valid" && !takeoverClaimIsStale(settled.owner))
                return null;
            if (settled.kind === "invalid" && sameTakeoverClaimMetadata(before, after)) {
                if (removeTakeoverClaimIfStale(stateRoot, settled))
                    continue;
            }
            continue;
        }
        if (removeTakeoverClaimIfStale(stateRoot, state))
            continue;
    }
    return null;
}
function releaseTakeoverClaim(stateRoot, claim) {
    if (!claim || !ownerToken(claim))
        return false;
    const current = readTakeoverClaimState(stateRoot);
    if (current.kind !== "valid" || !sameOwnerToken(current.owner, claim))
        return false;
    const file = takeoverClaimPath(stateRoot);
    let stats;
    try {
        stats = fs.lstatSync(file);
    }
    catch (error) {
        return Boolean(error && error.code === "ENOENT");
    }
    if (!stats.isDirectory())
        return false;
    const beforeMetadata = takeoverClaimMetadata(stateRoot);
    const markerOwner = acquireReclaimMarker(stateRoot);
    if (!markerOwner)
        return false;
    try {
        const settled = readTakeoverClaimState(stateRoot);
        const settledMetadata = takeoverClaimMetadata(stateRoot);
        if (settled.kind !== "valid" ||
            !sameOwnerToken(settled.owner, claim) ||
            !sameTakeoverClaimMetadata(beforeMetadata, settledMetadata)) {
            return false;
        }
        const quarantine = path.join(stateRoot, `.${TAKEOVER_CLAIM_NAME}.released.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`);
        try {
            fs.renameSync(file, quarantine);
            fs.rmSync(quarantine, { recursive: true, force: true });
            return true;
        }
        catch {
            try {
                fs.rmSync(quarantine, { recursive: true, force: true });
            }
            catch {
                /* A failed release leaves the canonical claim fail-closed. */
            }
            return false;
        }
    }
    finally {
        releaseReclaimMarker(stateRoot, markerOwner);
    }
}
function clearOwnerLock(stateRoot, expectedOwner) {
    if (!expectedOwner || !ownerToken(expectedOwner))
        return false;
    const file = lockPath(stateRoot);
    const current = readOwnerLockState(stateRoot);
    const beforeMetadata = ownerLockMetadata(file);
    if (current.kind !== "valid" || !sameOwnerToken(current.owner, expectedOwner) || !beforeMetadata)
        return false;
    let claim;
    try {
        claim = acquireTakeoverClaim(stateRoot);
    }
    catch (error) {
        if (error?.code === "OWNER_FOREIGN_CLAIM")
            return false;
        throw error;
    }
    if (!claim)
        return false;
    try {
        // Re-read after winning the unique takeover claim. Any contender that
        // replaced the old inode before the claim is never touched.
        const claimed = readOwnerLockState(stateRoot);
        const claimedMetadata = ownerLockMetadata(file);
        if (claimed.kind !== "valid" ||
            !sameOwnerToken(claimed.owner, expectedOwner) ||
            !sameOwnerLockMetadata(beforeMetadata, claimedMetadata)) {
            return false;
        }
        const candidate = path.join(stateRoot, `.${LOCK_NAME}.releasing.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`);
        try {
            // Pin the inode before the final pathname check. The unique claim keeps
            // another stale cleaner from unlinking a replacement in this interval.
            fs.linkSync(file, candidate);
            const pinned = readOwnerLockStateAt(candidate);
            const canonicalMetadata = ownerLockMetadata(file);
            if (pinned.kind !== "valid" ||
                !sameOwnerToken(pinned.owner, expectedOwner) ||
                !sameOwnerLockMetadata(claimedMetadata, canonicalMetadata)) {
                return false;
            }
            pauseBeforeTakeoverUnlink();
            const finalState = readOwnerLockState(stateRoot);
            const finalMetadata = ownerLockMetadata(file);
            if (finalState.kind !== "valid" ||
                !sameOwnerToken(finalState.owner, expectedOwner) ||
                !sameOwnerLockMetadata(claimedMetadata, finalMetadata)) {
                return false;
            }
            fs.rmSync(file);
            return true;
        }
        catch {
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
    finally {
        releaseTakeoverClaim(stateRoot, claim);
    }
}
function quarantineInvalidOwnerLock(stateRoot) {
    const file = lockPath(stateRoot);
    const before = readOwnerLockState(stateRoot);
    const beforeMetadata = ownerLockMetadata(file);
    if (before.kind !== "invalid" || !beforeMetadata)
        return false;
    // Give a writer one recovery interval to finish its record. New writers
    // never expose a partial canonical file because they publish via a hard link.
    sleepForOwnerRecovery(OWNER_RETRY_DELAY_MS);
    const settled = readOwnerLockState(stateRoot);
    const settledMetadata = ownerLockMetadata(file);
    if (settled.kind !== "invalid" || !sameOwnerLockMetadata(beforeMetadata, settledMetadata))
        return false;
    const quarantine = path.join(stateRoot, `.${LOCK_NAME}.invalid.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`);
    const claim = acquireTakeoverClaim(stateRoot);
    if (!claim)
        return false;
    let preserved = false;
    try {
        const claimed = readOwnerLockState(stateRoot);
        const claimedMetadata = ownerLockMetadata(file);
        if (claimed.kind !== "invalid" || !sameOwnerLockMetadata(settledMetadata, claimedMetadata))
            return false;
        // Pin the malformed inode, then re-check the canonical pathname before
        // removing it. A replacement inode is never touched by this recovery.
        fs.linkSync(file, quarantine);
        const pinned = readOwnerLockStateAt(quarantine);
        const canonicalMetadata = ownerLockMetadata(file);
        if (pinned.kind !== "invalid" || !sameOwnerLockMetadata(claimedMetadata, canonicalMetadata))
            return false;
        pauseBeforeTakeoverUnlink();
        const finalState = readOwnerLockState(stateRoot);
        const finalMetadata = ownerLockMetadata(file);
        if (finalState.kind !== "invalid" || !sameOwnerLockMetadata(claimedMetadata, finalMetadata))
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
        releaseTakeoverClaim(stateRoot, claim);
    }
}
function acquireOwnerLease(stateRoot, owner) {
    if (!owner || !ownerToken(owner)) {
        throw new OwnerLeaseError("OWNER_INVALID", "owner lease requires a token");
    }
    let sawUnreadable = false;
    for (let attempt = 0; attempt < OWNER_RETRY_COUNT; attempt += 1) {
        const claim = readTakeoverClaimState(stateRoot);
        if (claim.kind === "foreign") {
            throw new OwnerLeaseError("OWNER_FOREIGN_CLAIM", claim.reason);
        }
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
        if (current.kind === "unverifiable") {
            throw new OwnerLeaseError("OWNER_UNVERIFIABLE", "owner lease is unverifiable; refusing takeover", current.owner);
        }
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
    if (finalState.kind === "unverifiable") {
        throw new OwnerLeaseError("OWNER_UNVERIFIABLE", "owner lease is unverifiable; refusing takeover", finalState.owner);
    }
    if (finalState.kind === "invalid" || sawUnreadable) {
        throw new OwnerLeaseError("OWNER_UNREADABLE", "owner lease is not readable");
    }
    throw new OwnerLeaseError("OWNER_RACE", "owner lease changed during acquisition");
}
module.exports = {
    clearOwnerLock,
    acquireOwnerLease,
};
