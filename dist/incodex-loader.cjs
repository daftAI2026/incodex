// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
/** @typedef {{ __incodex?: { originalMain?: string } }} IncodexPackage */
const fs = require("node:fs");
const path = require("node:path");
const MAIN_NAME = "incodex-main.cjs";
/** @returns {string} */
function originalMain() {
    /** @type {IncodexPackage} */
    const pkg = JSON.parse(fs.readFileSync(path.join(__dirname, "package.json"), "utf8"));
    const rel = pkg.__incodex && pkg.__incodex.originalMain;
    if (!rel)
        throw new Error("[incodex] missing __incodex.originalMain");
    return path.join(__dirname, rel);
}
function loadMain() {
    const { resolveRuntimeFile } = require("./incodex-runtime-load.cjs");
    const file = resolveRuntimeFile(MAIN_NAME, __dirname);
    if (!fs.existsSync(file))
        throw new Error("[incodex] missing incodex-main.cjs");
    require(file);
}
try {
    loadMain();
}
catch (error) {
    console.error("[incodex] attach failed", error);
}
require(originalMain());
