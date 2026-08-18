// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.LOG_LIMIT = exports.FILE_MODE = exports.DIR_MODE = exports.SETTINGS_FILES = exports.READY_NAME = exports.LOCK_NAME = exports.OWNER_NAME = exports.LOGS_NAME = exports.IDENTITY_NAME = exports.SESSIONS_NAME = void 0;
exports.assertNotSymlink = assertNotSymlink;
exports.assertInsideParent = assertInsideParent;
exports.ensurePrivateDir = ensurePrivateDir;
exports.writePrivateFile = writePrivateFile;
exports.exclusiveCopyFile = exclusiveCopyFile;
exports.createSessionHome = createSessionHome;
exports.burnSessionHome = burnSessionHome;
exports.copySettings = copySettings;
exports.syncIdentity = syncIdentity;
exports.resolveSourceHome = resolveSourceHome;
exports.isManagedSessionHome = isManagedSessionHome;
exports.sweepOrphanSessions = sweepOrphanSessions;
exports.writeReady = writeReady;
exports.hasReady = hasReady;
exports.rotateAndAppendLog = rotateAndAppendLog;
exports.writePidFile = writePidFile;
exports.clearPidFile = clearPidFile;
exports.readPidFile = readPidFile;
exports.sessionRootFromHome = sessionRootFromHome;
const fs = require("node:fs");
const path = require("node:path");
const SESSIONS_NAME = "sessions";
exports.SESSIONS_NAME = SESSIONS_NAME;
const IDENTITY_NAME = "identity";
exports.IDENTITY_NAME = IDENTITY_NAME;
const LOGS_NAME = "logs";
exports.LOGS_NAME = LOGS_NAME;
const OWNER_NAME = "owner.json";
exports.OWNER_NAME = OWNER_NAME;
const LOCK_NAME = "lock";
exports.LOCK_NAME = LOCK_NAME;
const READY_NAME = "ready";
exports.READY_NAME = READY_NAME;
const PID_NAME = "incognito.pid";
const SETTINGS_FILES = ["auth.json", "config.toml"];
exports.SETTINGS_FILES = SETTINGS_FILES;
const DIR_MODE = 0o700;
exports.DIR_MODE = DIR_MODE;
const FILE_MODE = 0o600;
exports.FILE_MODE = FILE_MODE;
const LOG_LIMIT = 1024 * 1024;
exports.LOG_LIMIT = LOG_LIMIT;
const LOG_KEEP = 3;
const OPEN_EXCLUSIVE = fs.constants.O_WRONLY |
    fs.constants.O_CREAT |
    fs.constants.O_EXCL |
    (fs.constants.O_NOFOLLOW || 0);
const OPEN_PRIVATE = fs.constants.O_WRONLY |
    fs.constants.O_CREAT |
    fs.constants.O_TRUNC |
    (fs.constants.O_NOFOLLOW || 0);
const OPEN_APPEND = fs.constants.O_WRONLY |
    fs.constants.O_CREAT |
    fs.constants.O_APPEND |
    (fs.constants.O_NOFOLLOW || 0);
function lstatOrNull(target) {
    try {
        return fs.lstatSync(target);
    }
    catch (error) {
        if (error && error.code === "ENOENT")
            return null;
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
    }
    else if (!existing.isDirectory()) {
        throw new Error(`[incodex] expected directory: ${dir}`);
    }
    fs.chmodSync(dir, DIR_MODE);
    const again = assertNotSymlink(dir, "directory");
    if (!again?.isDirectory())
        throw new Error(`[incodex] directory vanished: ${dir}`);
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
    }
    finally {
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
function sessionsBase(userRoot, targetId) {
    const sessions = path.join(userRoot, SESSIONS_NAME);
    const sessionParent = ensurePrivateDir(sessions, userRoot);
    if (!targetId)
        return sessionParent;
    return ensurePrivateDir(path.join(sessions, targetId), sessionParent.real);
}
function createSessionHome(userRoot, options = {}) {
    const parent = path.dirname(userRoot);
    ensurePrivateDir(userRoot, parent);
    ensurePrivateDir(path.join(userRoot, IDENTITY_NAME), userRoot);
    ensurePrivateDir(path.join(userRoot, LOGS_NAME), userRoot);
    const sessionParent = sessionsBase(userRoot, options.targetId);
    const root = fs.mkdtempSync(path.join(sessionParent.real, "s-"));
    fs.chmodSync(root, DIR_MODE);
    const rootStat = assertNotSymlink(root, "session root");
    if (!rootStat?.isDirectory())
        throw new Error(`[incodex] session root is not a directory: ${root}`);
    const realRoot = realExisting(root);
    assertInsideParent(realRoot, sessionParent.real);
    const home = ensurePrivateDir(path.join(realRoot, "codex-home"), realRoot);
    const chromium = ensurePrivateDir(path.join(realRoot, "chromium"), realRoot);
    const sessionId = path.basename(realRoot);
    writePrivateFile(path.join(realRoot, LOCK_NAME), `${options.pid || ""}\n`, { exclusive: true });
    const owner = {
        sessionId,
        targetId: options.targetId || "",
        pid: options.pid || 0,
        sourceHome: options.sourceHome || "",
        createdAt: new Date().toISOString(),
        ino: rootStat.ino,
        dev: rootStat.dev,
    };
    writePrivateFile(path.join(realRoot, OWNER_NAME), `${JSON.stringify(owner)}\n`, { exclusive: true });
    return {
        sessionId,
        root: realRoot,
        home: home.real,
        chromium: chromium.real,
        identity: realExisting(path.join(userRoot, IDENTITY_NAME)),
        ino: rootStat.ino,
        dev: rootStat.dev,
    };
}
function readOwner(home) {
    const file = path.join(home, OWNER_NAME);
    const stats = assertNotSymlink(file, "owner manifest");
    if (!stats)
        throw new Error(`[incodex] missing session owner: ${file}`);
    return JSON.parse(fs.readFileSync(file, "utf8"));
}
function sessionRootFromHome(home) {
    const base = path.basename(home);
    if (base === "codex-home" || base === "chromium")
        return path.dirname(home);
    return home;
}
function burnSessionHome(target, expected) {
    const home = sessionRootFromHome(target);
    const stats = assertNotSymlink(home, "session root");
    if (!stats)
        return;
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
function removePrivateFile(dest) {
    const stats = assertNotSymlink(dest, "file");
    if (!stats)
        return;
    fs.rmSync(dest);
}
function syncIdentity(userRoot, sourceHome) {
    const identity = ensurePrivateDir(path.join(userRoot, IDENTITY_NAME), userRoot);
    for (const name of SETTINGS_FILES) {
        const src = path.join(sourceHome, name);
        const dest = path.join(identity.real, name);
        if (fs.existsSync(src)) {
            writePrivateFile(dest, fs.readFileSync(src));
        }
        else {
            removePrivateFile(dest);
        }
    }
    return identity.real;
}
function copySettings(home, sourceHome, userRoot) {
    const homeStat = assertNotSymlink(home, "session home");
    if (!homeStat?.isDirectory())
        throw new Error(`[incodex] session home missing: ${home}`);
    const identityDir = syncIdentity(userRoot, sourceHome);
    let copied = 0;
    for (const name of SETTINGS_FILES) {
        const src = path.join(identityDir, name);
        if (!fs.existsSync(src))
            continue;
        exclusiveCopyFile(src, path.join(home, name));
        copied += 1;
    }
    return copied;
}
function pidAlive(pid) {
    if (!Number.isInteger(pid) || pid <= 0)
        return false;
    try {
        process.kill(pid, 0);
        return true;
    }
    catch {
        return false;
    }
}
function sweepOrphanSessions(userRoot, options = {}) {
    const sessions = path.join(userRoot, SESSIONS_NAME);
    if (!lstatOrNull(sessions)?.isDirectory() || lstatOrNull(sessions)?.isSymbolicLink())
        return 0;
    const roots = listSessionRoots(sessions, options.targetId);
    let swept = 0;
    for (const root of roots) {
        if (options.keepSessionId && path.basename(root) === options.keepSessionId)
            continue;
        try {
            const owner = readOwner(root);
            if (owner.pid && pidAlive(owner.pid))
                continue;
            burnSessionHome(root, { userRoot, sessionId: owner.sessionId });
            swept += 1;
        }
        catch {
            try {
                burnSessionHome(root, { userRoot });
                swept += 1;
            }
            catch {
                /* leave it if we cannot prove it is safe */
            }
        }
    }
    for (const name of ["incognito-home", "incognito-chromium"]) {
        const leftover = path.join(userRoot, name);
        const stats = lstatOrNull(leftover);
        if (!stats || stats.isSymbolicLink())
            continue;
        try {
            fs.rmSync(leftover, { recursive: true, force: false });
        }
        catch {
            /* ignore */
        }
    }
    return swept;
}
function listSessionRoots(sessions, targetId) {
    const roots = [];
    const start = targetId ? path.join(sessions, targetId) : sessions;
    const startStat = lstatOrNull(start);
    if (!startStat || startStat.isSymbolicLink() || !startStat.isDirectory())
        return roots;
    for (const name of fs.readdirSync(start)) {
        const child = path.join(start, name);
        const stats = lstatOrNull(child);
        if (!stats || stats.isSymbolicLink() || !stats.isDirectory())
            continue;
        if (name.startsWith("s-"))
            roots.push(child);
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
    if (stats?.isSymbolicLink())
        throw new Error(`[incodex] refuse to write symlink log: ${file}`);
    if (stats && stats.size >= LOG_LIMIT) {
        for (let index = LOG_KEEP - 1; index >= 1; index -= 1) {
            const from = `${file}.${index}`;
            const to = `${file}.${index + 1}`;
            if (lstatOrNull(from) && !lstatOrNull(from).isSymbolicLink()) {
                fs.renameSync(from, to);
            }
        }
        if (!stats.isSymbolicLink())
            fs.renameSync(file, `${file}.1`);
        const extra = `${file}.${LOG_KEEP}`;
        if (lstatOrNull(extra) && !lstatOrNull(extra).isSymbolicLink())
            fs.rmSync(extra);
    }
    const fd = fs.openSync(file, OPEN_APPEND, FILE_MODE);
    try {
        fs.writeSync(fd, line);
        fs.fchmodSync(fd, FILE_MODE);
    }
    finally {
        fs.closeSync(fd);
    }
}
function resolveSourceHome(envHome, fallback) {
    if (typeof envHome === "string" && envHome.trim())
        return path.resolve(envHome.trim());
    return fallback;
}
function isManagedSessionHome(home, userRoot) {
    if (!home)
        return false;
    const stats = lstatOrNull(home);
    if (!stats || stats.isSymbolicLink() || !stats.isDirectory())
        return false;
    try {
        const realHome = realExisting(home);
        const sessions = realExisting(path.join(userRoot, SESSIONS_NAME));
        assertInsideParent(realHome, sessions);
        return true;
    }
    catch {
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
    if (!stats)
        return;
    if (stats.isSymbolicLink()) {
        throw new Error(`[incodex] refuse to remove symlink pid file: ${file}`);
    }
    fs.rmSync(file);
}
function readPidFile(userRoot) {
    const file = pidFile(userRoot);
    const stats = lstatOrNull(file);
    if (!stats)
        return 0;
    if (stats.isSymbolicLink()) {
        throw new Error(`[incodex] refuse to read symlink pid file: ${file}`);
    }
    const pid = Number(fs.readFileSync(file, "utf8").trim());
    return Number.isInteger(pid) && pid > 0 ? pid : 0;
}
