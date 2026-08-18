"use strict";

const fs = require("node:fs");
const path = require("node:path");

const MAIN_NAME = "incodex-main.cjs";

function originalMain() {
  const pkg = JSON.parse(fs.readFileSync(path.join(__dirname, "package.json"), "utf8"));
  const rel = pkg.__incodex && pkg.__incodex.originalMain;
  if (!rel) throw new Error("[incodex] missing __incodex.originalMain");
  return path.join(__dirname, rel);
}

function loadMain() {
  const instance = require("./incodex-instance.cjs");
  const userRoot = path.join(require("node:os").homedir(), ".incodex");
  const override = path.join(instance.targetStateDir(userRoot, process.execPath), MAIN_NAME);
  const bundled = path.join(__dirname, MAIN_NAME);
  const file = fs.existsSync(override) ? override : bundled;
  if (!fs.existsSync(file)) throw new Error("[incodex] missing incodex-main.cjs");
  require(file);
}

try {
  loadMain();
} catch (error) {
  console.error("[incodex] attach failed", error);
}

require(originalMain());
