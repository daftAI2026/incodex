// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.devHotEnabled = devHotEnabled;
exports.hotHomeRoot = hotHomeRoot;
exports.loadRuntimeModule = loadRuntimeModule;
exports.readRuntimeJson = readRuntimeJson;
exports.readVerifiedRuntimeArtifact = readVerifiedRuntimeArtifact;
exports.resolveRuntimeFile = resolveRuntimeFile;
const incodex_instance_cts_1 = require("./incodex-instance.cjs");
const crypto = require("node:crypto");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const RUNTIME_MANIFEST_NAME = "runtime-manifest.json";
const RUNTIME_FILE_NAME = /^incodex-[a-z-]+\.(?:cjs|js|json)$/;
const MAX_VERIFIED_RUNTIME_FILE_BYTES = 2 * 1024 * 1024;
const runtimeModuleCache = new Map();
function devHotEnabled(env = process.env) {
    return env.INCODEX_DEV_HOT === "1";
}
function hotHomeRoot(env = process.env) {
    const home = env.HOME;
    if (typeof home !== "string" || home.length === 0)
        return null;
    return path.join(home, ".incodex");
}
function resolveRuntimeFile(name, bundledDir, env = process.env, execPath = process.execPath) {
    const bundled = path.join(bundledDir, name);
    if (!devHotEnabled(env))
        return bundled;
    const root = hotHomeRoot(env);
    if (!root)
        return bundled;
    const override = path.join((0, incodex_instance_cts_1.targetStateDir)(root, execPath), name);
    return fs.existsSync(override) ? override : bundled;
}
function sha256(bytes) {
    return crypto.createHash("sha256").update(bytes).digest("hex");
}
function isSha256(value) {
    return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}
function isSourceCommit(value) {
    return value === "" || (typeof value === "string" && /^[0-9a-fA-F]{40}$/.test(value));
}
function readRegularFile(file, label) {
    const stats = fs.lstatSync(file);
    if (stats.isSymbolicLink() || !stats.isFile()) {
        throw new Error(`[incodex] invalid Runtime ${label}`);
    }
    if (stats.size > MAX_VERIFIED_RUNTIME_FILE_BYTES) {
        throw new Error(`[incodex] Runtime ${label} exceeds the size limit`);
    }
    return fs.readFileSync(file);
}
function readVerifiedRuntimeArtifact(name, bundledDir, env = process.env, execPath = process.execPath) {
    if (typeof name !== "string" || !RUNTIME_FILE_NAME.test(name)) {
        throw new Error("[incodex] invalid Runtime artifact name");
    }
    const runtimeFile = path.resolve(resolveRuntimeFile(name, bundledDir, env, execPath));
    const overrideRoot = hotHomeRoot(env);
    const overrideDir = overrideRoot ? path.resolve((0, incodex_instance_cts_1.targetStateDir)(overrideRoot, execPath)) : "";
    let isHotMain = false;
    if (devHotEnabled(env) && path.resolve(bundledDir) === overrideDir) {
        try {
            const stats = fs.lstatSync(path.join(overrideDir, "incodex-main.cjs"));
            isHotMain = !stats.isSymbolicLink() && stats.isFile();
        }
        catch {
            isHotMain = false;
        }
    }
    if (isHotMain) {
        return { path: runtimeFile, bytes: readRegularFile(runtimeFile, name) };
    }
    const releaseDir = path.resolve(bundledDir);
    const releasesDir = path.dirname(releaseDir);
    const runtimeRoot = path.dirname(releasesDir);
    if (path.basename(releasesDir) !== "releases" || path.basename(runtimeRoot) !== "runtime") {
        throw new Error("[incodex] Runtime release directory layout is invalid");
    }
    if (process.platform === "darwin" && typeof env.HOME === "string" && env.HOME.length > 0) {
        const homeRuntimeRoot = path.resolve(env.HOME, ".incodex", "runtime");
        if (fs.realpathSync(runtimeRoot) !== fs.realpathSync(homeRuntimeRoot)) {
            throw new Error("[incodex] Runtime release is outside the current user's Runtime root");
        }
    }
    const rootStats = fs.lstatSync(runtimeRoot);
    if (rootStats.isSymbolicLink() || !rootStats.isDirectory()) {
        throw new Error("[incodex] invalid Runtime root");
    }
    const currentPath = path.join(runtimeRoot, "current.json");
    const current = JSON.parse(readRegularFile(currentPath, "current.json").toString("utf8"));
    if (!current || current.schemaVersion !== 1 ||
        typeof current.release !== "string" || current.release.length === 0 ||
        current.release.includes("..") || path.isAbsolute(current.release) || current.release.includes("\\") ||
        typeof current.version !== "string" || current.version.length === 0 ||
        !isSha256(current.manifestSha256) || !isSourceCommit(current.sourceCommit)) {
        throw new Error("[incodex] invalid Runtime pointer");
    }
    const pointedReleaseDir = path.resolve(runtimeRoot, current.release);
    const rootPrefix = runtimeRoot.endsWith(path.sep) ? runtimeRoot : `${runtimeRoot}${path.sep}`;
    if (!pointedReleaseDir.startsWith(rootPrefix) || pointedReleaseDir !== releaseDir ||
        path.basename(pointedReleaseDir) !== `${current.version}-${current.manifestSha256}`) {
        throw new Error("[incodex] Runtime artifact is outside the active release");
    }
    const releaseStats = fs.lstatSync(pointedReleaseDir);
    if (releaseStats.isSymbolicLink() || !releaseStats.isDirectory()) {
        throw new Error("[incodex] invalid Runtime release directory");
    }
    const canonicalReleaseDir = fs.realpathSync(pointedReleaseDir);
    if (canonicalReleaseDir !== fs.realpathSync(bundledDir)) {
        throw new Error("[incodex] Runtime artifact is outside the active release");
    }
    const activeReleaseDir = canonicalReleaseDir;
    const manifestPath = path.join(activeReleaseDir, RUNTIME_MANIFEST_NAME);
    const manifestBytes = readRegularFile(manifestPath, RUNTIME_MANIFEST_NAME);
    if (sha256(manifestBytes) !== current.manifestSha256) {
        throw new Error("[incodex] Runtime manifest hash mismatch");
    }
    const manifest = JSON.parse(manifestBytes.toString("utf8"));
    if (!manifest || manifest.runtimeVersion !== current.version ||
        manifest.sourceCommit !== current.sourceCommit ||
        !manifest.files || typeof manifest.files !== "object" || Array.isArray(manifest.files) ||
        !current.files || typeof current.files !== "object" || Array.isArray(current.files)) {
        throw new Error("[incodex] invalid Runtime manifest");
    }
    const expected = current.files?.[name];
    if (!isSha256(expected) || manifest.files[name] !== expected) {
        throw new Error(`[incodex] Runtime manifest entry mismatch ${name}`);
    }
    if (fs.realpathSync(runtimeFile) !== path.resolve(activeReleaseDir, name)) {
        throw new Error("[incodex] Runtime artifact path mismatch");
    }
    const bytes = readRegularFile(runtimeFile, name);
    if (sha256(bytes) !== expected) {
        throw new Error(`[incodex] Runtime artifact hash mismatch ${name}`);
    }
    return { path: runtimeFile, bytes };
}
function readRuntimeJson(name, bundledDir, env = process.env, execPath = process.execPath) {
    const { bytes } = readVerifiedRuntimeArtifact(name, bundledDir, env, execPath);
    return JSON.parse(bytes.toString("utf8"));
}
function loadRuntimeModule(name, bundledDir, env = process.env, execPath = process.execPath) {
    const { path: filename, bytes } = readVerifiedRuntimeArtifact(name, bundledDir, env, execPath);
    const digest = sha256(bytes);
    const cached = runtimeModuleCache.get(filename);
    if (cached?.digest === digest)
        return cached.exports;
    const loaded = new Module(filename);
    loaded.filename = filename;
    loaded.paths = Module._nodeModulePaths(path.dirname(filename));
    loaded._compile(bytes.toString("utf8"), filename);
    runtimeModuleCache.set(filename, { digest, exports: loaded.exports });
    return loaded.exports;
}
