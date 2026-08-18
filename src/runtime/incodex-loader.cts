// @ts-nocheck
"use strict";

/** @typedef {{ __incodex?: { originalMain?: string } }} IncodexPackage */

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");

const MAIN_NAME = "incodex-main.cjs";
const RUNTIME_FILES = [
  "incodex-main.cjs",
  "incodex-preload.cjs",
  "incodex-inject.js",
  "incodex-safe-home.cjs",
  "incodex-ipc-guard.cjs",
  "incodex-instance.cjs",
  "incodex-window-kind.cjs",
  "incodex-runtime-load.cjs",
];

/** @returns {string} */
function originalMain() {
  /** @type {IncodexPackage} */
  const pkg = JSON.parse(fs.readFileSync(path.join(__dirname, "package.json"), "utf8"));
  const rel = pkg.__incodex && pkg.__incodex.originalMain;
  if (!rel) throw new Error("[incodex] missing __incodex.originalMain");
  return path.join(__dirname, rel);
}

function sha256File(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function refuseSymlink(target, label) {
  let stats;
  try {
    stats = fs.lstatSync(target);
  } catch (error) {
    if (error && error.code === "ENOENT") throw new Error(`[incodex] missing ${label}`);
    throw error;
  }
  if (stats.isSymbolicLink()) throw new Error(`[incodex] refuse symlink ${label}`);
  return stats;
}

function hotMain(env, execPath) {
  if (env.INCODEX_DEV_HOT !== "1") return null;
  const home = env.HOME;
  if (typeof home !== "string" || home.length === 0) return null;
  const id = crypto
    .createHash("sha256")
    .update(execPath || "unknown")
    .digest("hex")
    .slice(0, 12);
  const override = path.join(home, ".incodex", "targets", id, MAIN_NAME);
  try {
    const stats = fs.lstatSync(override);
    if (!stats.isSymbolicLink() && stats.isFile()) return override;
  } catch {
    return null;
  }
  return null;
}

function externalMain(env) {
  const home = env.HOME;
  if (typeof home !== "string" || home.length === 0) {
    throw new Error("[incodex] HOME is unset");
  }
  const root = path.join(home, ".incodex", "runtime");
  refuseSymlink(root, "runtime root");
  const currentPath = path.join(root, "current.json");
  refuseSymlink(currentPath, "current.json");
  const current = JSON.parse(fs.readFileSync(currentPath, "utf8"));
  if (!current || current.schemaVersion !== 1 || typeof current.release !== "string") {
    throw new Error("[incodex] invalid current.json");
  }
  if (current.release.includes("..") || current.release.startsWith("/") || current.release.includes("\\")) {
    throw new Error("[incodex] invalid runtime release path");
  }
  const releaseDir = path.resolve(root, current.release);
  const prefix = root.endsWith(path.sep) ? root : `${root}${path.sep}`;
  if (releaseDir !== root && !releaseDir.startsWith(prefix)) {
    throw new Error("[incodex] runtime release escaped");
  }
  refuseSymlink(releaseDir, "release directory");
  const files = current.files || {};
  for (const name of RUNTIME_FILES) {
    const expected = files[name];
    if (typeof expected !== "string") throw new Error(`[incodex] current.json missing ${name}`);
    const file = path.join(releaseDir, name);
    refuseSymlink(file, name);
    if (sha256File(file) !== expected) throw new Error(`[incodex] hash mismatch ${name}`);
  }
  return path.join(releaseDir, MAIN_NAME);
}

function loadMain() {
  const hot = hotMain(process.env, process.execPath);
  const file = hot || externalMain(process.env);
  if (!fs.existsSync(file)) throw new Error("[incodex] missing incodex-main.cjs");
  require(file);
}

try {
  loadMain();
} catch (error) {
  const text = String(error && error.message ? error.message : error);
  console.error("[incodex] attach failed", text.slice(0, 300));
}

require(originalMain());
