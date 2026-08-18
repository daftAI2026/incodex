// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.devHotEnabled = devHotEnabled;
exports.hotHomeRoot = hotHomeRoot;
exports.resolveRuntimeFile = resolveRuntimeFile;
const incodex_instance_cts_1 = require("./incodex-instance.cjs");
const fs = require("node:fs");
const path = require("node:path");
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
