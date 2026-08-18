"use strict";

const fs = require("node:fs");
const path = require("node:path");

const SESSIONS_NAME = "sessions";
const OWNER_NAME = "owner.json";
const PID_NAME = "incognito.pid";
const SETTINGS_FILES = ["auth.json", "config.toml"];
const DIR_MODE = 0o700;
const FILE_MODE = 0o600;

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
  if (prior?.isSymbolicLink()) {
    throw new Error(`[incodex] refuse to overwrite symlink file: ${dest}`);
  }
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

function exclusiveCopyFile(src, dest) {
  const destStat = assertNotSymlink(dest, "copy destination");
  if (destStat) {
    throw new Error(`[incodex] refuse to overwrite existing file: ${dest}`);
  }
  const data = fs.readFileSync(src);
  writePrivateFile(dest, data, { exclusive: true });
}

function createSessionHome(userRoot) {
  const parent = path.dirname(userRoot);
  ensurePrivateDir(userRoot, parent);
  const sessions = path.join(userRoot, SESSIONS_NAME);
  const sessionParent = ensurePrivateDir(sessions, userRoot);
  const home = fs.mkdtempSync(path.join(sessions, "s-"));
  fs.chmodSync(home, DIR_MODE);
  const homeStat = assertNotSymlink(home, "session home");
  if (!homeStat?.isDirectory()) throw new Error(`[incodex] session home is not a directory: ${home}`);
  const realHome = realExisting(home);
  assertInsideParent(realHome, sessionParent.real);
  const sessionId = path.basename(home);
  const owner = {
    sessionId,
    createdAt: new Date().toISOString(),
    ino: homeStat.ino,
    dev: homeStat.dev,
  };
  writePrivateFile(path.join(home, OWNER_NAME), `${JSON.stringify(owner)}\n`, { exclusive: true });
  return {
    sessionId,
    home: realHome,
    root: realExisting(userRoot),
    ino: homeStat.ino,
    dev: homeStat.dev,
  };
}

function readOwner(home) {
  const file = path.join(home, OWNER_NAME);
  const stats = assertNotSymlink(file, "owner manifest");
  if (!stats) throw new Error(`[incodex] missing session owner: ${file}`);
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function burnSessionHome(home, expected) {
  const stats = assertNotSymlink(home, "session home");
  if (!stats) return;
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
  if (expected.sessionId) {
    const owner = readOwner(home);
    if (owner.sessionId !== expected.sessionId) {
      throw new Error("[incodex] session id mismatch; refusing to burn");
    }
  }
  fs.rmSync(home, { recursive: true, force: false });
}

function copySettings(home, sourceHome) {
  const homeStat = assertNotSymlink(home, "session home");
  if (!homeStat?.isDirectory()) throw new Error(`[incodex] session home missing: ${home}`);
  let copied = 0;
  for (const name of SETTINGS_FILES) {
    const src = path.join(sourceHome, name);
    if (!fs.existsSync(src)) continue;
    exclusiveCopyFile(src, path.join(home, name));
    copied += 1;
  }
  return copied;
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

module.exports = {
  SESSIONS_NAME,
  OWNER_NAME,
  SETTINGS_FILES,
  DIR_MODE,
  FILE_MODE,
  assertNotSymlink,
  assertInsideParent,
  ensurePrivateDir,
  writePrivateFile,
  exclusiveCopyFile,
  createSessionHome,
  burnSessionHome,
  copySettings,
  isManagedSessionHome,
  writePidFile,
  clearPidFile,
  readPidFile,
};
