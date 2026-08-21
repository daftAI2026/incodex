// @ts-nocheck
"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const crypto = require("node:crypto");

const LOCK_NAME = "incognito.lock";
const SOCK_NAME = "incognito.sock";
const OWNER_RETRY_COUNT = 5;
const OWNER_RETRY_DELAY_MS = 100;
const TAKEOVER_CLAIM_NAME = ".incognito.lock.takeover";
const TAKEOVER_CLAIM_OWNER_NAME = "owner";
const TAKEOVER_CLAIM_RECLAIM_NAME = ".reclaim";

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
  if (!Number.isInteger(pid) || pid <= 0) return null;
  const listed = spawnSync("ps", ["-p", String(pid), "-o", "pid=,lstart=,comm="], { encoding: "utf8" });
  if (listed.status !== 0 || !listed.stdout.trim()) return null;
  const line = listed.stdout.trim();
  const match = line.match(/^(\d+)\s+(.+?)\s+(\S+)$/);
  if (!match) return null;
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
  if (!owner || typeof owner !== "object") return "";
  if (typeof owner.token === "string" && owner.token) return owner.token;
  if (typeof owner.nonce === "string" && owner.nonce) return owner.nonce;
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
  if (nonEmptyString(owner?.execIdentity)) return owner.execIdentity;
  if (nonEmptyString(owner?.comm)) return owner.comm;
  return nonEmptyString(owner?.execPath) ? path.basename(owner.execPath) : "";
}

function ownerMatchesLive(owner, live) {
  if (!hasReliableOwnerIdentity(owner) || !hasReliableOwnerIdentity(live)) return false;
  const ownerStart = owner.processStartIdentity || owner.startedAt;
  const liveStart = live.processStartIdentity || live.startedAt;
  if (owner.pid !== live.pid || ownerStart !== liveStart) return false;
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
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code === "EPERM";
  }
}

function sleepForOwnerRecovery(ms) {
  if (ms <= 0) return;
  const waiter = new Int32Array(new SharedArrayBuffer(4));
  Atomics.wait(waiter, 0, 0, ms);
}

function pauseBeforeTakeoverUnlink() {
  const pauseFile = process.env.INCODEX_TEST_TAKEOVER_PAUSE_FILE;
  const releaseFile = process.env.INCODEX_TEST_TAKEOVER_RELEASE_FILE;
  if (!pauseFile || !releaseFile) return;
  try {
    fs.writeFileSync(pauseFile, `${process.pid}\n`, { flag: "wx", mode: 0o600 });
  } catch {
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
  if (!pauseFile || !releaseFile) return;
  try {
    fs.writeFileSync(pauseFile, `${process.pid}\n`, { flag: "wx", mode: 0o600 });
  } catch {
    return;
  }
  const deadline = Date.now() + 5000;
  const waiter = new Int32Array(new SharedArrayBuffer(4));
  while (!fs.existsSync(releaseFile) && Date.now() < deadline) {
    Atomics.wait(waiter, 0, 0, 5);
  }
}

function writeAtomicRecord(file, value) {
  const temp = path.join(
    path.dirname(file),
    `.${path.basename(file)}.tmp.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`,
  );
  let fd = null;
  try {
    fd = fs.openSync(
      temp,
      fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL | (fs.constants.O_NOFOLLOW || 0),
      0o600,
    );
    const contents = Buffer.from(`${JSON.stringify(value)}\n`);
    let offset = 0;
    while (offset < contents.length) {
      offset += fs.writeSync(fd, contents, offset, contents.length - offset, offset);
    }
    try {
      fs.fsyncSync(fd);
    } catch {
      /* Some test filesystems do not support fsync; the complete temp file remains private. */
    }
  } finally {
    if (fd !== null) fs.closeSync(fd);
  }
  try {
    // A hard-link publish makes the canonical path either absent or complete.
    // A crash before this point leaves only an ignored temp file, never a
    // truncated record that can poison the next launch.
    fs.linkSync(temp, file);
  } finally {
    try {
      fs.rmSync(temp, { force: true });
    } catch {
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
  } catch (error) {
    if (error && error.code === "ENOENT") return { kind: "missing", owner: null };
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
  } catch (error) {
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
  } catch (error) {
    if (error && error.code === "ENOENT") return true;
    return false;
  }
  if (stats.isSymbolicLink()) return false;
  try {
    fs.rmSync(file);
    return true;
  } catch (error) {
    return Boolean(error && error.code === "ENOENT");
  }
}

function ownerLockMetadata(file) {
  try {
    const stats = fs.lstatSync(file);
    return { dev: stats.dev, ino: stats.ino, size: stats.size, mtimeMs: stats.mtimeMs };
  } catch {
    return null;
  }
}

function sameOwnerLockMetadata(left, right) {
  return Boolean(
    left &&
      right &&
      left.dev === right.dev &&
      left.ino === right.ino &&
      left.size === right.size &&
      left.mtimeMs === right.mtimeMs,
  );
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

function reclaimMarkerOwnerPath(stateRoot) {
  return path.join(takeoverClaimReclaimPath(stateRoot), TAKEOVER_CLAIM_OWNER_NAME);
}

function readTakeoverClaimState(stateRoot) {
  const file = takeoverClaimPath(stateRoot);
  let stats;
  try {
    stats = fs.lstatSync(file);
  } catch (error) {
    if (error && error.code === "ENOENT") return { kind: "missing", owner: null };
    return { kind: "invalid", owner: null, reason: String(error) };
  }
  if (stats.isDirectory()) {
    const owner = readOwnerLockStateAt(takeoverClaimOwnerPath(stateRoot));
    return owner.kind === "missing"
      ? { kind: "invalid", owner: null, reason: "takeover claim has no owner record" }
      : owner;
  }
  // A regular claim can only be left by an older runtime. It is read for a
  // bounded migration window, but new claims are always published as dirs.
  return readOwnerLockStateAt(file);
}

function takeoverClaimMetadata(stateRoot) {
  const file = takeoverClaimPath(stateRoot);
  try {
    const stats = fs.lstatSync(file);
    return { dev: stats.dev, ino: stats.ino };
  } catch {
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
  if (!owner || !Number.isInteger(owner.pid) || owner.pid <= 0) return false;
  if (!hasReliableOwnerIdentity(owner)) return false;
  const live = processIdentity(owner.pid);
  if (!live) return !pidAlive(owner.pid);
  return !ownerMatchesLive(owner, live);
}

function readReclaimMarkerState(stateRoot) {
  const marker = takeoverClaimReclaimPath(stateRoot);
  let stats;
  try {
    stats = fs.lstatSync(marker);
  } catch (error) {
    if (error?.code === "ENOENT") return { kind: "missing", owner: null };
    return { kind: "invalid", owner: null, reason: String(error) };
  }
  if (!stats.isDirectory()) return { kind: "invalid", owner: null, reason: "reclaim marker is not a directory" };
  const owner = readOwnerLockStateAt(reclaimMarkerOwnerPath(stateRoot));
  return owner.kind === "missing" ? { kind: "invalid", owner: null, reason: "reclaim marker has no owner" } : owner;
}

function createReclaimMarkerTemp(stateRoot, owner) {
  const temporary = path.join(
    stateRoot,
    `.${TAKEOVER_CLAIM_NAME}.reclaim.tmp.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`,
  );
  fs.mkdirSync(temporary, { mode: 0o700 });
  try {
    writeAtomicRecord(path.join(temporary, TAKEOVER_CLAIM_OWNER_NAME), owner);
    return temporary;
  } catch (error) {
    fs.rmSync(temporary, { recursive: true, force: true });
    throw error;
  }
}

function acquireReclaimMarker(stateRoot) {
  const owner = takeoverClaimOwner();
  if (!hasReliableOwnerIdentity(owner)) return null;
  const marker = takeoverClaimReclaimPath(stateRoot);
  for (let attempt = 0; attempt < OWNER_RETRY_COUNT; attempt += 1) {
    const current = readReclaimMarkerState(stateRoot);
    if (current.kind === "valid" && !takeoverClaimIsStale(current.owner)) return null;
    if (current.kind !== "missing" && current.kind !== "valid") return null;

    const temporary = createReclaimMarkerTemp(stateRoot, owner);
    if (current.kind === "missing") {
      try {
        fs.renameSync(temporary, marker);
        return owner;
      } catch (error) {
        fs.rmSync(temporary, { recursive: true, force: true });
        if (["EEXIST", "ENOTEMPTY", "ENOTDIR"].includes(error?.code)) continue;
        return null;
      }
    }

    const staleMarker = path.join(
      stateRoot,
      `.${TAKEOVER_CLAIM_NAME}.reclaim.stale.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`,
    );
    pauseBeforeReclaimHandoff();
    try {
      fs.renameSync(marker, staleMarker);
    } catch (error) {
      fs.rmSync(temporary, { recursive: true, force: true });
      if (error?.code === "ENOENT") continue;
      return null;
    }
    try {
      fs.renameSync(temporary, marker);
      fs.rmSync(staleMarker, { recursive: true, force: true });
      return owner;
    } catch (error) {
      fs.rmSync(temporary, { recursive: true, force: true });
      fs.rmSync(staleMarker, { recursive: true, force: true });
      if (["EEXIST", "ENOTEMPTY", "ENOTDIR"].includes(error?.code)) continue;
      return null;
    }
  }
  return null;
}

function releaseReclaimMarker(stateRoot, expectedOwner) {
  if (!expectedOwner || !ownerToken(expectedOwner)) return false;
  const current = readReclaimMarkerState(stateRoot);
  if (current.kind !== "valid" || !sameOwnerToken(current.owner, expectedOwner)) return false;
  const released = path.join(
    stateRoot,
    `.${TAKEOVER_CLAIM_NAME}.reclaim.released.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`,
  );
  try {
    fs.renameSync(takeoverClaimReclaimPath(stateRoot), released);
    fs.rmSync(released, { recursive: true, force: true });
    return true;
  } catch (error) {
    fs.rmSync(released, { recursive: true, force: true });
    return Boolean(error?.code === "ENOENT");
  }
}

function publishTakeoverClaim(stateRoot, owner) {
  const file = takeoverClaimPath(stateRoot);
  const temporary = path.join(
    stateRoot,
    `.${TAKEOVER_CLAIM_NAME}.tmp.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`,
  );
  fs.mkdirSync(temporary, { recursive: false, mode: 0o700 });
  try {
    writeAtomicRecord(path.join(temporary, TAKEOVER_CLAIM_OWNER_NAME), owner);
    try {
      // The destination is either absent or a non-empty claim directory. A
      // directory rename therefore gives us an atomic no-replace publish.
      fs.renameSync(temporary, file);
      return owner;
    } catch (error) {
      if (["EEXIST", "ENOTEMPTY", "ENOTDIR"].includes(error?.code)) {
        const conflict = new Error("takeover claim already exists");
        conflict.code = "EEXIST";
        throw conflict;
      }
      throw error;
    }
  } finally {
    try {
      fs.rmSync(temporary, { recursive: true, force: true });
    } catch {
      /* The temporary claim is private and best-effort after publication. */
    }
  }
}

function removeLegacyTakeoverClaimIfStale(stateRoot, expectedState) {
  const before = takeoverClaimMetadata(stateRoot);
  if (!before) return false;
  const current = readTakeoverClaimState(stateRoot);
  if (current.kind !== expectedState.kind || (current.kind === "valid" && !takeoverClaimIsStale(current.owner))) return false;
  pauseBeforeTakeoverUnlink();
  const final = readTakeoverClaimState(stateRoot);
  if (final.kind !== expectedState.kind || (final.kind === "valid" && !takeoverClaimIsStale(final.owner))) return false;
  if (!sameTakeoverClaimMetadata(before, takeoverClaimMetadata(stateRoot))) return false;
  try {
    fs.rmSync(takeoverClaimPath(stateRoot));
    return true;
  } catch (error) {
    return Boolean(error && error.code === "ENOENT");
  }
}

function removeTakeoverClaimIfStale(stateRoot, expectedState) {
  const file = takeoverClaimPath(stateRoot);
  const current = readTakeoverClaimState(stateRoot);
  if (current.kind !== expectedState.kind) return false;
  if (current.kind === "valid" && !takeoverClaimIsStale(current.owner)) return false;
  if (!fs.existsSync(file)) return false;

  let stats;
  try {
    stats = fs.lstatSync(file);
  } catch {
    return false;
  }
  if (!stats.isDirectory()) return removeLegacyTakeoverClaimIfStale(stateRoot, expectedState);

  const beforeMetadata = takeoverClaimMetadata(stateRoot);
  if (!beforeMetadata) return false;
  const markerOwner = acquireReclaimMarker(stateRoot);
  if (!markerOwner) return false;

  try {
    const claimed = readTakeoverClaimState(stateRoot);
    const claimedMetadata = takeoverClaimMetadata(stateRoot);
    if (
      claimed.kind !== expectedState.kind ||
      (claimed.kind === "valid" && !takeoverClaimIsStale(claimed.owner)) ||
      !sameTakeoverClaimMetadata(beforeMetadata, claimedMetadata)
    ) {
      return false;
    }

    pauseBeforeTakeoverUnlink();
    const finalState = readTakeoverClaimState(stateRoot);
    const finalMetadata = takeoverClaimMetadata(stateRoot);
    if (
      finalState.kind !== expectedState.kind ||
      (finalState.kind === "valid" && !takeoverClaimIsStale(finalState.owner)) ||
      !sameTakeoverClaimMetadata(beforeMetadata, finalMetadata)
    ) {
      return false;
    }

    const quarantine = path.join(
      stateRoot,
      `.${TAKEOVER_CLAIM_NAME}.stale.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`,
    );
    try {
      // Only the process that created the fixed marker may move this
      // non-empty directory. A new claim is published at a fresh inode.
      fs.renameSync(file, quarantine);
      fs.rmSync(quarantine, { recursive: true, force: true });
      return true;
    } catch {
      try {
        fs.rmSync(quarantine, { recursive: true, force: true });
      } catch {
        /* Never touch the canonical claim after a failed handoff. */
      }
      return false;
    }
  } finally {
    if (fs.existsSync(file)) {
      const currentMetadata = takeoverClaimMetadata(stateRoot);
      if (sameTakeoverClaimMetadata(beforeMetadata, currentMetadata)) releaseReclaimMarker(stateRoot, markerOwner);
    }
  }
}

function acquireTakeoverClaim(stateRoot) {
  fs.mkdirSync(stateRoot, { recursive: true, mode: 0o700 });
  for (let attempt = 0; attempt < OWNER_RETRY_COUNT; attempt += 1) {
    try {
      const owner = takeoverClaimOwner();
      if (!hasReliableOwnerIdentity(owner)) return null;
      return publishTakeoverClaim(stateRoot, owner);
    } catch (error) {
      if (error?.code !== "EEXIST") throw error;
    }

    const state = readTakeoverClaimState(stateRoot);
    if (state.kind === "missing") continue;
    if (state.kind === "valid" && !takeoverClaimIsStale(state.owner)) return null;
    if (state.kind === "unverifiable") return null;
    if (state.kind === "invalid") {
      const before = takeoverClaimMetadata(stateRoot);
      sleepForOwnerRecovery(OWNER_RETRY_DELAY_MS);
      const settled = readTakeoverClaimState(stateRoot);
      const after = takeoverClaimMetadata(stateRoot);
      if (settled.kind === "valid" && !takeoverClaimIsStale(settled.owner)) return null;
      if (settled.kind === "invalid" && sameTakeoverClaimMetadata(before, after)) {
        if (removeTakeoverClaimIfStale(stateRoot, settled)) continue;
      }
      continue;
    }
    if (removeTakeoverClaimIfStale(stateRoot, state)) continue;
  }
  return null;
}

function releaseTakeoverClaim(stateRoot, claim) {
  if (!claim || !ownerToken(claim)) return false;
  const current = readTakeoverClaimState(stateRoot);
  if (current.kind !== "valid" || !sameOwnerToken(current.owner, claim)) return false;
  const file = takeoverClaimPath(stateRoot);
  let stats;
  try {
    stats = fs.lstatSync(file);
  } catch (error) {
    return Boolean(error && error.code === "ENOENT");
  }
  if (!stats.isDirectory()) {
    try {
      fs.rmSync(file);
      return true;
    } catch (error) {
      return Boolean(error && error.code === "ENOENT");
    }
  }

  const beforeMetadata = takeoverClaimMetadata(stateRoot);
  const markerOwner = acquireReclaimMarker(stateRoot);
  if (!markerOwner) return false;
  try {
    const settled = readTakeoverClaimState(stateRoot);
    const settledMetadata = takeoverClaimMetadata(stateRoot);
    if (
      settled.kind !== "valid" ||
      !sameOwnerToken(settled.owner, claim) ||
      !sameTakeoverClaimMetadata(beforeMetadata, settledMetadata)
    ) {
      return false;
    }
    const quarantine = path.join(
      stateRoot,
      `.${TAKEOVER_CLAIM_NAME}.released.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`,
    );
    try {
      fs.renameSync(file, quarantine);
      fs.rmSync(quarantine, { recursive: true, force: true });
      return true;
    } catch {
      try {
        fs.rmSync(quarantine, { recursive: true, force: true });
      } catch {
        /* A failed release leaves the canonical claim fail-closed. */
      }
      return false;
    }
  } finally {
    if (fs.existsSync(file)) {
      const currentMetadata = takeoverClaimMetadata(stateRoot);
      if (sameTakeoverClaimMetadata(beforeMetadata, currentMetadata)) releaseReclaimMarker(stateRoot, markerOwner);
    }
  }
}

function clearOwnerLock(stateRoot, expectedOwner) {
  if (!expectedOwner || !ownerToken(expectedOwner)) return false;
  const file = lockPath(stateRoot);
  const current = readOwnerLockState(stateRoot);
  const beforeMetadata = ownerLockMetadata(file);
  if (current.kind !== "valid" || !sameOwnerToken(current.owner, expectedOwner) || !beforeMetadata) return false;

  const claim = acquireTakeoverClaim(stateRoot);
  if (!claim) return false;
  try {
    // Re-read after winning the unique takeover claim. Any contender that
    // replaced the old inode before the claim is never touched.
    const claimed = readOwnerLockState(stateRoot);
    const claimedMetadata = ownerLockMetadata(file);
    if (
      claimed.kind !== "valid" ||
      !sameOwnerToken(claimed.owner, expectedOwner) ||
      !sameOwnerLockMetadata(beforeMetadata, claimedMetadata)
    ) {
      return false;
    }

    const candidate = path.join(
      stateRoot,
      `.${LOCK_NAME}.releasing.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`,
    );
    try {
      // Pin the inode before the final pathname check. The unique claim keeps
      // another stale cleaner from unlinking a replacement in this interval.
      fs.linkSync(file, candidate);
      const pinned = readOwnerLockStateAt(candidate);
      const canonicalMetadata = ownerLockMetadata(file);
      if (
        pinned.kind !== "valid" ||
        !sameOwnerToken(pinned.owner, expectedOwner) ||
        !sameOwnerLockMetadata(claimedMetadata, canonicalMetadata)
      ) {
        return false;
      }
      pauseBeforeTakeoverUnlink();
      const finalState = readOwnerLockState(stateRoot);
      const finalMetadata = ownerLockMetadata(file);
      if (
        finalState.kind !== "valid" ||
        !sameOwnerToken(finalState.owner, expectedOwner) ||
        !sameOwnerLockMetadata(claimedMetadata, finalMetadata)
      ) {
        return false;
      }
      fs.rmSync(file);
      return true;
    } catch {
      return false;
    } finally {
      try {
        fs.rmSync(candidate, { force: true });
      } catch {
        /* The candidate is only a temporary inode pin. */
      }
    }
  } finally {
    releaseTakeoverClaim(stateRoot, claim);
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
  if (!owner || !Number.isInteger(owner.pid) || owner.pid <= 0) return true;
  if (!hasReliableOwnerIdentity(owner)) return false;
  const live = processIdentity(owner.pid);
  if (!live) return !pidAlive(owner.pid);
  return !ownerMatchesLive(owner, live);
}

function staleOwner(stateRoot) {
  const state = readOwnerLockState(stateRoot);
  if (state.kind !== "valid") return state.kind === "missing";
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

function quarantineInvalidOwnerLock(stateRoot) {
  const file = lockPath(stateRoot);
  const before = readOwnerLockState(stateRoot);
  const beforeMetadata = ownerLockMetadata(file);
  if (before.kind !== "invalid" || !beforeMetadata) return false;

  // Give a legacy writer one recovery interval to finish its record. New
  // writers never expose a partial canonical file because they publish via a
  sleepForOwnerRecovery(OWNER_RETRY_DELAY_MS);
  const settled = readOwnerLockState(stateRoot);
  const settledMetadata = ownerLockMetadata(file);
  if (settled.kind !== "invalid" || !sameOwnerLockMetadata(beforeMetadata, settledMetadata)) return false;

  const quarantine = path.join(
    stateRoot,
    `.${LOCK_NAME}.invalid.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`,
  );
  const claim = acquireTakeoverClaim(stateRoot);
  if (!claim) return false;
  let preserved = false;
  try {
    const claimed = readOwnerLockState(stateRoot);
    const claimedMetadata = ownerLockMetadata(file);
    if (claimed.kind !== "invalid" || !sameOwnerLockMetadata(settledMetadata, claimedMetadata)) return false;
    // Pin the malformed inode, then re-check the canonical pathname before
    // removing it. A replacement inode is never touched by this recovery.
    fs.linkSync(file, quarantine);
    const pinned = readOwnerLockStateAt(quarantine);
    const canonicalMetadata = ownerLockMetadata(file);
    if (pinned.kind !== "invalid" || !sameOwnerLockMetadata(claimedMetadata, canonicalMetadata)) return false;
    pauseBeforeTakeoverUnlink();
    const finalState = readOwnerLockState(stateRoot);
    const finalMetadata = ownerLockMetadata(file);
    if (finalState.kind !== "invalid" || !sameOwnerLockMetadata(claimedMetadata, finalMetadata)) return false;
    try {
      fs.rmSync(file);
      preserved = true;
      return true;
    } catch (error) {
      if (error && error.code === "ENOENT") {
        preserved = true;
        return true;
      }
      return false;
    }
  } catch (error) {
    return false;
  } finally {
    if (!preserved) {
      try {
        fs.rmSync(quarantine, { force: true });
      } catch {
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
    try {
      writeOwnerLock(stateRoot, owner);
      const current = readOwnerLock(stateRoot);
      if (!sameOwnerToken(current, owner)) {
        throw new OwnerLeaseError("OWNER_VERIFY_FAILED", "owner lease verification failed", current);
      }
      return owner;
    } catch (error) {
      if (error?.code !== "EEXIST") throw error;
    }

    const current = readOwnerLockState(stateRoot);
    if (current.kind === "missing") continue;
    if (current.kind === "unverifiable") {
      throw new OwnerLeaseError(
        "OWNER_UNVERIFIABLE",
        "owner lease is unverifiable; refusing takeover",
        current.owner,
      );
    }
    if (current.kind === "invalid") {
      sawUnreadable = true;
      if (quarantineInvalidOwnerLock(stateRoot)) continue;
      continue;
    }

    if (!staleOwnerRecord(current.owner)) {
      throw new OwnerLeaseError("OWNER_BUSY", "another Incognito owner is active", current.owner);
    }
    if (!clearOwnerLock(stateRoot, current.owner)) continue;
  }

  const finalState = readOwnerLockState(stateRoot);
  if (finalState.kind === "valid" && !staleOwnerRecord(finalState.owner)) {
    throw new OwnerLeaseError("OWNER_BUSY", "another Incognito owner won the lease race", finalState.owner);
  }
  if (finalState.kind === "unverifiable") {
    throw new OwnerLeaseError(
      "OWNER_UNVERIFIABLE",
      "owner lease is unverifiable; refusing takeover",
      finalState.owner,
    );
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
      if (done) return;
      done = true;
      socket.destroy();
      resolve(ok);
    };
    socket.setTimeout(timeoutMs);
    socket.once("connect", () => {
      try {
        socket.write(`${expectedToken ? `raise ${expectedToken}` : "raise"}\n`);
      } catch {
        finish(false);
      }
    });
    socket.on("data", (buf) => {
      response += String(buf);
      if (response.split("\n").some((line) => line.trim() === "ok")) finish(true);
      else if (response.split("\n").some((line) => line.trim() === "denied")) finish(false);
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
    if (await connectExisting(stateRoot, timeoutMs, token)) return true;
    if (attempt + 1 < attempts && delayMs > 0) await new Promise((resolve) => setTimeout(resolve, delayMs));
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
      if (!request.includes("\n")) return;
      const line = request.slice(0, request.indexOf("\n")).trim();
      const expectedRequest = expectedToken ? `raise ${expectedToken}` : "raise";
      if (line !== expectedRequest) {
        try {
          socket.end("denied\n");
        } catch {
          /* EPIPE: peer already closed. */
        }
        return;
      }
      onRaise();
      try {
        if (!socket.destroyed) socket.end("ok\n");
      } catch {
        /* EPIPE: peer already closed. */
      }
    });
  });
  server.listen(sockPath(stateRoot));
  return server;
}

function singleFlight(holder, start) {
  if (holder.current) return holder.current;
  holder.current = Promise.resolve()
    .then(start)
    .finally(() => {
      holder.current = null;
    });
  return holder.current;
}

export {
  LOCK_NAME,
  SOCK_NAME,
  targetIdFromExec,
  targetStateDir,
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
