"use strict";

const { spawn } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const USER_ROOT = path.join(os.homedir(), ".incodex");
const INCOGNITO_HOME = path.join(USER_ROOT, "incognito-home");
const INCOGNITO_CHROMIUM = path.join(USER_ROOT, "incognito-chromium");
const DEFAULT_CODEX_HOME = path.join(os.homedir(), ".codex");
const AUTH_NAME = "auth.json";
const SETTINGS_FILES = [AUTH_NAME, "config.toml"];
const KEEP_ON_BURN = new Set(SETTINGS_FILES);

function resolvedCodexHome() {
  const env = process.env.CODEX_HOME;
  if (env && env.trim()) return path.resolve(env.trim());
  return DEFAULT_CODEX_HOME;
}

function isIncognito() {
  if (process.env.INCODEX_INCOGNITO === "1") return true;
  return path.resolve(resolvedCodexHome()) === path.resolve(INCOGNITO_HOME);
}

function pickFile(name) {
  const override = path.join(USER_ROOT, name);
  if (fs.existsSync(override)) return override;
  return path.join(__dirname, name);
}

function injectSource() {
  const file = pickFile("incodex-inject.js");
  if (!fs.existsSync(file)) return "";
  return fs.readFileSync(file, "utf8");
}

function readLocaleOverride() {
  const file = path.join(DEFAULT_CODEX_HOME, "config.toml");
  if (!fs.existsSync(file)) return "";
  try {
    const match = fs.readFileSync(file, "utf8").match(/^\s*localeOverride\s*=\s*"([^"]+)"/m);
    return match?.[1]?.trim() ?? "";
  } catch {
    return "";
  }
}

function copySettings() {
  fs.mkdirSync(INCOGNITO_HOME, { recursive: true });
  let copied = 0;
  for (const name of SETTINGS_FILES) {
    const src = path.join(DEFAULT_CODEX_HOME, name);
    if (!fs.existsSync(src)) continue;
    fs.copyFileSync(src, path.join(INCOGNITO_HOME, name));
    copied += 1;
  }
  return copied > 0;
}

const PID_NAME = ".incodex-pid";

function assertSafeIncognitoHome() {
  const home = path.resolve(INCOGNITO_HOME);
  const real = path.resolve(DEFAULT_CODEX_HOME);
  const root = path.resolve(USER_ROOT);
  if (home === real) throw new Error("[incodex] refuse to burn real CODEX_HOME");
  if (!home.startsWith(`${root}${path.sep}`)) {
    throw new Error("[incodex] incognito home is outside ~/.incodex");
  }
  return home;
}

function burnIncognitoHome() {
  let home;
  try {
    home = assertSafeIncognitoHome();
  } catch (error) {
    logLaunch("burn-refused", { error: String(error) });
    return;
  }
  if (!fs.existsSync(home)) return;
  for (const name of fs.readdirSync(home)) {
    if (KEEP_ON_BURN.has(name) || name === PID_NAME) continue;
    fs.rmSync(path.join(home, name), { recursive: true, force: true });
  }
  logLaunch("burn");
}

function writePid() {
  fs.mkdirSync(INCOGNITO_HOME, { recursive: true });
  fs.writeFileSync(path.join(INCOGNITO_HOME, PID_NAME), String(process.pid));
}

function clearPid() {
  try {
    fs.rmSync(path.join(INCOGNITO_HOME, PID_NAME), { force: true });
  } catch {
    /* ignore */
  }
}

function readIncognitoPid() {
  const file = path.join(INCOGNITO_HOME, PID_NAME);
  if (!fs.existsSync(file)) return 0;
  const pid = Number(fs.readFileSync(file, "utf8").trim());
  if (!Number.isInteger(pid) || pid <= 0) return 0;
  if (pid === process.pid) return pid;
  try {
    process.kill(pid, 0);
    return pid;
  } catch {
    return 0;
  }
}

function incognitoAlreadyRunning() {
  const pid = readIncognitoPid();
  return pid > 0 && pid !== process.pid;
}

function raisePid(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return;
  if (process.platform !== "darwin") return;
  spawn(
    "osascript",
    [
      "-e",
      `tell application "System Events" to set frontmost of (first process whose unix id is ${pid}) to true`,
    ],
    { detached: true, stdio: "ignore" },
  ).unref();
}

function isAuxiliaryWindow(win) {
  if (!win || win.isDestroyed()) return true;
  try {
    if (typeof win.isAlwaysOnTop === "function" && win.isAlwaysOnTop()) return true;
    if (typeof win.isFocusable === "function" && !win.isFocusable()) return true;
    const bounds = win.getBounds();
    if (bounds.width < 400 || bounds.height < 300) return true;
  } catch {
    return true;
  }
  return false;
}

function mainWindows(electron) {
  return electron.BrowserWindow.getAllWindows().filter((win) => !isAuxiliaryWindow(win));
}

function hideAuxiliaryWindows(electron) {
  for (const win of electron.BrowserWindow.getAllWindows()) {
    if (!isAuxiliaryWindow(win)) continue;
    try {
      win.hide();
    } catch {
      /* ignore */
    }
  }
}

function raiseOurWindows() {
  let electron;
  try {
    electron = require("electron");
  } catch {
    raisePid(process.pid);
    return;
  }
  hideAuxiliaryWindows(electron);
  try {
    if (process.platform === "darwin") electron.app.focus({ steal: true });
  } catch {
    /* ignore */
  }
  for (const win of mainWindows(electron)) {
    try {
      if (win.isMinimized()) win.restore();
      win.show();
      win.focus();
      if (typeof win.moveTop === "function") win.moveTop();
    } catch {
      /* ignore */
    }
  }
  raisePid(process.pid);
}

function raiseExistingIncognito() {
  const pid = readIncognitoPid();
  if (!pid) return false;
  try {
    process.kill(pid, "SIGUSR1");
  } catch {
    /* process may not listen yet */
  }
  raisePid(pid);
  setTimeout(() => raisePid(pid), 200);
  setTimeout(() => raisePid(pid), 600);
  logLaunch("raise-existing", { pid });
  return true;
}

function raiseChildWhenReady(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return;
  for (const delay of [150, 400, 800, 1400]) {
    setTimeout(() => raisePid(pid), delay);
  }
}

function logLaunch(message, extra) {
  try {
    fs.appendFileSync(
      path.join(USER_ROOT, "incognito.log"),
      `${new Date().toISOString()} ${message}${extra ? ` ${JSON.stringify(extra)}` : ""}\n`,
    );
  } catch {
    /* ignore */
  }
}

// Chrome NewIncognitoWindow -> NewEmptyWindow -> OpenEmptyWindow -> WindowSizer.
// Mac tile is kWindowTilePixels = 22 in window_sizer_mac.mm; Aura/Linux/Win is 10.
const CHROME_WINDOW_TILE_PIXELS = process.platform === "darwin" ? 22 : 10;
const CHROME_MIN_VISIBLE = 30;

function captureSourceBounds() {
  try {
    const { BrowserWindow } = require("electron");
    const win = BrowserWindow.getFocusedWindow() || BrowserWindow.getAllWindows()[0];
    if (!win || win.isDestroyed()) return "";
    const b = win.getBounds();
    return `${b.x},${b.y},${b.width},${b.height}`;
  } catch {
    return "";
  }
}

function readSourceBounds() {
  const raw = process.env.INCODEX_SOURCE_BOUNDS;
  if (!raw) return null;
  const parts = raw.split(",").map(Number);
  if (parts.length !== 4 || parts.some((n) => !Number.isFinite(n))) return null;
  return { x: parts[0], y: parts[1], width: parts[2], height: parts[3] };
}

function chromeTileBounds(source, screen) {
  const bounds = {
    x: source.x + CHROME_WINDOW_TILE_PIXELS,
    y: source.y + CHROME_WINDOW_TILE_PIXELS,
    width: source.width,
    height: source.height,
  };
  const display = screen.getDisplayMatching(bounds);
  const work = display.workArea;
  bounds.height = Math.max(CHROME_MIN_VISIBLE, bounds.height);
  bounds.width = Math.max(CHROME_MIN_VISIBLE, bounds.width);
  if (bounds.y < work.y) bounds.y = work.y;
  if (process.platform === "darwin") {
    bounds.height = Math.min(work.height, bounds.height);
    if (bounds.x < work.x || bounds.x + bounds.width > work.x + work.width) {
      bounds.x = work.x;
    }
    if (bounds.y < work.y || bounds.y + bounds.height > work.y + work.height) {
      bounds.y = work.y;
    }
  } else {
    const minX = work.x + CHROME_MIN_VISIBLE - bounds.width;
    const minY = work.y + CHROME_MIN_VISIBLE - bounds.height;
    const maxX = work.x + work.width - CHROME_MIN_VISIBLE;
    const maxY = work.y + work.height - CHROME_MIN_VISIBLE;
    bounds.x = Math.min(Math.max(bounds.x, minX), maxX);
    bounds.y = Math.min(Math.max(bounds.y, minY), maxY);
  }
  return bounds;
}

function applyChromeWindowTile(win) {
  if (!win || win.isDestroyed()) return;
  const source = readSourceBounds();
  if (!source) return;
  let screen;
  try {
    screen = require("electron").screen;
  } catch {
    return;
  }
  try {
    win.setBounds(chromeTileBounds(source, screen));
  } catch {
    /* ignore */
  }
}

function launchIncognito() {
  if (incognitoAlreadyRunning()) {
    raiseExistingIncognito();
    return Promise.resolve({ ok: true, reason: "already-running" });
  }
  try {
    fs.mkdirSync(INCOGNITO_HOME, { recursive: true });
    fs.mkdirSync(INCOGNITO_CHROMIUM, { recursive: true });
    burnIncognitoHome();
    copySettings();
  } catch (error) {
    logLaunch("prepare-failed", { error: String(error) });
    return Promise.resolve({ ok: false, reason: "prepare-failed" });
  }
  const bin = process.execPath;
  if (!bin) return Promise.resolve({ ok: false, reason: "spawn-failed" });
  const args = [`--user-data-dir=${INCOGNITO_CHROMIUM}`];
  const sourceBounds = captureSourceBounds();
  logLaunch("launch", {
    bin,
    home: INCOGNITO_HOME,
    userData: INCOGNITO_CHROMIUM,
    sourceBounds,
    tile: CHROME_WINDOW_TILE_PIXELS,
  });
  return new Promise((resolve) => {
    let settled = false;
    const done = (result) => {
      if (settled) return;
      settled = true;
      resolve(result);
    };
    let child;
    try {
      child = spawn(bin, args, {
        detached: true,
        stdio: "ignore",
        env: {
          ...process.env,
          CODEX_HOME: INCOGNITO_HOME,
          INCODEX_INCOGNITO: "1",
          CODEX_ELECTRON_USER_DATA_PATH: INCOGNITO_CHROMIUM,
          INCODEX_SOURCE_BOUNDS: sourceBounds,
        },
      });
    } catch (error) {
      logLaunch("spawn-threw", { error: String(error) });
      done({ ok: false, reason: "spawn-failed" });
      return;
    }
    if (!child.pid) {
      logLaunch("spawn-no-pid");
      done({ ok: false, reason: "spawn-failed" });
      return;
    }
    child.once("error", (error) => {
      logLaunch("spawn-error", { error: String(error) });
      done({ ok: false, reason: "spawn-failed" });
    });
    child.once("exit", (code) => {
      if (settled) return;
      logLaunch("exited-early", { code });
      done({ ok: false, reason: "exited-early" });
    });
    raiseChildWhenReady(child.pid);
    setTimeout(() => done({ ok: true }), 500);
    child.unref();
  });
}

function interceptOpenBeacon(session) {
  if (!session?.webRequest || session.__incodexBeacon) return;
  session.__incodexBeacon = true;
  session.webRequest.onBeforeRequest({ urls: ["*://incodex.invalid/*"] }, (details, callback) => {
    const url = String(details.url || "");
    if (url.includes("/open")) launchIncognito();
    if (url.includes("/quit") && isIncognito()) {
      burnIncognitoHome();
      clearPid();
      try {
        require("electron").app.quit();
      } catch {
        /* ignore */
      }
    }
    callback({ cancel: true });
  });
}

function hookPreload(session) {
  if (!session || session.__incodexPreload) return;
  session.__incodexPreload = true;
  interceptOpenBeacon(session);
  const preload = pickFile("incodex-preload.cjs");
  if (!fs.existsSync(preload)) return;
  try {
    if (typeof session.registerPreloadScript === "function") {
      session.registerPreloadScript({ filePath: preload, type: "frame" });
    } else if (typeof session.setPreloads === "function") {
      session.setPreloads([...(session.getPreloads?.() ?? []), preload]);
    }
  } catch (error) {
    console.error("[incodex] preload failed", error);
  }
}

function hookWindow(win, source) {
  if (!win?.webContents || isAuxiliaryWindow(win)) return;
  hookPreload(win.webContents.session);
  const run = () => {
    if (!source || win.webContents.isDestroyed()) return;
    const locale = JSON.stringify(readLocaleOverride());
    const prefix = `window.__incodexIncognito=${isIncognito() ? "true" : "false"};window.__incodexLocale=${locale};`;
    win.webContents.executeJavaScript(prefix + source, false).catch(() => {});
  };
  win.webContents.on("dom-ready", run);
  win.webContents.on("did-finish-load", run);
  run();
}

function attachElectron() {
  let electron;
  try {
    electron = require("electron");
  } catch {
    return;
  }

  if (isIncognito()) {
    process.env.CODEX_HOME = INCOGNITO_HOME;
    process.env.INCODEX_INCOGNITO = "1";
    process.env.CODEX_ELECTRON_USER_DATA_PATH = INCOGNITO_CHROMIUM;
  }

  const source = injectSource();
  electron.ipcMain.handle("incodex-open-incognito", async () => {
    if (isIncognito()) return { ok: false, reason: "already-incognito" };
    return launchIncognito();
  });
  electron.ipcMain.handle("incodex-quit-incognito", async () => {
    if (!isIncognito()) return { ok: false, reason: "not-incognito" };
    burnIncognitoHome();
    clearPid();
    electron.app.quit();
    return { ok: true };
  });
  electron.ipcMain.on("incodex-is-incognito", (event) => {
    event.returnValue = isIncognito();
  });

  electron.app.on("browser-window-created", (_event, win) => {
    if (isAuxiliaryWindow(win)) {
      if (isIncognito()) {
        try {
          win.hide();
        } catch {
          /* ignore */
        }
      }
      return;
    }
    hookWindow(win, source);
    if (!isIncognito()) return;
    applyChromeWindowTile(win);
    const bringForward = () => {
      applyChromeWindowTile(win);
      raiseOurWindows();
    };
    win.once("ready-to-show", bringForward);
    win.once("show", () => {
      bringForward();
      setTimeout(bringForward, 50);
      setTimeout(bringForward, 300);
    });
    win.on("closed", () => {
      if (mainWindows(electron).some((open) => open !== win && !open.isDestroyed())) return;
      burnIncognitoHome();
      clearPid();
      electron.app.exit(0);
    });
  });
  if (isIncognito()) {
    writePid();
    process.on("SIGUSR1", () => raiseOurWindows());
    electron.app.on("window-all-closed", () => {
      burnIncognitoHome();
      clearPid();
      electron.app.exit(0);
    });
    electron.app.on("before-quit", () => {
      burnIncognitoHome();
      clearPid();
    });
  }

  const ready = () => {
    hookPreload(electron.session.defaultSession);
    for (const win of electron.BrowserWindow.getAllWindows()) hookWindow(win, source);
    if (isIncognito()) raiseOurWindows();
  };
  if (electron.app.isReady()) ready();
  else void electron.app.whenReady().then(ready);
}

try {
  attachElectron();
} catch (error) {
  console.error("[incodex] main attach failed", error);
}
