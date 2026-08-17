"use strict";

const fs = require("node:fs");
const path = require("node:path");

const INJECT_NAME = "incodex-inject.js";

function originalMain() {
  const pkg = JSON.parse(fs.readFileSync(path.join(__dirname, "package.json"), "utf8"));
  const rel = pkg.__incodex && pkg.__incodex.originalMain;
  if (!rel) throw new Error("[incodex] missing __incodex.originalMain");
  return path.join(__dirname, rel);
}

function injectSource() {
  const override = path.join(require("node:os").homedir(), ".incodex", INJECT_NAME);
  const bundled = path.join(__dirname, INJECT_NAME);
  const file = fs.existsSync(override) ? override : bundled;
  if (!fs.existsSync(file)) return "";
  return fs.readFileSync(file, "utf8");
}

function hookWindow(win, source) {
  const contents = win.webContents;
  const run = () => {
    if (!source || contents.isDestroyed()) return;
    contents.executeJavaScript(source, false).catch(() => {});
  };
  contents.on("dom-ready", run);
  contents.on("did-finish-load", run);
  run();
}

function attachElectron() {
  let electron;
  try {
    electron = require("electron");
  } catch {
    return;
  }
  const source = injectSource();
  if (!source) return;

  const hookExisting = () => {
    for (const win of electron.BrowserWindow.getAllWindows()) hookWindow(win, source);
  };

  electron.app.on("browser-window-created", (_event, win) => {
    if (win) hookWindow(win, source);
  });

  if (electron.app.isReady()) hookExisting();
  else void electron.app.whenReady().then(hookExisting);
}

try {
  attachElectron();
} catch (error) {
  console.error("[incodex] attach failed", error);
}

require(originalMain());
