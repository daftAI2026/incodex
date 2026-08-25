// @ts-nocheck
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const ownerCore = require("./incodex-owner-core.cts");

const SESSIONS_NAME = "sessions";
const BURN_PROOF_PREFIX = ".incodex-burned-";
const BURN_PROOF_SUFFIX = ".json";
const LOGS_NAME = "logs";
const OWNER_NAME = "owner.json";
const LOCK_NAME = "lock";
const READY_NAME = "ready";
const PID_NAME = "incognito.pid";
const SETTINGS_FILES = ["auth.json", "config.toml"];
const GLOBAL_STATE_NAME = ".codex-global-state.json";
const MAIN_WINDOW_BOUNDS_KEY = "electron-main-window-bounds";
const DESKTOP_FIRST_SEEN_AT_MS_KEY = "desktop-first-seen-at-ms";
const PERSISTED_ATOM_STATE_KEY = "electron-persisted-atom-state";
const MIGRATION_ANNOUNCEMENT_KEY = "chatgpt-migration-announcement-completed-v1";
const UPDATE_ANNOUNCEMENT_KEY = "chatgpt-update-downloaded-announcement-seen-v1";
const DIR_MODE = 0o700;
const FILE_MODE = 0o600;
const MAIN_WINDOW_MIN_WIDTH = 480;
const MAIN_WINDOW_MIN_HEIGHT = 600;
const LOG_LIMIT = 1024 * 1024;
const LOG_KEEP = 3;

const OPEN_EXCLUSIVE =
  fs.constants.O_WRONLY |
  fs.constants.O_CREAT |
  fs.constants.O_EXCL |
  (fs.constants.O_NOFOLLOW || 0);

const OPEN_PRIVATE =
  fs.constants.O_WRONLY |
  fs.constants.O_CREAT |
  fs.constants.O_TRUNC |
  (fs.constants.O_NOFOLLOW || 0);

const OPEN_APPEND =
  fs.constants.O_WRONLY |
  fs.constants.O_CREAT |
  fs.constants.O_APPEND |
  (fs.constants.O_NOFOLLOW || 0);

function lstatOrNull(target) {
  try {
    return fs.lstatSync(target);
  } catch (error) {
    if (error && error.code === "ENOENT") return null;
    throw error;
  }
}

function assertNotSymlink(target, label) {
  const stats = lstatOrNull(target);
  if (stats?.isSymbolicLink()) {
    throw new Error(`[incodex] refuse to use symlink ${label}: ${target}`);
  }
  return stats;
}

function assertInsideParent(realPath, parentReal) {
  const prefix = parentReal.endsWith(path.sep) ? parentReal : `${parentReal}${path.sep}`;
  if (realPath !== parentReal && !realPath.startsWith(prefix)) {
    throw new Error(`[incodex] path escaped private parent: ${realPath}`);
  }
}

function realExisting(target) {
  return fs.realpathSync.native(target);
}

function ensurePrivateDir(dir, parent) {
  const existing = assertNotSymlink(dir, "directory");
  if (!existing) {
    fs.mkdirSync(dir, { recursive: true, mode: DIR_MODE });
  } else if (!existing.isDirectory()) {
    throw new Error(`[incodex] expected directory: ${dir}`);
  }
  fs.chmodSync(dir, DIR_MODE);
  const again = assertNotSymlink(dir, "directory");
  if (!again?.isDirectory()) throw new Error(`[incodex] directory vanished: ${dir}`);
  const realDir = realExisting(dir);
  const realParent = realExisting(parent);
  assertInsideParent(realDir, realParent);
  return { real: realDir, ino: again.ino, dev: again.dev };
}

function writePrivateFile(dest, data, { exclusive = false } = {}) {
  const prior = assertNotSymlink(dest, "file");
  if (exclusive && prior) {
    throw new Error(`[incodex] refuse to overwrite existing file: ${dest}`);
  }
  const flags = exclusive || !prior ? OPEN_EXCLUSIVE : OPEN_PRIVATE;
  const fd = fs.openSync(dest, flags, FILE_MODE);
  try {
    fs.writeSync(fd, data);
    fs.fchmodSync(fd, FILE_MODE);
  } finally {
    fs.closeSync(fd);
  }
}

function writePrivateFileAtomic(dest, data) {
  const parent = path.dirname(dest);
  const temporary = path.join(
    parent,
    `.${path.basename(dest)}.tmp.${process.pid}.${Date.now()}.${Math.random().toString(16).slice(2)}`,
  );
  const fd = fs.openSync(temporary, OPEN_EXCLUSIVE, FILE_MODE);
  try {
    fs.writeSync(fd, data);
    try {
      fs.fsyncSync(fd);
    } catch {
      /* 某些测试文件系统不提供 fsync；临时文件仍保持完整。 */
    }
  } finally {
    fs.closeSync(fd);
  }
  try {
    assertNotSymlink(dest, "file");
    fs.renameSync(temporary, dest);
  } finally {
    try {
      fs.rmSync(temporary, { force: true });
    } catch {
      /* 原子 rename 成功后，临时文件清理只是 best effort。 */
    }
  }
}

function exclusiveCopyFile(src, dest) {
  const destStat = assertNotSymlink(dest, "copy destination");
  if (destStat) {
    throw new Error(`[incodex] refuse to overwrite existing file: ${dest}`);
  }
  const data = fs.readFileSync(src);
  writePrivateFile(dest, data, { exclusive: true });
}

function sessionsBase(userRoot, targetId) {
  const sessions = path.join(userRoot, SESSIONS_NAME);
  const sessionParent = ensurePrivateDir(sessions, userRoot);
  if (!targetId) return sessionParent;
  return ensurePrivateDir(path.join(sessions, targetId), sessionParent.real);
}

function createSessionHome(userRoot, options = {}) {
  const parent = path.dirname(userRoot);
  ensurePrivateDir(userRoot, parent);
  ensurePrivateDir(path.join(userRoot, LOGS_NAME), userRoot);
  const sessionParent = sessionsBase(userRoot, options.targetId);
  const root = fs.mkdtempSync(path.join(sessionParent.real, "s-"));
  fs.chmodSync(root, DIR_MODE);
  const rootStat = assertNotSymlink(root, "session root");
  if (!rootStat?.isDirectory()) throw new Error(`[incodex] session root is not a directory: ${root}`);
  const realRoot = realExisting(root);
  assertInsideParent(realRoot, sessionParent.real);
  const home = ensurePrivateDir(path.join(realRoot, "codex-home"), realRoot);
  const chromium = ensurePrivateDir(path.join(realRoot, "chromium"), realRoot);
  const sessionId = path.basename(realRoot);
  writePrivateFile(path.join(realRoot, LOCK_NAME), `${options.pid || ""}\n`, { exclusive: true });
  const processStartIdentity = ownerCore.processIdentity(options.pid || 0)?.processStartIdentity || "";
  const owner = {
    sessionId,
    targetId: options.targetId || "",
    pid: options.pid || 0,
    sourceHome: options.sourceHome || "",
    createdAt: new Date().toISOString(),
    ino: rootStat.ino,
    dev: rootStat.dev,
    processStartIdentity,
  };
  if (options.handoffPending === true) owner.handoffPending = true;
  writePrivateFile(path.join(realRoot, OWNER_NAME), `${JSON.stringify(owner)}\n`, { exclusive: true });
  return {
    sessionId,
    root: realRoot,
    home: home.real,
    chromium: chromium.real,
    ino: rootStat.ino,
    dev: rootStat.dev,
  };
}

function handoffSessionOwner(sessionRoot, pid) {
  const live = ownerCore.processIdentity(pid);
  if (!live?.processStartIdentity) {
    throw new Error(`[incodex] cannot hand off session owner: process identity unavailable for pid ${pid}`);
  }
  const rootStats = assertNotSymlink(sessionRoot, "session root");
  if (!rootStats?.isDirectory()) throw new Error(`[incodex] session root is not a directory: ${sessionRoot}`);
  const ownerPath = path.join(sessionRoot, OWNER_NAME);
  const owner = readOwner(sessionRoot);
  if (owner.ino !== rootStats.ino || owner.dev !== rootStats.dev) {
    throw new Error("[incodex] session root identity changed; refusing owner handoff");
  }
  owner.pid = pid;
  owner.processStartIdentity = live.processStartIdentity;
  if (owner.handoffPending === true) owner.handoffPending = false;
  writePrivateFileAtomic(ownerPath, `${JSON.stringify(owner)}\n`);
  return { pid, processStartIdentity: live.processStartIdentity };
}

function readOwner(home) {
  const file = path.join(home, OWNER_NAME);
  const stats = assertNotSymlink(file, "owner manifest");
  if (!stats) throw new Error(`[incodex] missing session owner: ${file}`);
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function assertBurnIdentity(home, expected) {
  if (!expected.sessionId) return;
  const file = path.join(home, OWNER_NAME);
  const stats = assertNotSymlink(file, "owner manifest");
  if (!stats) {
    // Child burn already removed owner.json; Chromium may recreate a few
    // files under the same path. The folder name is the session id.
    if (path.basename(home) === expected.sessionId) return;
    throw new Error(`[incodex] missing session owner: ${file}`);
  }
  const owner = JSON.parse(fs.readFileSync(file, "utf8"));
  if (owner.sessionId !== expected.sessionId) {
    throw new Error("[incodex] session id mismatch; refusing to burn");
  }
}

function sessionRootFromHome(home) {
  const base = path.basename(home);
  if (base === "codex-home" || base === "chromium") return path.dirname(home);
  return home;
}

function burnSessionHome(target, expected) {
  return burnSessionHomeInner(target, expected, null);
}

function burnSessionHomeWithOwner(target, expected, ownerSnapshot) {
  return burnSessionHomeInner(target, expected, ownerSnapshot);
}

function burnSessionHomeInner(target, expected, ownerSnapshot) {
  const home = sessionRootFromHome(target);
  const stats = assertNotSymlink(home, "session root");
  if (!stats) return false;
  if (!stats.isDirectory()) {
    throw new Error(`[incodex] refuse to burn non-directory: ${home}`);
  }
  if (expected.ino != null && stats.ino !== expected.ino) {
    throw new Error("[incodex] session home inode changed; refusing to burn");
  }
  if (expected.dev != null && stats.dev !== expected.dev) {
    throw new Error("[incodex] session home device changed; refusing to burn");
  }
  const realHome = realExisting(home);
  const sessions = realExisting(path.join(expected.userRoot, SESSIONS_NAME));
  assertInsideParent(realHome, sessions);
  assertBurnIdentity(home, expected);
  if (ownerSnapshot) assertBurnOwner(home, ownerSnapshot);
  fs.rmSync(home, { recursive: true, force: false });
  return true;
}

function assertBurnOwner(home, expected) {
  const owner = readOwner(home);
  const start = owner.processStartIdentity || owner.startedAt;
  if (owner.pid !== expected.pid || start !== expected.processStartIdentity) {
    throw new Error("[incodex] session owner changed; refusing to burn");
  }
}

function cleanupExpectedForAttempt(expected, originalRemoved) {
  if (!originalRemoved) return expected;
  // +--------------------------------------------------------------------+
  // | 只有删除过有 inode/dev 证明的原 root，才允许同一路径重建后降级。 |
  // +--------------------------------------------------------------------+
  const { ino: _ino, dev: _dev, ...late } = expected;
  return late;
}

function validSessionId(sessionId) {
  return typeof sessionId === "string" && /^s-[A-Za-z0-9]+$/.test(sessionId);
}

function burnProofPath(userRoot, sessionId) {
  if (!validSessionId(sessionId)) return null;
  const sessions = path.join(userRoot, SESSIONS_NAME);
  const stats = assertNotSymlink(sessions, "sessions directory");
  if (!stats?.isDirectory()) return null;
  const realSessions = realExisting(sessions);
  const proof = path.join(realSessions, `${BURN_PROOF_PREFIX}${sessionId}${BURN_PROOF_SUFFIX}`);
  assertInsideParent(proof, realSessions);
  return proof;
}

function writeBurnProof(root, expected) {
  if (!expected || !validSessionId(expected.sessionId)) return false;
  if (!Number.isSafeInteger(expected.ino) || !Number.isSafeInteger(expected.dev)) return false;
  try {
    const proof = burnProofPath(expected.userRoot, expected.sessionId);
    if (!proof) return false;
    writePrivateFile(
      proof,
      `${JSON.stringify({
        sessionId: expected.sessionId,
        root: path.resolve(root),
        ino: expected.ino,
        dev: expected.dev,
      })}\n`,
      { exclusive: true },
    );
    return true;
  } catch {
    return false;
  }
}

function readBurnProof(root, userRoot, sessionId) {
  if (!validSessionId(sessionId)) return null;
  try {
    const proof = burnProofPath(userRoot, sessionId);
    if (!proof || !assertNotSymlink(proof, "burn proof")) return null;
    const rootStats = lstatOrNull(root);
    if (rootStats?.isSymbolicLink()) return null;
    const currentRoot = rootStats ? realExisting(root) : path.resolve(root);
    const record = JSON.parse(fs.readFileSync(proof, "utf8"));
    if (
      record.sessionId !== sessionId ||
      record.root !== currentRoot ||
      !Number.isSafeInteger(record.ino) ||
      !Number.isSafeInteger(record.dev)
    ) {
      return null;
    }
    return { userRoot, sessionId, ino: record.ino, dev: record.dev };
  } catch {
    return null;
  }
}

function clearBurnProof(userRoot, sessionId) {
  try {
    const proof = burnProofPath(userRoot, sessionId);
    if (!proof || !assertNotSymlink(proof, "burn proof")) return false;
    fs.rmSync(proof, { force: false });
    return true;
  } catch {
    return false;
  }
}

async function cleanupExitedSession(session, childOwner, options = {}) {
  const userRoot = options.userRoot;
  const quiesce = options.quiesceSessionHelpers;
  const readProof = options.readBurnProof ?? readBurnProof;
  const burn = options.burnSessionHome ?? burnSessionHome;
  const burnWithOwner = options.burnSessionHomeWithOwner ?? burnSessionHomeWithOwner;
  const cleanupExpected = options.cleanupExpectedForAttempt ?? cleanupExpectedForAttempt;
  const exists = options.exists ?? fs.existsSync;
  const clearProof = options.clearBurnProof ?? clearBurnProof;
  const wait = options.wait ?? ((ms) => new Promise((resolve) => setTimeout(resolve, ms)));
  const log = options.log ?? (() => {});

  try {
    if (typeof quiesce !== "function") throw new Error("session helper quiescence is unavailable");
    await quiesce(session.root);
  } catch (error) {
    log("parent-quiesce-refused", { error: String(error), sessionId: session.sessionId });
    return false;
  }

  const expected = {
    userRoot,
    sessionId: session.sessionId,
    ino: session.ino,
    dev: session.dev,
  };
  const childProof = readProof(session.root, userRoot, session.sessionId);
  let originalRemoved = Boolean(
    childProof && childProof.ino === expected.ino && childProof.dev === expected.dev,
  );

  for (let attempt = 1; attempt <= 5; attempt += 1) {
    const attemptExpected = cleanupExpected(expected, originalRemoved);
    try {
      const removed = childOwner && !originalRemoved
        ? burnWithOwner(session.root, attemptExpected, childOwner)
        : burn(session.root, attemptExpected);
      if (removed) originalRemoved = true;
    } catch (error) {
      if (attempt < 5) {
        await wait(250 * attempt);
        continue;
      }
      log("parent-burn-refused", { error: String(error), sessionId: session.sessionId });
      return false;
    }
    if (attempt < 5 && (originalRemoved || exists(session.root))) {
      await wait(400 * attempt);
      continue;
    }
    break;
  }

  if (exists(session.root)) {
    log("parent-burn-retained", { sessionId: session.sessionId });
    return false;
  }
  clearProof(userRoot, session.sessionId);
  return true;
}

function copySettings(home, sourceHome, liveBounds: any = null) {
  const homeStat = assertNotSymlink(home, "session home");
  if (!homeStat?.isDirectory()) throw new Error(`[incodex] session home missing: ${home}`);
  let copied = 0;
  for (const name of SETTINGS_FILES) {
    const src = path.join(sourceHome, name);
    if (!assertNotSymlink(src, "source setting")) continue;
    exclusiveCopyFile(src, path.join(home, name));
    copied += 1;
  }
  seedWindowState(home, sourceHome, liveBounds);
  return copied;
}

function validatedWindowBounds(value, requireMaximized = true) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const { x, y, width, height, isMaximized } = value;
  if (![x, y, width, height].every(Number.isSafeInteger)) return null;
  if (x < -0x80000000 || x > 0x7fffffff || y < -0x80000000 || y > 0x7fffffff) return null;
  if (width < MAIN_WINDOW_MIN_WIDTH || width > 0x7fffffff) return null;
  if (height < MAIN_WINDOW_MIN_HEIGHT || height > 0x7fffffff) return null;
  if (requireMaximized && typeof isMaximized !== "boolean") return null;
  return { x, y, width, height };
}

// 投影稳定几何，并补回官方因文件预建而跳过的空 Home 初始化哨兵。
function seedWindowState(home, sourceHome, liveBounds: any = null) {
  const homeStat = assertNotSymlink(home, "session home");
  if (!homeStat?.isDirectory()) throw new Error(`[incodex] session home missing: ${home}`);
  let bounds = validatedWindowBounds(liveBounds, false);
  if (!bounds) {
    const source = path.join(sourceHome, GLOBAL_STATE_NAME);
    const sourceStat = assertNotSymlink(source, "source global state");
    if (!sourceStat) return false;
    if (!sourceStat.isFile()) {
      throw new Error(`[incodex] source global state is not a file: ${source}`);
    }
    let sourceState;
    try {
      sourceState = JSON.parse(fs.readFileSync(source, "utf8"));
    } catch {
      return false;
    }
    bounds = validatedWindowBounds(sourceState?.[MAIN_WINDOW_BOUNDS_KEY]);
  }
  if (!bounds) return false;
  const state = {
    [DESKTOP_FIRST_SEEN_AT_MS_KEY]: Date.now(),
    [MAIN_WINDOW_BOUNDS_KEY]: bounds,
    [PERSISTED_ATOM_STATE_KEY]: {
      [MIGRATION_ANNOUNCEMENT_KEY]: true,
      [UPDATE_ANNOUNCEMENT_KEY]: true,
    },
  };
  writePrivateFile(
    path.join(home, GLOBAL_STATE_NAME),
    `${JSON.stringify(state)}\n`,
    { exclusive: true },
  );
  return true;
}

function processStatus(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return "dead";
  try {
    process.kill(pid, 0);
    return "live";
  } catch (error) {
    return error?.code === "ESRCH" ? "dead" : "unknown";
  }
}

function sweepOrphanSessions(userRoot, options = {}) {
  const sessions = path.join(userRoot, SESSIONS_NAME);
  if (!lstatOrNull(sessions)?.isDirectory() || lstatOrNull(sessions)?.isSymbolicLink()) return 0;
  const roots = listSessionRoots(sessions, options.targetId);
  let swept = 0;
  for (const root of roots) {
    if (options.keepSessionId && path.basename(root) === options.keepSessionId) continue;
    const sessionId = path.basename(root);
    const proof = readBurnProof(root, userRoot, sessionId);
    if (proof) {
      try {
        const removed = burnSessionHome(root, cleanupExpectedForAttempt(proof, true));
        if (removed) {
          clearBurnProof(userRoot, sessionId);
          swept += 1;
        }
      } catch {
        /* leave a proven late root for the next bounded janitor pass */
      }
      continue;
    }
    try {
      const owner = readOwner(root);
      if (!Number.isInteger(owner.pid)) continue;
      if (owner.handoffPending === true) continue;
      const expectedStart = owner.processStartIdentity || owner.startedAt;
      if (expectedStart && !ownerCore.isCanonicalProcessStartIdentity(expectedStart)) continue;
      let status;
      if (options.pidAlive) {
        status = options.pidAlive(owner.pid) ? "live" : "dead";
      } else {
        status = processStatus(owner.pid);
      }
      if (status === "unknown") continue;
      if (status === "live") {
        if (!expectedStart) continue;
        const identify = options.processIdentity || ownerCore.processIdentity;
        const live = identify(owner.pid);
        if (!live || live.processStartIdentity === expectedStart) continue;
      }
      if (!Number.isSafeInteger(owner.ino) || !Number.isSafeInteger(owner.dev)) continue;
      const expected = {
        userRoot,
        sessionId: owner.sessionId,
        ino: owner.ino,
        dev: owner.dev,
      };
      const removed = expectedStart
        ? burnSessionHomeWithOwner(root, expected, {
            pid: owner.pid,
            processStartIdentity: expectedStart,
          })
        : burnSessionHome(root, expected);
      if (removed) swept += 1;
    } catch {
      /* leave it if we cannot prove it is safe */
    }
  }
  for (const name of ["incognito-home", "incognito-chromium"]) {
    const leftover = path.join(userRoot, name);
    const stats = lstatOrNull(leftover);
    if (!stats || stats.isSymbolicLink()) continue;
    try {
      fs.rmSync(leftover, { recursive: true, force: false });
    } catch {
      /* ignore */
    }
  }
  return swept;
}

function listSessionRoots(sessions, targetId) {
  const roots = [];
  const start = targetId ? path.join(sessions, targetId) : sessions;
  const startStat = lstatOrNull(start);
  if (!startStat || startStat.isSymbolicLink() || !startStat.isDirectory()) return roots;
  for (const name of fs.readdirSync(start)) {
    const child = path.join(start, name);
    const stats = lstatOrNull(child);
    if (!stats || stats.isSymbolicLink() || !stats.isDirectory()) continue;
    if (name.startsWith("s-")) roots.push(child);
    else if (!targetId) {
      for (const nested of fs.readdirSync(child)) {
        const nest = path.join(child, nested);
        const nestStat = lstatOrNull(nest);
        if (nestStat && !nestStat.isSymbolicLink() && nestStat.isDirectory() && nested.startsWith("s-")) {
          roots.push(nest);
        }
      }
    }
  }
  return roots;
}

function writeReady(sessionRoot) {
  writePrivateFile(path.join(sessionRoot, READY_NAME), `${Date.now()}\n`, { exclusive: true });
}

function hasReady(sessionRoot) {
  const file = path.join(sessionRoot, READY_NAME);
  const stats = lstatOrNull(file);
  return Boolean(stats && !stats.isSymbolicLink());
}

function rotateAndAppendLog(userRoot, line) {
  const logs = ensurePrivateDir(path.join(userRoot, LOGS_NAME), userRoot);
  const file = path.join(logs.real, "incognito.log");
  const stats = lstatOrNull(file);
  if (stats?.isSymbolicLink()) throw new Error(`[incodex] refuse to write symlink log: ${file}`);
  if (stats && stats.size >= LOG_LIMIT) {
    for (let index = LOG_KEEP - 1; index >= 1; index -= 1) {
      const from = `${file}.${index}`;
      const to = `${file}.${index + 1}`;
      if (lstatOrNull(from) && !lstatOrNull(from).isSymbolicLink()) {
        fs.renameSync(from, to);
      }
    }
    if (!stats.isSymbolicLink()) fs.renameSync(file, `${file}.1`);
    const extra = `${file}.${LOG_KEEP}`;
    if (lstatOrNull(extra) && !lstatOrNull(extra).isSymbolicLink()) fs.rmSync(extra);
  }
  const fd = fs.openSync(file, OPEN_APPEND, FILE_MODE);
  try {
    fs.writeSync(fd, line);
    fs.fchmodSync(fd, FILE_MODE);
  } finally {
    fs.closeSync(fd);
  }
}

function resolveSourceHome(envHome, fallback) {
  if (typeof envHome === "string" && envHome.trim()) return path.resolve(envHome.trim());
  return fallback;
}

function isManagedSessionHome(home, userRoot) {
  if (!home) return false;
  const stats = lstatOrNull(home);
  if (!stats || stats.isSymbolicLink() || !stats.isDirectory()) return false;
  try {
    const realHome = realExisting(home);
    const sessions = realExisting(path.join(userRoot, SESSIONS_NAME));
    assertInsideParent(realHome, sessions);
    return true;
  } catch {
    return false;
  }
}

function pidFile(userRoot) {
  return path.join(userRoot, PID_NAME);
}

function writePidFile(userRoot, pid) {
  ensurePrivateDir(userRoot, path.dirname(userRoot));
  writePrivateFile(pidFile(userRoot), `${pid}\n`);
}

function clearPidFile(userRoot) {
  const file = pidFile(userRoot);
  const stats = lstatOrNull(file);
  if (!stats) return;
  if (stats.isSymbolicLink()) {
    throw new Error(`[incodex] refuse to remove symlink pid file: ${file}`);
  }
  fs.rmSync(file);
}

function readPidFile(userRoot) {
  const file = pidFile(userRoot);
  const stats = lstatOrNull(file);
  if (!stats) return 0;
  if (stats.isSymbolicLink()) {
    throw new Error(`[incodex] refuse to read symlink pid file: ${file}`);
  }
  const pid = Number(fs.readFileSync(file, "utf8").trim());
  return Number.isInteger(pid) && pid > 0 ? pid : 0;
}

export {
  SESSIONS_NAME,
  LOGS_NAME,
  OWNER_NAME,
  LOCK_NAME,
  READY_NAME,
  SETTINGS_FILES,
  DIR_MODE,
  FILE_MODE,
  LOG_LIMIT,
  assertNotSymlink,
  assertInsideParent,
  ensurePrivateDir,
  writePrivateFile,
  handoffSessionOwner,
  exclusiveCopyFile,
  createSessionHome,
  burnSessionHome,
  burnSessionHomeWithOwner,
  cleanupExpectedForAttempt,
  writeBurnProof,
  readBurnProof,
  clearBurnProof,
  cleanupExitedSession,
  copySettings,
  seedWindowState,
  resolveSourceHome,
  isManagedSessionHome,
  sweepOrphanSessions,
  writeReady,
  hasReady,
  rotateAndAppendLog,
  writePidFile,
  clearPidFile,
  readPidFile,
  sessionRootFromHome,
};
