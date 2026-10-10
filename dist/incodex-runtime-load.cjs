// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.createRendererUpdateCoordinator = createRendererUpdateCoordinator;
exports.prepareRendererUpdate = prepareRendererUpdate;
exports.rendererUpdateStillSelected = rendererUpdateStillSelected;
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
// current.json selects a generation; it does not revoke a running process's
// already verified immutable release. Keep its identity, never its file bytes.
const verifiedReleases = new Map();
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
    const current = verifiedReleases.get(releaseDir) ??
        JSON.parse(readRegularFile(currentPath, "current.json").toString("utf8"));
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
    if (!verifiedReleases.has(releaseDir)) {
        verifiedReleases.set(releaseDir, Object.freeze({
            ...current,
            files: Object.freeze({ ...current.files }),
        }));
    }
    return { path: runtimeFile, bytes };
}
// UI candidates use the existing release verifier. Compare all declared assets,
// including native/preload/controller bytes; only the injector may change live.
function prepareRendererUpdate(bundledDir, env = process.env, execPath = process.execPath) {
    const name = "incodex-inject.js";
    readVerifiedRuntimeArtifact(name, bundledDir, env, execPath);
    const baselineDir = path.resolve(bundledDir);
    const baseline = verifiedReleases.get(baselineDir);
    if (!baseline)
        throw new Error("[incodex] renderer update needs a verified release");
    const runtimeRoot = path.dirname(path.dirname(baselineDir));
    const current = JSON.parse(readRegularFile(path.join(runtimeRoot, "current.json"), "current.json").toString("utf8"));
    if (!current || typeof current.release !== "string" || current.release.includes("..") ||
        path.isAbsolute(current.release) || current.release.includes("\\")) {
        throw new Error("[incodex] invalid Runtime pointer");
    }
    const releaseDir = path.resolve(runtimeRoot, current.release);
    const canonicalRoot = fs.realpathSync(runtimeRoot);
    if (fs.realpathSync(releaseDir) !== path.join(canonicalRoot, "releases", path.basename(releaseDir))) {
        throw new Error("[incodex] Runtime release ancestry was redirected");
    }
    const artifact = readVerifiedRuntimeArtifact(name, releaseDir, env, execPath);
    const selection = verifiedReleases.get(releaseDir);
    function manifest(directory, pointer) {
        const bytes = readRegularFile(path.join(directory, RUNTIME_MANIFEST_NAME), RUNTIME_MANIFEST_NAME);
        if (sha256(bytes) !== pointer.manifestSha256)
            throw new Error("[incodex] Runtime manifest hash mismatch");
        const value = JSON.parse(bytes.toString("utf8"));
        const entries = Object.entries(pointer.files);
        if (entries.some(([file, digest]) => !isSha256(digest) || value.files[file] !== digest)) {
            throw new Error("[incodex] Runtime manifest entry mismatch");
        }
        if (Object.entries(value.files).some(([file, digest]) => !isSha256(digest) ||
            (file !== "incodex-loader.cjs" && pointer.files[file] !== digest))) {
            throw new Error("[incodex] Runtime pointer is missing manifest entries");
        }
        return value;
    }
    // A previously pinned candidate must still match today's complete selection.
    if (!sameSelection(selection, current))
        throw new Error("[incodex] Runtime selection changed during preparation");
    const oldManifest = manifest(baselineDir, baseline), nextManifest = manifest(releaseDir, selection);
    const allFiles = new Set([...Object.keys(oldManifest.files), ...Object.keys(nextManifest.files)]);
    const restartRequired = [...allFiles].some(file => file !== name && oldManifest.files[file] !== nextManifest.files[file]);
    if (!restartRequired) {
        // Verify every published file, not only the requested UI. Never execute a
        // mixed or modified generation, even when its declared hashes look equal.
        for (const [file, digest] of Object.entries(selection.files)) {
            if (path.basename(file) !== file || file.includes("\\") || !isSha256(digest)) {
                throw new Error("[incodex] invalid Runtime artifact name");
            }
            const target = path.join(releaseDir, file);
            if (fs.realpathSync(target) !== path.join(fs.realpathSync(releaseDir), file) ||
                sha256(readRegularFile(target, file)) !== digest) {
                throw new Error(`[incodex] Runtime artifact hash mismatch ${file}`);
            }
        }
    }
    return Object.freeze({
        key: selection.manifestSha256, id: selection.files[name], releaseDir, runtimeRoot,
        selection, restartRequired, source: restartRequired ? "" : artifact.bytes.toString("utf8"),
    });
}
function sameSelection(a, b) {
    return a && b && a.schemaVersion === b.schemaVersion && a.release === b.release &&
        a.version === b.version && a.manifestSha256 === b.manifestSha256 && a.sourceCommit === b.sourceCommit &&
        b.files && typeof b.files === "object" && !Array.isArray(b.files) &&
        Object.keys(a.files).length === Object.keys(b.files).length &&
        Object.keys(a.files).every(name => a.files[name] === b.files[name]);
}
function rendererUpdateStillSelected(candidate) {
    try {
        const current = JSON.parse(readRegularFile(path.join(candidate.runtimeRoot, "current.json"), "current.json").toString("utf8"));
        return sameSelection(candidate.selection, current) === true;
    }
    catch {
        return false;
    }
}
// Platform adapters supply existing authorized windows and their real ACK.
// Keep one transaction in flight; publication bursts request a fresh pass.
function createRendererUpdateCoordinator({ initial, prepare, windows, apply, isSelected }) {
    let active = initial, pending = false, running = null, disposed = false;
    let state = { phase: "active", active, windows: [] };
    async function update() {
        let candidate;
        try {
            candidate = prepare();
        }
        catch (error) {
            state = { phase: "retained", active, windows: [], error: String(error) };
            return;
        }
        if (candidate.restartRequired) {
            state = { phase: "restart-required", active, candidate, windows: [] };
            return;
        }
        if (candidate.key === active.key)
            return;
        const attempted = [], results = [];
        state = { phase: "activating", active, candidate, windows: results };
        try {
            if (disposed || !isSelected(candidate))
                throw new Error("Runtime candidate superseded");
            for (const window of windows()) {
                if (disposed || !isSelected(candidate))
                    throw new Error("Runtime candidate superseded");
                // A missing ACK can mean the renderer activated before transport failed.
                // Include that window in rollback, not just the acknowledged ones.
                attempted.push(window);
                if (await apply(window, candidate) !== true)
                    throw new Error("Renderer did not acknowledge activation");
                results.push({ window, state: "acknowledged" });
            }
            if (disposed || !isSelected(candidate))
                throw new Error("Runtime candidate superseded");
            active = candidate;
            state = { phase: "active", active, windows: results };
        }
        catch (error) {
            const rollback = [];
            for (const window of attempted.reverse()) {
                let ok = false;
                try {
                    ok = await apply(window, active) === true;
                }
                catch { /* retain this window's uncertainty */ }
                rollback.push({ window, state: ok ? "rolled-back" : "rollback-failed" });
            }
            state = { phase: rollback.some(item => item.state === "rollback-failed") ? "rollback-failed" : "retained",
                active, candidate, windows: rollback, error: String(error) };
        }
    }
    return {
        refresh() {
            if (disposed)
                return Promise.resolve(state);
            pending = true;
            if (!running)
                running = Promise.resolve().then(async () => {
                    try {
                        while (pending && !disposed) {
                            pending = false;
                            await update();
                        }
                        return state;
                    }
                    finally {
                        running = null;
                    }
                });
            return running;
        },
        status: () => state,
        dispose() { disposed = true; pending = false; },
    };
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
