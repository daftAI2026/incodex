// @ts-nocheck
/**
 * [INPUT]: 依赖 ASAR 原始入口标记、外置 Runtime 清单与 Node 文件/哈希能力。
 * [OUTPUT]: 校验并接入 Runtime，启动官方主入口；native open 交由 Rust 独占管理。
 * [POS]: 唯一写入官方 ASAR 的边界，不以外置 Runtime 更新替代宿主 loader 换代。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
/** @typedef {{ __incodex?: { originalMain?: string } }} IncodexPackage */
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const MAIN_NAME = "incodex-main.cjs";
const MANIFEST_NAME = "runtime-manifest.json";
const RUNTIME_FILES = ["incodex-main.cjs","incodex-main-actions.cjs","incodex-preload.cjs","incodex-inject.js","incodex-safe-home.cjs","incodex-ipc-guard.cjs","incodex-owner-core.cjs","incodex-owner-recovery.cjs","incodex-instance.cjs","incodex-window-kind.cjs","incodex-window-lifecycle.cjs","incodex-runtime-load.cjs","incodex-codex-mode.cjs","incodex-macos-update.cjs","incodex-dock-menu.cjs","incodex-permission-copy.json","incodex-permission-ui.cjs"];
/** @returns {string} */
function originalMain() {
    /** @type {IncodexPackage} */
    const pkg = JSON.parse(fs.readFileSync(path.join(__dirname, "package.json"), "utf8"));
    const rel = pkg.__incodex && pkg.__incodex.originalMain;
    if (!rel)
        throw new Error("[incodex] missing __incodex.originalMain");
    return path.join(__dirname, rel);
}
function sha256File(file) {
    return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}
function sha256Bytes(bytes) {
    return crypto.createHash("sha256").update(bytes).digest("hex");
}
function refuseSymlink(target, label) {
    let stats;
    try {
        stats = fs.lstatSync(target);
    }
    catch (error) {
        if (error && error.code === "ENOENT")
            throw new Error(`[incodex] missing ${label}`);
        throw error;
    }
    if (stats.isSymbolicLink())
        throw new Error(`[incodex] refuse symlink ${label}`);
    if (label !== "runtime root" && !stats.isFile() && !stats.isDirectory()) {
        throw new Error(`[incodex] invalid ${label}`);
    }
    return stats;
}
function isSha256(value) {
    return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}
function isSourceCommit(value) {
    return value === "" || (typeof value === "string" && /^[0-9a-fA-F]{40}$/.test(value));
}
function hotMain(env, execPath) {
    if (env.INCODEX_DEV_HOT !== "1")
        return null;
    const home = env.HOME;
    if (typeof home !== "string" || home.length === 0)
        return null;
    const id = crypto
        .createHash("sha256")
        .update(execPath || "unknown")
        .digest("hex")
        .slice(0, 12);
    const override = path.join(home, ".incodex", "targets", id, MAIN_NAME);
    try {
        const stats = fs.lstatSync(override);
        if (!stats.isSymbolicLink() && stats.isFile())
            return override;
    }
    catch {
        return null;
    }
    return null;
}
function readCurrentRuntime(root) {
    const currentPath = path.join(root, "current.json");
    refuseSymlink(currentPath, "current.json");
    const current = JSON.parse(fs.readFileSync(currentPath, "utf8"));
    if (!current ||
        current.schemaVersion !== 1 ||
        typeof current.release !== "string" ||
        typeof current.version !== "string" ||
        current.version.length === 0) {
        throw new Error("[incodex] invalid current.json");
    }
    if (current.release.includes("..") || current.release.startsWith("/") || current.release.includes("\\")) {
        throw new Error("[incodex] invalid runtime release path");
    }
    return current;
}
function resolveReleaseDirectory(root, release) {
    const releaseDir = path.resolve(root, release);
    const prefix = root.endsWith(path.sep) ? root : `${root}${path.sep}`;
    if (releaseDir !== root && !releaseDir.startsWith(prefix)) {
        throw new Error("[incodex] runtime release escaped");
    }
    refuseSymlink(releaseDir, "release directory");
    return releaseDir;
}
function runtimeFiles(current) {
    const files = current.files;
    if (!files || typeof files !== "object" || Array.isArray(files)) {
        throw new Error("[incodex] invalid runtime files");
    }
    return files;
}
function verifyRuntimeManifest(releaseDir, current, files) {
    const hasManifestHash = Object.prototype.hasOwnProperty.call(current, "manifestSha256");
    const hasSourceCommit = Object.prototype.hasOwnProperty.call(current, "sourceCommit");
    if (hasManifestHash !== hasSourceCommit) {
        throw new Error("[incodex] invalid runtime manifest pointer");
    }
    if (!hasManifestHash)
        return;
    if (!isSha256(current.manifestSha256) || !isSourceCommit(current.sourceCommit)) {
        throw new Error("[incodex] invalid runtime manifest pointer");
    }
    const releaseName = path.basename(releaseDir);
    if (releaseName !== `${current.version}-${current.manifestSha256}`) {
        throw new Error("[incodex] runtime release name does not match manifest hash");
    }
    const manifestPath = path.join(releaseDir, MANIFEST_NAME);
    refuseSymlink(manifestPath, MANIFEST_NAME);
    const manifestBytes = fs.readFileSync(manifestPath);
    if (sha256Bytes(manifestBytes) !== current.manifestSha256) {
        throw new Error("[incodex] runtime manifest hash mismatch");
    }
    const manifest = JSON.parse(manifestBytes.toString("utf8"));
    if (!manifest ||
        manifest.runtimeVersion !== current.version ||
        manifest.sourceCommit !== current.sourceCommit ||
        !manifest.files ||
        typeof manifest.files !== "object" ||
        Array.isArray(manifest.files)) {
        throw new Error("[incodex] invalid runtime manifest");
    }
    for (const name of RUNTIME_FILES) {
        if (manifest.files[name] !== files[name]) {
            throw new Error(`[incodex] runtime manifest entry mismatch ${name}`);
        }
    }
}
function verifyRuntimeFiles(releaseDir, files) {
    for (const name of RUNTIME_FILES) {
        const expected = files[name];
        if (!isSha256(expected))
            throw new Error(`[incodex] current.json missing ${name}`);
        const file = path.join(releaseDir, name);
        refuseSymlink(file, name);
        if (sha256File(file) !== expected)
            throw new Error(`[incodex] hash mismatch ${name}`);
    }
}
function externalMain(env) {
    const home = env.HOME;
    if (typeof home !== "string" || home.length === 0) {
        throw new Error("[incodex] HOME is unset");
    }
    const root = path.join(home, ".incodex", "runtime");
    refuseSymlink(root, "runtime root");
    const current = readCurrentRuntime(root);
    const releaseDir = resolveReleaseDirectory(root, current.release);
    const files = runtimeFiles(current);
    verifyRuntimeManifest(releaseDir, current, files);
    verifyRuntimeFiles(releaseDir, files);
    return path.join(releaseDir, MAIN_NAME);
}
function loadMain() {
    const hot = hotMain(process.env, process.execPath);
    const file = hot || externalMain(process.env);
    if (!fs.existsSync(file))
        throw new Error("[incodex] missing incodex-main.cjs");
    const runtime = require(file);
    if (runtime && typeof runtime.startupGate?.then === "function")
        return runtime.startupGate;
    return null;
}
function reportAttachError(error) {
    const text = error && error.message ? String(error.message) : String(error);
    console.error("[incodex] attach failed", text.slice(0, 300));
    return error?.code === "INCODEX_STARTUP_BLOCKED";
}
async function bootstrap() {
    if (process.platform === "darwin" &&
        process.env.INCODEX_NATIVE_OPEN === "1" &&
        process.env.INCODEX_INCOGNITO === "1" &&
        process.env.INCODEX_CLEANUP_OWNER === "native") {
        // native open 独占 CDP、进程监督与会话回收；官方初始化不等待第二个 Runtime owner。
        const official = originalMain();
        require(official);
        return;
    }
    let runtimeStartup = null;
    try {
        runtimeStartup = loadMain();
    }
    catch (error) {
        if (reportAttachError(error))
            return;
    }
    require(originalMain());
    if (!runtimeStartup)
        return;
    try {
        await runtimeStartup;
    }
    catch (error) {
        reportAttachError(error);
    }
}
void bootstrap();
