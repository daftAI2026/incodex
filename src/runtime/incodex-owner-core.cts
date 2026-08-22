// @ts-nocheck
"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");

const LOCK_NAME = "incognito.lock";
const ACTIVE_LOCK_PREFIX = `${LOCK_NAME}.active.`;
const QUARANTINE_PREFIX = `.${LOCK_NAME}.quarantine.`;
const OWNER_TOKEN_PATTERN = /^[a-f0-9]{32}$/;
const MAX_SIDECAR_QUARANTINES = 128;
const SOCK_NAME = "incognito.sock";
const OWNER_RETRY_COUNT = 5;
const OWNER_RETRY_DELAY_MS = 100;
const OWNER_PORT_BASE = 45000;
const OWNER_PORT_SPAN = 15000;
let ownerRecordTestHook = null;

function targetIdFromExec(execPath) {
  return crypto.createHash("sha256").update(execPath || "unknown").digest("hex").slice(0, 12);
}

function targetStateDir(userRoot, execPath) {
  return path.join(userRoot, "targets", targetIdFromExec(execPath));
}

function ownerPortFromExec(execPath) {
  const uid = arguments.length > 1 ? arguments[1] : (typeof process.getuid === "function" ? process.getuid() : 0);
  const digest = crypto.createHash("sha256").update(`${uid}:${execPath || "unknown"}`).digest();
  return OWNER_PORT_BASE + digest.readUInt32BE(0) % OWNER_PORT_SPAN;
}

function processIdentity(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return null;
  const listed = spawnSync("ps", ["-p", String(pid), "-o", "pid=,lstart=,comm="], {
    encoding: "utf8",
    env: { ...process.env, LC_ALL: "C" },
  });
  if (listed.status !== 0 || !listed.stdout.trim()) return null;
  const line = listed.stdout.trim();
  const match = line.match(/^(\d+)\s+(.+?)\s+(\S+)$/);
  if (!match) return null;
  if (!isCanonicalProcessStartIdentity(match[2].trim())) return null;
  const comm = match[3];
  return {
    pid: Number(match[1]),
    startedAt: match[2].trim(),
    processStartIdentity: match[2].trim(),
    comm,
    execIdentity: comm,
  };
}

function isCanonicalProcessStartIdentity(value) {
  if (typeof value !== "string") return false;
  const parts = value.trim().split(/\s+/);
  return (
    parts.length === 5 &&
    ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"].includes(parts[0]) &&
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].includes(parts[1]) &&
    /^\d{1,2}$/.test(parts[2]) &&
    /^\d{2}:\d{2}:\d{2}$/.test(parts[3]) &&
    /^\d{4}$/.test(parts[4])
  );
}

function ownerToken(owner) {
  if (!owner || typeof owner !== "object") return "";
  if (typeof owner.token === "string" && owner.token) return owner.token;
  if (typeof owner.nonce === "string" && owner.nonce) return owner.nonce;
  return "";
}

function nonEmptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function hasReliableOwnerIdentity(owner) {
  return Boolean(
    owner &&
      (nonEmptyString(owner.processStartIdentity) || nonEmptyString(owner.startedAt)) &&
      (nonEmptyString(owner.execIdentity) || nonEmptyString(owner.execPath)),
  );
}

function executableIdentity(owner) {
  const raw = nonEmptyString(owner?.execIdentity)
    ? owner.execIdentity
    : nonEmptyString(owner?.comm)
      ? owner.comm
      : owner?.execPath;
  return nonEmptyString(raw) ? path.basename(String(raw).replace(/[/\\]+$/, "")) : "";
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

function lockPath(stateRoot) {
  return path.join(stateRoot, LOCK_NAME);
}

function activeOwnerPath(stateRoot, token) {
  return path.join(stateRoot, `${ACTIVE_LOCK_PREFIX}${token}`);
}

function activeOwnerTokenFromPath(file) {
  const name = path.basename(file);
  return name.startsWith(ACTIVE_LOCK_PREFIX) ? name.slice(ACTIVE_LOCK_PREFIX.length) : "";
}

function setOwnerRecordTestHook(hook) {
  ownerRecordTestHook = typeof hook === "function" ? hook : null;
}

function writeAtomicRecord(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
  const temporary = path.join(
    path.dirname(file),
    `.${path.basename(file)}.tmp.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`,
  );
  const fd = fs.openSync(temporary, fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL, 0o600);
  try {
    const contents = Buffer.from(`${JSON.stringify(value)}\n`);
    fs.writeSync(fd, contents, 0, contents.length, 0);
    try {
      fs.fsyncSync(fd);
    } catch {
      /* Some test filesystems do not expose fsync; the temp record is complete. */
    }
  } finally {
    fs.closeSync(fd);
  }
  try {
    fs.renameSync(temporary, file);
  } finally {
    try {
      fs.rmSync(temporary, { force: true });
    } catch {
      /* Best effort cleanup; the canonical record is already authoritative. */
    }
  }
}

function writeOwnerLock(stateRoot, owner) {
  return writeAtomicRecord(lockPath(stateRoot), owner);
}

function writeOwnerRecordExclusive(file, owner) {
  fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
  const temporary = path.join(
    path.dirname(file),
    `.${path.basename(file)}.tmp.${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`,
  );
  const fd = fs.openSync(temporary, fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL, 0o600);
  try {
    const contents = Buffer.from(`${JSON.stringify(owner)}\n`);
    fs.writeSync(fd, contents, 0, contents.length, 0);
    try { fs.fsyncSync(fd); } catch { /* The complete temp record is still valid. */ }
  } finally {
    fs.closeSync(fd);
  }
  try {
    // link(2) is atomic no-replace publication on the same filesystem.
    fs.linkSync(temporary, file);
  } finally {
    try { fs.rmSync(temporary, { force: true }); } catch { /* Best effort temp cleanup. */ }
  }
}

function writeOwnerLockExclusive(stateRoot, owner) {
  return writeOwnerRecordExclusive(lockPath(stateRoot), owner);
}

function readOwnerLockStateAt(file) {
  let stats;
  try {
    stats = fs.lstatSync(file);
  } catch (error) {
    if (error?.code === "ENOENT") return { kind: "missing", owner: null };
    return { kind: "invalid", owner: null, reason: String(error) };
  }
  if (stats.isSymbolicLink() || !stats.isFile()) return { kind: "invalid", owner: null, reason: "not a regular file" };
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

function isOwnerQuarantinePath(file) {
  return typeof file === "string" && path.basename(file).startsWith(QUARANTINE_PREFIX);
}

function retainedQuarantineState(state) {
  return state.kind === "unverifiable"
    ? state
    : { kind: "unverifiable", owner: state.owner, reason: "retained quarantine requires manual resolution" };
}

function reclaimStaleActiveOwnerRecord(file, expectedOwner) {
  if (!staleOwnerRecord(expectedOwner)) return { removed: false };
  const quarantine = path.join(
    path.dirname(file),
    `${QUARANTINE_PREFIX}${process.pid}.${Date.now()}.${crypto.randomBytes(8).toString("hex")}`,
  );
  try {
    fs.renameSync(file, quarantine);
  } catch {
    return { removed: false };
  }
  const latest = readOwnerLockStateAt(quarantine);
  if (latest.kind === "valid" && sameOwnerToken(latest.owner, expectedOwner) && staleOwnerRecord(latest.owner)) {
    try {
      ownerRecordTestHook?.({ originalPath: file, quarantinePath: quarantine, owner: latest.owner });
    } catch {
      /* Test hooks must not change the production cleanup decision. */
    }
    try {
      fs.rmSync(quarantine);
      return { removed: true };
    } catch {
      return { removed: false, path: quarantine, state: latest };
    }
  }
  return { removed: false, path: quarantine, state: latest };
}

function readOwnerRecords(stateRoot) {
  const records = [];
  const canonical = lockPath(stateRoot);
  let names = [];
  let quarantineAttempts = 0;
  try {
    names = fs.readdirSync(stateRoot);
    for (const name of names) {
      if (isOwnerQuarantinePath(name)) {
        const file = path.join(stateRoot, name);
        records.push({ path: file, state: retainedQuarantineState(readOwnerLockStateAt(file)) });
        continue;
      }
      if (!name.startsWith(ACTIVE_LOCK_PREFIX)) continue;
      const file = path.join(stateRoot, name);
      const state = readOwnerLockStateAt(file);
      const fileToken = activeOwnerTokenFromPath(file);
      if (state.kind === "valid" && (!OWNER_TOKEN_PATTERN.test(fileToken) || ownerToken(state.owner) !== fileToken)) {
        records.push({
          path: file,
          state: { kind: "unverifiable", owner: state.owner, reason: "sidecar filename token does not match owner token" },
        });
        continue;
      }
      if (state.kind === "valid" && quarantineAttempts >= MAX_SIDECAR_QUARANTINES) {
        records.push({
          path: file,
          state: { kind: "unverifiable", owner: state.owner, reason: "stale-sidecar cleanup cap reached; manual resolution required" },
        });
        continue;
      }
      if (state.kind === "valid" && quarantineAttempts < MAX_SIDECAR_QUARANTINES) {
        quarantineAttempts += 1;
        const result = reclaimStaleActiveOwnerRecord(file, state.owner);
        if (result.removed) continue;
        if (result.path) {
          records.push({ path: result.path, state: retainedQuarantineState(result.state) });
          continue;
        }
      }
      records.push({ path: file, state });
    }
  } catch {
    /* A missing state root still has a missing canonical diagnostic. */
  }
  // 活跃旁路记录优先；canonical 记录只保留为历史诊断元数据。
  records.push({ path: canonical, state: readOwnerLockStateAt(canonical) });
  return records;
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
  return Boolean(left && right && left.dev === right.dev && left.ino === right.ino);
}

function currentOwner(sessionId, execPath) {
  const live = processIdentity(process.pid);
  if (!live?.processStartIdentity || !live?.execIdentity) {
    throw new OwnerLeaseError("IDENTITY_UNAVAILABLE", "cannot acquire owner lease without process identity");
  }
  const token = crypto.randomBytes(16).toString("hex");
  return {
    pid: process.pid,
    startedAt: live.startedAt || "",
    processStartIdentity: live.processStartIdentity || "",
    execPath: execPath || process.execPath,
    execIdentity: live.execIdentity || "",
    sessionId: sessionId || "",
    token,
    nonce: token,
  };
}

function staleOwnerRecord(owner) {
  if (!owner || !Number.isInteger(owner.pid) || owner.pid <= 0) return true;
  if (!hasReliableOwnerIdentity(owner)) return false;
  if (!pidAlive(owner.pid)) return true;
  if (!isCanonicalProcessStartIdentity(owner.processStartIdentity || owner.startedAt)) return false;
  const live = processIdentity(owner.pid);
  if (!live) return false;
  return !ownerMatchesLive(owner, live);
}

function staleOwner(stateRoot) {
  const state = readOwnerLockState(stateRoot);
  return state.kind === "missing" || (state.kind === "valid" && staleOwnerRecord(state.owner));
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
  return sameOwnerToken(readOwnerLock(stateRoot), expectedOwner);
}

module.exports = {
  LOCK_NAME,
  SOCK_NAME,
  OWNER_RETRY_COUNT,
  OWNER_RETRY_DELAY_MS,
  OWNER_PORT_BASE,
  OWNER_PORT_SPAN,
  targetIdFromExec,
  targetStateDir,
  ownerPortFromExec,
  lockPath,
  activeOwnerPath,
  setOwnerRecordTestHook,
  processIdentity,
  isCanonicalProcessStartIdentity,
  ownerToken,
  hasReliableOwnerIdentity,
  ownerMatchesLive,
  sameOwnerToken,
  pidAlive,
  writeAtomicRecord,
  writeOwnerRecordExclusive,
  writeOwnerLock,
  writeOwnerLockExclusive,
  readOwnerLockStateAt,
  readOwnerLockState,
  readOwnerLock,
  readOwnerRecords,
  isOwnerQuarantinePath,
  ownerLockMetadata,
  sameOwnerLockMetadata,
  currentOwner,
  staleOwnerRecord,
  staleOwner,
  OwnerLeaseError,
  ownsOwnerLease,
};
