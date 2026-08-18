"use strict";

const { spawn } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const safeHome = require("./incodex-safe-home.cjs");

const USER_ROOT = path.join(os.homedir(), ".incodex");
const DEFAULT_CODEX_HOME = path.join(os.homedir(), ".codex");
const READY_TIMEOUT_MS = 15_000;

function targetId() {
  return crypto.createHash("sha256").update(process.execPath || "unknown").digest("hex").slice(0, 12);
}

function resolvedCodexHome() {
  const env = process.env.CODEX_HOME;
  if (env && env.trim()) return path.resolve(env.trim());
  return DEFAULT_CODEX_HOME;
}

function isIncognito() {
  if (process.env.INCODEX_INCOGNITO === "1") return true;
  return safeHome.isManagedSessionHome(resolvedCodexHome(), USER_ROOT);
}

function sessionFromEnv() {
  const home = process.env.CODEX_HOME;
  const sessionId = process.env.INCODEX_SESSION_ID;
  const root = process.env.INCODEX_SESSION_ROOT || (home ? safeHome.sessionRootFromHome(home) : "");
  if (!home || !sessionId) return null;
  return { home, sessionId, root };
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

function burnIncognitoHome() {
  const session = sessionFromEnv();
  const home = session?.root || session?.home || process.env.CODEX_HOME;
  if (!home) return;
  try {
    safeHome.burnSessionHome(home, {
      userRoot: USER_ROOT,
      sessionId: session?.sessionId || process.env.INCODEX_SESSION_ID,
    });
    logLaunch("burn", { home });
  } catch (error) {
    logLaunch("burn-refused", { error: String(error), home });
  }
}

function markSessionReady() {
  const session = sessionFromEnv();
  if (!session?.root) return;
  try {
    safeHome.writeReady(session.root);
  } catch {
    /* already written */
  }
}

function writePid() {
  safeHome.writePidFile(USER_ROOT, process.pid);
}

function clearPid() {
  try {
    safeHome.clearPidFile(USER_ROOT);
  } catch (error) {
    logLaunch("pid-clear-refused", { error: String(error) });
  }
}

function readIncognitoPid() {
  let pid = 0;
  try {
    pid = safeHome.readPidFile(USER_ROOT);
  } catch (error) {
    logLaunch("pid-read-refused", { error: String(error) });
    return 0;
  }
  if (!pid) return 0;
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
    safeHome.rotateAndAppendLog(
      USER_ROOT,
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
  const appTarget = targetId();
  try {
    safeHome.sweepOrphanSessions(USER_ROOT, { targetId: appTarget });
  } catch (error) {
    logLaunch("janitor-failed", { error: String(error) });
  }
  let session;
  try {
    session = safeHome.createSessionHome(USER_ROOT, { targetId: appTarget, pid: process.pid });
    safeHome.copySettings(session.home, DEFAULT_CODEX_HOME, USER_ROOT);
  } catch (error) {
    logLaunch("prepare-failed", { error: String(error) });
    return Promise.resolve({ ok: false, reason: "prepare-failed" });
  }
  const bin = process.execPath;
  if (!bin) {
    try {
      safeHome.burnSessionHome(session.root, { userRoot: USER_ROOT, sessionId: session.sessionId });
    } catch {
      /* ignore */
    }
    return Promise.resolve({ ok: false, reason: "spawn-failed" });
  }
  const args = [`--user-data-dir=${session.chromium}`];
  const sourceBounds = captureSourceBounds();
  logLaunch("launch", {
    bin,
    home: session.home,
    chromium: session.chromium,
    sessionId: session.sessionId,
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
          CODEX_HOME: session.home,
          INCODEX_INCOGNITO: "1",
          INCODEX_SESSION_ID: session.sessionId,
          INCODEX_SESSION_ROOT: session.root,
          CODEX_ELECTRON_USER_DATA_PATH: session.chromium,
          INCODEX_SOURCE_BOUNDS: sourceBounds,
        },
      });
    } catch (error) {
      logLaunch("spawn-threw", { error: String(error) });
      try {
        safeHome.burnSessionHome(session.root, { userRoot: USER_ROOT, sessionId: session.sessionId });
      } catch {
        /* ignore */
      }
      done({ ok: false, reason: "spawn-failed" });
      return;
    }
    if (!child.pid) {
      logLaunch("spawn-no-pid");
      try {
        safeHome.burnSessionHome(session.root, { userRoot: USER_ROOT, sessionId: session.sessionId });
      } catch {
        /* ignore */
      }
      done({ ok: false, reason: "spawn-failed" });
      return;
    }
    child.on("error", (error) => {
      logLaunch("spawn-error", { error: String(error) });
      done({ ok: false, reason: "spawn-failed" });
    });
    child.on("exit", (code) => {
      logLaunch("child-exit", { code, sessionId: session.sessionId });
      try {
        safeHome.burnSessionHome(session.root, { userRoot: USER_ROOT, sessionId: session.sessionId });
      } catch (error) {
        logLaunch("parent-burn-refused", { error: String(error) });
      }
      if (!settled) done({ ok: false, reason: "exited-early" });
    });
    raiseChildWhenReady(child.pid);
    const started = Date.now();
    const timer = setInterval(() => {
      if (settled) {
        clearInterval(timer);
        return;
      }
      if (safeHome.hasReady(session.root)) {
        clearInterval(timer);
        logLaunch("ready", { sessionId: session.sessionId, ms: Date.now() - started });
        done({ ok: true });
        return;
      }
      if (Date.now() - started > READY_TIMEOUT_MS) {
        clearInterval(timer);
        logLaunch("ready-timeout", { sessionId: session.sessionId });
        done({ ok: false, reason: "ready-timeout" });
      }
    }, 50);
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

  if (!isIncognito()) {
    try {
      safeHome.sweepOrphanSessions(USER_ROOT, { targetId: targetId() });
    } catch (error) {
      logLaunch("janitor-failed", { error: String(error) });
    }
  } else {
    process.env.INCODEX_INCOGNITO = "1";
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
    win.once("ready-to-show", () => {
      markSessionReady();
      bringForward();
    });
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
