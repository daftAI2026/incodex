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

function incognitoAlreadyRunning() {
  const file = path.join(INCOGNITO_HOME, PID_NAME);
  if (!fs.existsSync(file)) return false;
  const pid = Number(fs.readFileSync(file, "utf8").trim());
  if (!Number.isInteger(pid) || pid <= 0) return false;
  if (pid === process.pid) return true;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
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

function launchIncognito() {
  if (incognitoAlreadyRunning()) {
    logLaunch("already-running");
    return { ok: true, reason: "already-running" };
  }
  fs.mkdirSync(INCOGNITO_HOME, { recursive: true });
  fs.mkdirSync(INCOGNITO_CHROMIUM, { recursive: true });
  copySettings();
  const bin = process.execPath;
  const args = [`--user-data-dir=${INCOGNITO_CHROMIUM}`];
  logLaunch("launch", { bin, home: INCOGNITO_HOME, userData: INCOGNITO_CHROMIUM });
  const child = spawn(bin, args, {
    detached: true,
    stdio: "ignore",
    env: {
      ...process.env,
      CODEX_HOME: INCOGNITO_HOME,
      INCODEX_INCOGNITO: "1",
      CODEX_ELECTRON_USER_DATA_PATH: INCOGNITO_CHROMIUM,
    },
  });
  child.unref();
  return { ok: true };
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
  if (!win?.webContents) return;
  hookPreload(win.webContents.session);
  const run = () => {
    if (!source || win.webContents.isDestroyed()) return;
    const prefix = `window.__incodexIncognito=${isIncognito() ? "true" : "false"};`;
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
    hookWindow(win, source);
    if (!isIncognito()) return;
    win.on("close", () => {
      burnIncognitoHome();
      clearPid();
    });
  });
  if (isIncognito()) {
    writePid();
    electron.app.on("window-all-closed", () => {
      burnIncognitoHome();
      clearPid();
      electron.app.quit();
    });
    electron.app.on("before-quit", () => {
      burnIncognitoHome();
      clearPid();
    });
  }

  const ready = () => {
    hookPreload(electron.session.defaultSession);
    for (const win of electron.BrowserWindow.getAllWindows()) hookWindow(win, source);
  };
  if (electron.app.isReady()) ready();
  else void electron.app.whenReady().then(ready);
}

try {
  attachElectron();
} catch (error) {
  console.error("[incodex] main attach failed", error);
}
