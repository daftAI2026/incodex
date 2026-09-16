// @ts-nocheck
"use strict";

const { spawn } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const safeHome = require("./incodex-safe-home.cjs");
const ipcGuard = require("./incodex-ipc-guard.cjs");
const instance = require("./incodex-instance.cjs");
const windowKind = require("./incodex-window-kind.cjs");
const windowLifecycle = require("./incodex-window-lifecycle.cjs");
const codexMode = require("./incodex-codex-mode.cjs");
const dockMenu =
  process.platform === "darwin" ? require("./incodex-dock-menu.cjs") : null;
const windowsPlatform =
  process.platform === "win32" ? require("./incodex-windows-platform.cjs") : null;

const USER_ROOT = path.join(os.homedir(), ".incodex");
const DEFAULT_CODEX_HOME = path.join(os.homedir(), ".codex");
const DEFAULT_APP_PATH = "/Applications/ChatGPT.app";
const DEFAULT_APP_ASAR_PATH = path.join(DEFAULT_APP_PATH, "Contents", "Resources", "app.asar");
const DEFAULT_APP_EXECUTABLE_PATH = path.join(DEFAULT_APP_PATH, "Contents", "MacOS", "ChatGPT");
const ACCESSIBILITY_BUNDLE_ID = "com.openai.codex";
const ACCESSIBILITY_MARKER_NAME = "accessibility-setup.json";
const ACCESSIBILITY_MAX_MARKER_BYTES = 8 * 1024;
const ACCESSIBILITY_PACKAGE_MAX_BYTES = 256 * 1024;
const ACCESSIBILITY_RESET_TIMEOUT_MS = 5_000;
const ACCESSIBILITY_SETTINGS_URL =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
// The Runtime builder replaces this token with the small {en, zh-CN} table.
const ACCESSIBILITY_COPY = "__INCODEX_ACCESSIBILITY_COPY__";
const READY_TIMEOUT_MS = 15_000;
let capturedSourceHome = null;
const shownWindows = new WeakSet();

function targetId() {
  return instance.targetIdFromExec(process.execPath);
}

function stateRoot() {
  return instance.targetStateDir(USER_ROOT, process.execPath);
}

function resolvedCodexHome() {
  return safeHome.resolveSourceHome(process.env.CODEX_HOME, DEFAULT_CODEX_HOME);
}

function sourceHome() {
  if (capturedSourceHome) return capturedSourceHome;
  if (process.env.INCODEX_SOURCE_HOME) {
    return safeHome.resolveSourceHome(process.env.INCODEX_SOURCE_HOME, DEFAULT_CODEX_HOME);
  }
  return resolvedCodexHome();
}

function captureSourceHome() {
  if (isIncognito()) return;
  capturedSourceHome = resolvedCodexHome();
}

function isIncognito() {
  if (process.env.INCODEX_INCOGNITO === "1") return true;
  return safeHome.isManagedSessionHome(resolvedCodexHome(), USER_ROOT);
}

function captureChildOwnerSnapshot() {
  if (windowsPlatform) return null;
  if (!isIncognito() || typeof instance.processIdentity !== "function") return null;
  const live = instance.processIdentity(process.pid);
  if (!live?.processStartIdentity) return null;
  return Object.freeze({
    pid: process.pid,
    processStartIdentity: live.processStartIdentity,
  });
}

const childOwnerSnapshot = captureChildOwnerSnapshot();

function sessionFromEnv() {
  const home = process.env.CODEX_HOME;
  const sessionId = process.env.INCODEX_SESSION_ID;
  const root = process.env.INCODEX_SESSION_ROOT || (home ? safeHome.sessionRootFromHome(home) : "");
  if (!home || !sessionId) return null;
  const ino = Number(process.env.INCODEX_SESSION_INO);
  const dev = Number(process.env.INCODEX_SESSION_DEV);
  return {
    home,
    sessionId,
    root,
    ino: Number.isSafeInteger(ino) ? ino : null,
    dev: Number.isSafeInteger(dev) ? dev : null,
  };
}

function pickFile(name) {
  const { resolveRuntimeFile } = require("./incodex-runtime-load.cjs");
  return resolveRuntimeFile(name, __dirname);
}

function injectSource() {
  const file = pickFile("incodex-inject.js");
  if (!fs.existsSync(file)) return "";
  return fs.readFileSync(file, "utf8");
}

function readLocaleOverride() {
  const file = path.join(sourceHome(), "config.toml");
  if (!fs.existsSync(file)) return "";
  try {
    const content = fs.readFileSync(file, "utf8");
    const match = content.match(
      process.platform === "win32"
        ? /^\s*localeOverride\s*=\s*(?:"([^"]+)"|'([^']+)')/m
        : /^\s*localeOverride\s*=\s*"([^"]+)"/m,
    );
    return (match?.[1] ?? match?.[2] ?? "").trim();
  } catch {
    return "";
  }
}

function accessibilityInstallIdIsSafe(installId) {
  return (
    typeof installId === "string" &&
    installId.length > 0 &&
    installId.length <= 128 &&
    /^[A-Za-z0-9._-]+$/.test(installId)
  );
}

function accessibilityPathIsDefaultApp(appPath) {
  return typeof appPath === "string" && path.resolve(appPath) === DEFAULT_APP_PATH;
}

function accessibilityCurrentUid() {
  return typeof process.getuid === "function" ? process.getuid() : null;
}

function accessibilityStatIsSafe(stat, directory) {
  if (!stat || stat.isSymbolicLink() || (directory ? !stat.isDirectory() : !stat.isFile())) {
    return false;
  }
  const uid = accessibilityCurrentUid();
  if (uid !== null && stat.uid !== uid) return false;
  return (stat.mode & 0o022) === 0;
}

function accessibilityMarkerLayout(fileSystem, requestPath, installId) {
  if (!accessibilityInstallIdIsSafe(installId) || typeof requestPath !== "string") {
    return { kind: "unsafe" };
  }
  let absolute;
  try {
    absolute = path.resolve(requestPath);
  } catch {
    return { kind: "unsafe" };
  }
  const installDir = path.dirname(absolute);
  const transactionsDir = path.dirname(installDir);
  const root = path.dirname(transactionsDir);
  if (
    path.basename(absolute) !== ACCESSIBILITY_MARKER_NAME ||
    path.basename(installDir) !== installId ||
    path.basename(transactionsDir) !== "transactions" ||
    !root ||
    root === transactionsDir
  ) {
    return { kind: "unsafe" };
  }
  for (const directory of [root, transactionsDir, installDir]) {
    let stat;
    try {
      stat = fileSystem.lstatSync(directory);
    } catch (error) {
      if (error?.code === "ENOENT") return { kind: "missing" };
      return { kind: "unsafe" };
    }
    if (!accessibilityStatIsSafe(stat, true)) return { kind: "unsafe" };
  }
  let markerStat;
  try {
    markerStat = fileSystem.lstatSync(absolute);
  } catch (error) {
    if (error?.code === "ENOENT") return { kind: "missing" };
    return { kind: "unsafe" };
  }
  if (!accessibilityStatIsSafe(markerStat, false)) return { kind: "unsafe" };
  if (markerStat.size > ACCESSIBILITY_MAX_MARKER_BYTES) return { kind: "unsafe" };
  return { kind: "ok", absolute, root, transactionsDir, installDir, markerStat };
}

function accessibilityMarkerIsValid(marker, appPath, installId) {
  if (!marker || typeof marker !== "object") return false;
  if (marker.schemaVersion !== 1 || marker.installId !== installId) return false;
  if (!accessibilityPathIsDefaultApp(marker.appPath) || marker.appPath !== appPath) return false;
  if (!Number.isSafeInteger(marker.requestedAtMs) || marker.requestedAtMs <= 0) return false;
  if (
    marker.requestId !== undefined &&
    (typeof marker.requestId !== "string" || marker.requestId.length === 0 || marker.requestId.length > 100)
  ) {
    return false;
  }
  return ["pending", "granted", "deferred", "error", "awaiting-user"].includes(marker.state);
}

function readAccessibilityMarker(fileSystem, requestPath, appPath, installId) {
  const layout = accessibilityMarkerLayout(fileSystem, requestPath, installId);
  if (layout.kind !== "ok") return { kind: layout.kind };
  let marker;
  try {
    marker = JSON.parse(fileSystem.readFileSync(layout.absolute, "utf8"));
  } catch {
    return { kind: "unsafe" };
  }
  if (!accessibilityMarkerIsValid(marker, appPath, installId)) return { kind: "unsafe" };
  return { kind: "ok", marker, layout };
}

function accessibilityRequestMatches(current, snapshot) {
  return (
    current?.kind === "ok" &&
    current.marker.installId === snapshot.installId &&
    current.marker.appPath === snapshot.appPath &&
    current.marker.requestedAtMs === snapshot.requestedAtMs &&
    current.marker.requestId === snapshot.requestId
  );
}

function writeAccessibilityMarkerState(
  fileSystem,
  requestPath,
  appPath,
  installId,
  expectedRequestedAtMs,
  state,
  now,
  extra = {},
  expectedRequestId,
) {
  const current = readAccessibilityMarker(fileSystem, requestPath, appPath, installId);
  if (
    current.kind !== "ok" ||
    current.marker.requestedAtMs !== expectedRequestedAtMs ||
    current.marker.requestId !== expectedRequestId ||
    !["granted", "deferred", "error", "awaiting-user"].includes(state)
  ) {
    return false;
  }
  const next = {
    ...current.marker,
    ...extra,
    state,
    updatedAtMs: now(),
  };
  if (!Number.isSafeInteger(next.updatedAtMs)) next.updatedAtMs = Date.now();
  if (state !== "error") delete next.error;
  const temporary = path.join(
    current.layout.installDir,
    `.accessibility-setup.${process.pid}.${Date.now()}.tmp`,
  );
  let descriptor = null;
  try {
    descriptor = fileSystem.openSync(temporary, "wx", 0o600);
    fileSystem.writeFileSync(descriptor, `${JSON.stringify(next)}\n`, "utf8");
    if (typeof fileSystem.fsyncSync === "function") fileSystem.fsyncSync(descriptor);
    fileSystem.closeSync(descriptor);
    descriptor = null;
    const latest = readAccessibilityMarker(fileSystem, requestPath, appPath, installId);
    if (
      !accessibilityRequestMatches(latest, {
        installId,
        appPath,
        requestedAtMs: expectedRequestedAtMs,
        requestId: expectedRequestId,
      }) ||
      latest.marker.state !== current.marker.state
    ) {
      return false;
    }
    fileSystem.renameSync(temporary, requestPath);
    return true;
  } catch {
    return false;
  } finally {
    if (descriptor !== null) {
      try {
        fileSystem.closeSync(descriptor);
      } catch {
        /* best effort */
      }
    }
    try {
      fileSystem.unlinkSync(temporary);
    } catch {
      /* best effort */
    }
  }
}

function readInstalledRuntimeIdentity(app, fileSystem = fs) {
  if (!app || typeof app.getAppPath !== "function") return null;
  let appPath;
  try {
    appPath = path.resolve(app.getAppPath());
  } catch {
    return null;
  }
  if (appPath !== DEFAULT_APP_ASAR_PATH) return null;
  const bundlePath = DEFAULT_APP_PATH;
  const packagePath = path.join(appPath, "package.json");
  let stat;
  try {
    stat = fileSystem.lstatSync(packagePath);
    if (stat.isSymbolicLink() || !stat.isFile() || stat.size > ACCESSIBILITY_PACKAGE_MAX_BYTES) {
      return null;
    }
    const packageJson = JSON.parse(fileSystem.readFileSync(packagePath, "utf8"));
    const installId = packageJson?.__incodex?.installId;
    if (!accessibilityInstallIdIsSafe(installId)) return null;
    return { appPath: bundlePath, installId };
  } catch {
    return null;
  }
}

function accessibilityCopyValue(copy, key) {
  const value = typeof copy === "function" ? copy(key) : copy?.[key];
  return typeof value === "string" ? value : "";
}

function resolveAccessibilityCopy(locale = "en") {
  const source = ACCESSIBILITY_COPY && typeof ACCESSIBILITY_COPY === "object" ? ACCESSIBILITY_COPY : null;
  if (!source) return null;
  const language = String(locale || "").toLowerCase().startsWith("zh") ? "zh-CN" : "en";
  const selected = source[language] || source.en;
  return selected && typeof selected === "object" ? { ...selected } : null;
}

function createAccessibilitySetupController(options = {}) {
  const fileSystem = options.fs || fs;
  const shell = options.shell || null;
  const dialog = options.dialog || null;
  const systemPreferences = options.systemPreferences || null;
  const spawnCommand = options.spawn || spawn;
  const requestPath = options.requestPath;
  const appPath = options.appPath;
  const installId = options.installId;
  const platform = options.platform || process.platform;
  const copy = options.copy;
  const now = typeof options.now === "function" ? options.now : () => Date.now();
  let flight = null;

  function transition(snapshot, state, extra = {}) {
    return writeAccessibilityMarkerState(
      fileSystem,
      requestPath,
      appPath,
      installId,
      snapshot.requestedAtMs,
      state,
      now,
      extra,
      snapshot.requestId,
    );
  }

  function inIncognito() {
    return typeof options.isIncognito === "function" ? options.isIncognito() : options.isIncognito === true;
  }

  function probe() {
    if (platform !== "darwin" || !systemPreferences) return { kind: "unknown" };
    if (typeof systemPreferences.isTrustedAccessibilityClient !== "function") {
      return { kind: "unknown" };
    }
    try {
      // false is intentional: startup never asks macOS to prompt on its own.
      const trusted = systemPreferences.isTrustedAccessibilityClient(false);
      if (typeof trusted !== "boolean") {
        return { kind: "unknown", error: "Accessibility probe returned a non-boolean result" };
      }
      return {
        kind: "known",
        trusted,
      };
    } catch (error) {
      return { kind: "unknown", error: String(error) };
    }
  }

  function showRepairError(error) {
    const title = accessibilityCopyValue(copy, "errorTitle");
    const body = accessibilityCopyValue(copy, "errorBody");
    if (title && body && typeof dialog?.showErrorBox === "function") dialog.showErrorBox(title, body);
    try {
      logLaunch("accessibility-repair-failed", { error: String(error) });
    } catch {
      /* Logging is best effort. */
    }
  }

  function resetAccessibility() {
    return new Promise((resolve) => {
      let child;
      let settled = false;
      let timer = null;
      const done = (result) => {
        if (settled) return;
        settled = true;
        if (timer) clearTimeout(timer);
        resolve(result);
      };
      timer = setTimeout(() => done({ ok: false, error: "tccutil timed out" }), ACCESSIBILITY_RESET_TIMEOUT_MS);
      timer.unref?.();
      try {
        child = spawnCommand(
          "/usr/bin/tccutil",
          ["reset", "Accessibility", ACCESSIBILITY_BUNDLE_ID],
          { stdio: "ignore" },
        );
      } catch (error) {
        done({ ok: false, error: String(error) });
        return;
      }
      if (!child || typeof child.once !== "function") {
        done({ ok: false, error: "tccutil did not start" });
        return;
      }
      child.once("error", (error) => done({ ok: false, error: String(error) }));
      child.once("close", (code) =>
        done(code === 0 ? { ok: true } : { ok: false, error: `tccutil exited ${String(code)}` }),
      );
    });
  }

  async function openAccessibilitySurfaces() {
    if ((await shell.openExternal(ACCESSIBILITY_SETTINGS_URL)) === false) {
      throw new Error("could not open Accessibility settings");
    }
    shell.showItemInFolder(appPath);
  }

  async function showAwaitingUserPrompt(snapshot) {
    const localized = {
      title: accessibilityCopyValue(copy, "addedTitle"),
      message: accessibilityCopyValue(copy, "addedBody"),
      checkAgain: accessibilityCopyValue(copy, "checkAgain"),
      later: accessibilityCopyValue(copy, "later"),
    };
    if (Object.values(localized).some((value) => !value) || typeof dialog?.showMessageBox !== "function") {
      return false;
    }
    const choice = await dialog.showMessageBox({
      type: "info",
      title: localized.title,
      message: localized.message,
      buttons: [localized.checkAgain, localized.later],
      defaultId: 0,
      cancelId: 1,
      noLink: true,
    });
    if (choice?.response !== 0) return false;
    const current = readAccessibilityMarker(fileSystem, requestPath, appPath, installId);
    if (!accessibilityRequestMatches(current, snapshot) || current.marker.state !== "awaiting-user") {
      return false;
    }
    const checked = probe();
    if (checked.kind !== "known" || !checked.trusted) return false;
    return transition(snapshot, "granted");
  }

  async function processRequest() {
    if (
      platform !== "darwin" ||
      inIncognito() ||
      !accessibilityPathIsDefaultApp(appPath) ||
      !accessibilityInstallIdIsSafe(installId) ||
      !copy
    ) {
      return { ok: false, state: "unknown", reason: "unsupported-host" };
    }
    const loaded = readAccessibilityMarker(fileSystem, requestPath, appPath, installId);
    if (loaded.kind !== "ok") return { ok: false, state: loaded.kind };
    const marker = loaded.marker;
    if (marker.state === "granted" || marker.state === "deferred" || marker.state === "error") {
      return { ok: true, state: marker.state };
    }
    const snapshot = {
      installId: marker.installId,
      appPath: marker.appPath,
      requestedAtMs: marker.requestedAtMs,
      requestId: marker.requestId,
    };
    const initial = probe();
    if (initial.kind !== "known") return { ok: false, state: "unknown", reason: initial.error };
    if (initial.trusted) {
      const written = transition(snapshot, "granted");
      return { ok: written, state: written ? "granted" : "stale" };
    }
    if (marker.state === "awaiting-user") {
      return { ok: true, state: "awaiting-user" };
    }
    if (typeof dialog?.showMessageBox !== "function") {
      return { ok: false, state: "unknown", reason: "dialog-unavailable" };
    }
    const localized = {
      title: accessibilityCopyValue(copy, "title"),
      message: accessibilityCopyValue(copy, "body"),
      repair: accessibilityCopyValue(copy, "repair"),
      later: accessibilityCopyValue(copy, "later"),
    };
    if (Object.values(localized).some((value) => !value)) {
      return { ok: false, state: "unknown", reason: "accessibility-copy-unavailable" };
    }
    let choice;
    try {
      choice = await dialog.showMessageBox({
        type: "warning",
        title: localized.title,
        message: localized.message,
        buttons: [localized.repair, localized.later],
        defaultId: 0,
        cancelId: 1,
        noLink: true,
      });
    } catch (error) {
      const written = transition(snapshot, "error", { error: String(error) });
      if (written) showRepairError(error);
      return { ok: false, state: written ? "error" : "stale" };
    }
    const currentAfterDialog = readAccessibilityMarker(fileSystem, requestPath, appPath, installId);
    if (!accessibilityRequestMatches(currentAfterDialog, snapshot) || currentAfterDialog.marker.state !== "pending") {
      return { ok: false, state: "stale" };
    }
    if (choice?.response !== 0) {
      const written = transition(snapshot, "deferred");
      return { ok: written, state: written ? "deferred" : "stale" };
    }
    const beforeReset = probe();
    if (beforeReset.kind !== "known") {
      return { ok: false, state: "unknown", reason: beforeReset.error };
    }
    if (beforeReset.trusted) {
      const written = transition(snapshot, "granted");
      return { ok: written, state: written ? "granted" : "stale" };
    }

    // Record the user-driven attempt before running a process that may be
    // interrupted.  A crash after tccutil must never turn into an automatic
    // second reset on the next launch.
    const awaiting = transition(snapshot, "awaiting-user");
    if (!awaiting) return { ok: false, state: "stale" };

    const reset = await resetAccessibility();
    if (!reset.ok) {
      const written = transition(snapshot, "error", { error: reset.error });
      if (written) showRepairError(reset.error);
      return { ok: false, state: written ? "error" : "stale" };
    }
    try {
      await openAccessibilitySurfaces();
    } catch (error) {
      const written = transition(snapshot, "error", { error: String(error) });
      if (written) showRepairError(error);
      return { ok: false, state: written ? "error" : "stale" };
    }
    const checked = await showAwaitingUserPrompt(snapshot);
    return { ok: true, state: checked ? "granted" : "awaiting-user" };
  }

  function run() {
    if (flight) return flight;
    flight = Promise.resolve()
      .then(processRequest)
      .catch((error) => {
        try {
          logLaunch("accessibility-setup-failed", { error: String(error) });
        } catch {
          /* best effort */
        }
        return { ok: false, state: "error", reason: String(error) };
      })
      .finally(() => {
        flight = null;
      });
    return flight;
  }

  return { run };
}

function sessionBurnExpectation(session, userRoot = USER_ROOT) {
  return {
    userRoot,
    sessionId: session.sessionId,
    ino: session.ino,
    dev: session.dev,
  };
}

function burnIncognitoSession(session, ownerSnapshot, userRoot = USER_ROOT) {
  if (
    !session ||
    !ownerSnapshot ||
    !Number.isInteger(ownerSnapshot.pid) ||
    ownerSnapshot.pid <= 0 ||
    typeof ownerSnapshot.processStartIdentity !== "string" ||
    !ownerSnapshot.processStartIdentity
  ) {
    return false;
  }
  const expected = sessionBurnExpectation(session, userRoot);
  const removed = safeHome.burnSessionHomeWithOwner(session.root, expected, ownerSnapshot);
  if (removed && !safeHome.writeBurnProof(session.root, expected)) {
    logLaunch("burn-proof-write-failed", { home: session.root });
  }
  return removed;
}

function burnIncognitoHome() {
  if (process.env.INCODEX_CLEANUP_OWNER === "native") return;
  const session = sessionFromEnv();
  const home = session?.root || session?.home || process.env.CODEX_HOME;
  if (!home) return;
  if (!session || session.ino == null || session.dev == null) {
    logLaunch("burn-refused", { home, reason: "session identity is unavailable" });
    return;
  }
  if (!childOwnerSnapshot) {
    logLaunch("burn-refused", { home, reason: "process identity is unavailable" });
    return;
  }
  try {
    burnIncognitoSession(session, childOwnerSnapshot);
    logLaunch("burn", { home });
  } catch (error) {
    logLaunch("burn-refused", { error: String(error), home });
  }
}

function cleanupExitedSession(session, childOwner, options = {}) {
  return safeHome.cleanupExitedSession(session, childOwner, {
    userRoot: USER_ROOT,
    quiesceSessionHelpers: instance.quiesceSessionHelpers,
    log: logLaunch,
    ...options,
  });
}

function markSessionReady() {
  if (windowsPlatform) {
    const marked = windowsPlatform.markReady(process.env.INCODEX_WINDOWS_READY_PIPE || "");
    if (!marked) {
      logLaunch("ready-refused", { reason: "guardian pipe unavailable" });
    }
    return marked;
  }
  const session = sessionFromEnv();
  if (!session?.root) return false;
  try {
    safeHome.writeReady(session.root);
    return true;
  } catch {
    /* already written */
    return false;
  }
}

async function writePid() {
  try {
    return await instance.acquireOwnerLease(
      stateRoot(),
      instance.currentOwner(process.env.INCODEX_SESSION_ID, process.execPath),
    );
  } catch (error) {
    logLaunch("lock-refused", { error: String(error) });
    return null;
  }
}

function startupBlocked(error) {
  error.code = "INCODEX_STARTUP_BLOCKED";
  return error;
}

async function clearPid(lease, server) {
  if (lease && instance.releaseOwnerLease) {
    if (!(await instance.releaseOwnerLease(stateRoot(), lease))) {
      logLaunch("lock-clear-refused", { sessionId: lease.sessionId });
    }
    return;
  }
  try {
    if (server?.listening) server.close();
  } catch {
    /* Server shutdown is best effort when no managed lease remains. */
  }
}

async function incognitoAlreadyRunning() {
  const records = instance.readOwnerRecords(stateRoot());
  if (records.some(({ state }) => state.kind === "unverifiable")) {
    throw new Error("owner lease is unverifiable");
  }
  const owners = records
    .filter(({ state }) => state.kind === "valid")
    .map(({ state }) => state.owner);
  for (const owner of owners) {
    if (await instance.connectExistingWithRetry(stateRoot(), instance.ownerToken(owner))) return true;
  }
  if (owners.length === 0 || owners.every((owner) => instance.staleOwnerRecord(owner))) return false;
  throw new Error("owner lease is active but its raise socket is unavailable");
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
    const bounds = typeof win.getBounds === "function" ? win.getBounds() : {};
    const url = win.webContents && !win.webContents.isDestroyed() ? win.webContents.getURL() : "";
    return windowKind.isAuxiliarySnapshot({
      alwaysOnTop: typeof win.isAlwaysOnTop === "function" && win.isAlwaysOnTop(),
      focusable: typeof win.isFocusable !== "function" || win.isFocusable(),
      width: bounds.width,
      height: bounds.height,
      url,
      hasParent: typeof win.getParentWindow === "function" && Boolean(win.getParentWindow()),
    });
  } catch {
    return false;
  }
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
      // The host owns initial visibility. A ready hidden window may be a
      // prewarmed surface, not a user request to open another chat window.
      if (win.isVisible() || win.isMinimized()) shownWindows.add(win);
      if (!shownWindows.has(win)) continue;
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

async function raiseExistingIncognito() {
  const ok = await instance.connectExisting(stateRoot());
  logLaunch("raise-existing", { ok });
  return ok;
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
      stateRoot(),
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

function captureSourceBounds(sourceWindow) {
  try {
    const electron = require("electron");
    const focused = electron.BrowserWindow.getFocusedWindow();
    const usable = (win) => win && !win.isDestroyed() && !isAuxiliaryWindow(win);
    const visible = (win) => usable(win) && (win.isVisible() || win.isMinimized());
    // Renderer actions already passed the IPC identity check. Their actual
    // window remains the source even when an AX click does not focus it.
    const win = usable(sourceWindow) ? sourceWindow
      : visible(focused) ? focused : mainWindows(electron).find(visible);
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

const launchHolder = { current: null };

function launchIncognito(sourceWindow) {
  const sourceBounds = captureSourceBounds(sourceWindow);
  const launch = windowsPlatform
    ? () =>
        windowsPlatform.launchIncognito({
          helperPath: process.env.INCODEX_WINDOWS_HELPER,
          sourceHome: sourceHome(),
          sourceBounds,
        })
    : () => launchIncognitoOnce(sourceBounds);
  return instance.singleFlight(launchHolder, launch);
}

function runtimeOwnedSessionEnv(session, sourceBounds) {
  return {
    ...process.env,
    CODEX_HOME: session.home,
    INCODEX_INCOGNITO: "1",
    INCODEX_CLEANUP_OWNER: "runtime",
    INCODEX_SESSION_ID: session.sessionId,
    INCODEX_SESSION_ROOT: session.root,
    INCODEX_SESSION_INO: String(session.ino),
    INCODEX_SESSION_DEV: String(session.dev),
    CODEX_ELECTRON_USER_DATA_PATH: session.chromium,
    INCODEX_SOURCE_BOUNDS: sourceBounds,
    INCODEX_SOURCE_HOME: sourceHome(),
  };
}

function prepareIncognitoSession(options = {}) {
  const {
    userRoot = USER_ROOT,
    sourceHomePath = sourceHome(),
    appTarget = targetId(),
    pid = process.pid,
    createSessionHome = safeHome.createSessionHome,
    copySettings = safeHome.copySettings,
    burnSessionHome = safeHome.burnSessionHome,
    log = logLaunch,
  } = options;
  let session;
  try {
    session = createSessionHome(userRoot, {
      targetId: appTarget,
      pid,
      sourceHome: sourceHomePath,
      handoffPending: true,
    });
    copySettings(session.home, sourceHomePath);
    return { ok: true, session };
  } catch (error) {
    if (session) {
      try {
        burnSessionHome(session.root, sessionBurnExpectation(session, userRoot));
      } catch (cleanupError) {
        try {
          log("prepare-burn-refused", {
            error: String(cleanupError),
            sessionId: session.sessionId,
          });
        } catch {
          /* Cleanup logging must not replace the preparation failure. */
        }
      }
    }
    try {
      log("prepare-failed", { error: String(error) });
    } catch {
      /* Logging is best effort; the caller still receives prepare-failed. */
    }
    return { ok: false, reason: "prepare-failed" };
  }
}

async function launchIncognitoOnce(sourceBounds) {
  let alreadyRunning;
  try {
    alreadyRunning = await incognitoAlreadyRunning();
  } catch (error) {
    logLaunch("owner-unavailable", { error: String(error) });
    return { ok: false, reason: "owner-unavailable" };
  }
  if (alreadyRunning) {
    await raiseExistingIncognito();
    return { ok: true, reason: "already-running" };
  }
  const appTarget = targetId();
  try {
    safeHome.sweepOrphanSessions(USER_ROOT, { targetId: appTarget });
  } catch (error) {
    logLaunch("janitor-failed", { error: String(error) });
  }
  const prepared = prepareIncognitoSession();
  if (!prepared.ok) return Promise.resolve(prepared);
  const { session } = prepared;
  const bin = process.execPath;
  if (!bin) {
    try {
      safeHome.burnSessionHome(session.root, sessionBurnExpectation(session));
    } catch {
      /* ignore */
    }
    return Promise.resolve({ ok: false, reason: "spawn-failed" });
  }
  const args = [`--user-data-dir=${session.chromium}`, "codex://new?mode=codex"];
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
    function done(result) {
      if (settled) return;
      settled = true;
      resolve(result);
    }
    let child;
    try {
      child = spawn(bin, args, {
        detached: true,
        stdio: "ignore",
        env: runtimeOwnedSessionEnv(session, sourceBounds),
      });
    } catch (error) {
      logLaunch("spawn-threw", { error: String(error) });
      try {
        safeHome.burnSessionHome(session.root, sessionBurnExpectation(session));
      } catch {
        /* ignore */
      }
      done({ ok: false, reason: "spawn-failed" });
      return;
    }
    if (!child.pid) {
      logLaunch("spawn-no-pid");
      try {
        safeHome.burnSessionHome(session.root, sessionBurnExpectation(session));
      } catch {
        /* ignore */
      }
      done({ ok: false, reason: "spawn-failed" });
      return;
    }
    let childOwner = null;
    child.on("error", (error) => {
      logLaunch("spawn-error", { error: String(error) });
      done({ ok: false, reason: "spawn-failed" });
    });
    child.on("exit", (code) => {
      logLaunch("child-exit", { code, sessionId: session.sessionId });
      // This exact child exit observation is the only caller allowed to finish
      // a partial child burn after helper quiescence.
      void cleanupExitedSession(session, childOwner);
      if (!settled) done({ ok: false, reason: "exited-early" });
    });
    try {
      childOwner = safeHome.handoffSessionOwner(session.root, child.pid);
    } catch (error) {
      logLaunch("owner-handoff-failed", { error: String(error), sessionId: session.sessionId });
      try {
        if (!child.killed) child.kill();
      } catch {
        /* child exit handler still owns bounded cleanup. */
      }
      done({ ok: false, reason: "spawn-failed" });
      return;
    }
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

const allowedWindows = new Map();
const trustedOrigins = new Set(["app://-", "https://chatgpt.com"]);
const acceptedWindows = new WeakSet();
const readyWindows = new WeakSet();

function rememberWindow(win) {
  if (!win || typeof win.id !== "number") return;
  ipcGuard.bindWindowIdentity(allowedWindows, win, trustedOrigins);
  win.once("closed", () => allowedWindows.delete(win.id));
}

function authorizeEvent(event) {
  return ipcGuard.authorizeSender(ipcGuard.snapshotFromEvent(event), allowedWindows);
}

function hookPreload(session) {
  if (!session || session.__incodexPreload) return;
  session.__incodexPreload = true;
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

function reportInjectionError(error) {
  logLaunch("ui-injection-failed", { error: String(error) });
}

function markAcceptedWindowReady(win) {
  if (!windowsPlatform || !isIncognito() || readyWindows.has(win)) return;
  if (!acceptedWindows.has(win) || win.isDestroyed() || !win.isVisible()) return;
  if (markSessionReady()) readyWindows.add(win);
}

function reportInjectionProbe(win, reportMissing = true) {
  return win.webContents.executeJavaScript("window.__incodexUiProbe", false).then((probe) => {
    if (reportMissing || probe?.accepted === true) logLaunch("ui-probe", probe);
    if (windowsPlatform && isIncognito() && probe?.accepted === true) {
      acceptedWindows.add(win);
      markAcceptedWindowReady(win);
    }
    return probe;
  });
}

function markSessionClosed() {
  if (!windowsPlatform) return;
  if (!windowsPlatform.markClosed(process.env.INCODEX_WINDOWS_CLOSE_PIPE || "")) {
    logLaunch("close-refused", { reason: "guardian pipe unavailable" });
  }
}

function selectOfficialCodexModeFallback(win) {
  if (!isIncognito()) return;
  try {
    win.webContents.sendInputEvent({ type: "keyDown", keyCode: "3", modifiers: ["control"] });
    win.webContents.sendInputEvent({ type: "keyUp", keyCode: "3", modifiers: ["control"] });
    return true;
  } catch (error) {
    logLaunch("codex-mode-selection-failed", { error: String(error) });
    return false;
  }
}

const codexModeReadiness = codexMode.createCodexModeReadiness({
  isIncognito,
  log: logLaunch,
  selectFallback: selectOfficialCodexModeFallback,
});

function hookWindow(win, source) {
  if (!win?.webContents || isAuxiliaryWindow(win)) return;
  rememberWindow(win);
  hookPreload(win.webContents.session);
  function run(report) {
    if (!source || win.webContents.isDestroyed()) return;
    if (!ipcGuard.bindWindowIdentity(allowedWindows, win, trustedOrigins)) return;
    const locale = JSON.stringify(readLocaleOverride());
    const platform = JSON.stringify(process.platform);
    const prefix = `window.__incodexIncognito=${isIncognito() ? "true" : "false"};window.__incodexLocale=${locale};window.__incodexPlatform=${platform};`;
    win.webContents
      .executeJavaScript(prefix + source, false)
      .then(() => {
        codexModeReadiness.observe(win);
        return report ? reportInjectionProbe(win) : undefined;
      })
      .catch((error) => reportInjectionError(error));
  }
  win.webContents.on("dom-ready", () => run(false));
  win.webContents.on("did-finish-load", () => run(true));
  run(false);
  if (windowsPlatform && isIncognito()) {
    windowsPlatform.observeRuntimeUiReadiness(
      win,
      () => reportInjectionProbe(win, false).then((probe) => probe?.accepted === true),
      () => markAcceptedWindowReady(win),
    );
  }
}

async function attachElectron() {
  let electron;
  try {
    electron = require("electron");
  } catch {
    return;
  }
  function launchFromNativeMenu(source) {
    void launchIncognito()
      .then((result) => {
        if (!result.ok) logLaunch(`${source}-open-failed`, { reason: result.reason });
      })
      .catch((error) => logLaunch(`${source}-open-failed`, { error: String(error) }));
  }
  const dockMenuController = dockMenu?.createDockMenuController({
    dock: electron.app.dock,
    Menu: electron.Menu,
    MenuItem: electron.MenuItem,
    isIncognito: isIncognito(),
    onOpen: () => launchFromNativeMenu("dock-menu"),
    log: logLaunch,
  });
  const statusMenuController = dockMenu && dockMenu.createStatusMenuController({
    loadBridge: () =>
      dockMenu.createNativeStatusMenuBridge({
        appPath: electron.app.getAppPath(),
        onError: (error) => logLaunch("status-menu-action-failed", { error: String(error) }),
      }),
    isIncognito: isIncognito(),
    onOpen: () => launchFromNativeMenu("status-menu"),
    log: logLaunch,
  });
  electron.app.once("will-quit", () => statusMenuController?.dispose());
  const packagedOrigin = ipcGuard.navigationOrigin(
    require("node:url").pathToFileURL(electron.app.getAppPath()).href,
  );
  if (packagedOrigin) trustedOrigins.add(packagedOrigin);
  captureSourceHome();

  if (!isIncognito()) {
    if (!windowsPlatform) {
      try {
        safeHome.sweepOrphanSessions(USER_ROOT, { targetId: targetId() });
      } catch (error) {
        logLaunch("janitor-failed", { error: String(error) });
      }
    }
  } else {
    process.env.INCODEX_INCOGNITO = "1";
  }

  let accessibilitySetupController = null;
  if (
    !isIncognito() &&
    process.platform === "darwin" &&
    path.resolve(process.execPath || "") === DEFAULT_APP_EXECUTABLE_PATH
  ) {
    const identity = readInstalledRuntimeIdentity(electron.app);
    if (identity?.appPath === DEFAULT_APP_PATH) {
      accessibilitySetupController = createAccessibilitySetupController({
        app: electron.app,
        shell: electron.shell,
        dialog: electron.dialog,
        systemPreferences: electron.systemPreferences,
        spawn,
        fs,
        requestPath: path.join(
          USER_ROOT,
          "transactions",
          identity.installId,
          ACCESSIBILITY_MARKER_NAME,
        ),
        appPath: identity.appPath,
        installId: identity.installId,
        bundleId: ACCESSIBILITY_BUNDLE_ID,
        platform: process.platform,
        isIncognito,
        copy: resolveAccessibilityCopy(
          readLocaleOverride() || electron.app.getLocale?.() || "en",
        ),
        now: () => Date.now(),
      });
    }
  }

  const source = injectSource();
  let ownerLease = null;
  let raiseServer = null;
  let incognitoExitStarted = false;
  function finishIncognito(code) {
    if (incognitoExitStarted) return;
    incognitoExitStarted = true;
    markSessionClosed();
    burnIncognitoHome();
    void clearPid(ownerLease, raiseServer);
    electron.app.exit(code);
  }
  const incognitoWindowLifecycle = isIncognito()
    ? windowLifecycle.createIncognitoWindowLifecycle(finishIncognito)
    : null;
  electron.ipcMain.handle("incodex-action", async (event, payload) => {
    const requestId = typeof payload?.requestId === "string" ? payload.requestId : "";
    const gate = authorizeEvent(event);
    if (!gate.ok) return ipcGuard.actionResponse(requestId, gate);
    const action = payload?.action;
    if (action === "configure-dock-menu") {
      const configured =
        dockMenuController && dockMenuController.configure(payload?.label) === true;
      return ipcGuard.actionResponse(requestId, {
        ok: configured,
        code: configured ? "OK" : "UNAVAILABLE",
      });
    }
    if (action === "configure-status-menu") {
      const configured =
        statusMenuController && (await statusMenuController.configure(payload?.label)) === true;
      return ipcGuard.actionResponse(requestId, {
        ok: configured,
        code: configured ? "OK" : "UNAVAILABLE",
      });
    }
    if (action === "open") {
      if (isIncognito()) {
        return ipcGuard.actionResponse(requestId, {
          ok: false,
          code: "ALREADY_INCOGNITO",
          reason: "already-incognito",
        });
      }
      const sourceWindow = electron.BrowserWindow.fromWebContents(event.sender);
      const result = await launchIncognito(sourceWindow);
      return ipcGuard.actionResponse(requestId, {
        ok: result.ok === true,
        code: result.ok ? "OK" : String(result.reason || "FAILED").toUpperCase(),
        reason: result.reason,
      });
    }
    if (action === "quit") {
      if (!isIncognito()) {
        return ipcGuard.actionResponse(requestId, {
          ok: false,
          code: "NOT_INCOGNITO",
          reason: "not-incognito",
        });
      }
      electron.app.quit();
      return ipcGuard.actionResponse(requestId, { ok: true, code: "OK" });
    }
    return ipcGuard.actionResponse(requestId, { ok: false, code: "UNKNOWN_ACTION" });
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
    incognitoWindowLifecycle?.observe(win);
    if (!isIncognito()) return;
    function bringForward() {
      if (win.isDestroyed() || (!win.isVisible() && !win.isMinimized())) return false;
      applyChromeWindowTile(win);
      raiseOurWindows();
      return true;
    }
    win.once("ready-to-show", () => {
      if (!bringForward()) return;
      if (!windowsPlatform) markSessionReady();
      else markAcceptedWindowReady(win);
    });
    win.once("show", () => {
      shownWindows.add(win);
      bringForward();
      if (!windowsPlatform) markSessionReady();
      markAcceptedWindowReady(win);
      setTimeout(bringForward, 50);
      setTimeout(bringForward, 300);
    });
  });
  if (isIncognito() && windowsPlatform) {
    try {
      raiseServer = windowsPlatform.listenForRaise(
        process.env.INCODEX_WINDOWS_RAISE_PIPE || "",
        () => raiseOurWindows(),
      );
      raiseServer.once("error", (error) => {
        logLaunch("raise-pipe-failed", { error: String(error) });
        void clearPid(ownerLease, raiseServer);
        electron.app.exit(1);
      });
    } catch (error) {
      logLaunch("raise-pipe-failed", { error: String(error) });
      electron.app.exit(1);
      throw startupBlocked(error instanceof Error ? error : new Error(String(error)));
    }
  }
  if (isIncognito() && !windowsPlatform) {
    ownerLease = await writePid();
    if (!ownerLease) {
      try {
        electron.app.exit(1);
      } catch {
        /* Electron may not be ready yet; returning still prevents a second owner. */
      }
      throw startupBlocked(new Error("[incodex] owner lease refused"));
    }
    try {
      raiseServer = instance.listenForRaise(stateRoot(), () => raiseOurWindows(), ownerLease);
      raiseServer.once("error", (error) => {
        logLaunch("raise-socket-failed", { error: String(error) });
        void clearPid(ownerLease, raiseServer);
        try {
          electron.app.exit(1);
        } catch {
          /* ignore */
        }
      });
    } catch (error) {
      logLaunch("raise-socket-failed", { error: String(error) });
      void clearPid(ownerLease, raiseServer);
      try {
        electron.app.exit(1);
      } catch {
        /* ignore */
      }
      throw startupBlocked(error instanceof Error ? error : new Error(String(error)));
    }
  }
  if (isIncognito()) {
    electron.app.on("window-all-closed", () => {
      finishIncognito(0);
    });
  } else if (accessibilitySetupController) {
    electron.app.on("activate", () => {
      void accessibilitySetupController.run();
    });
    electron.app.on("browser-window-focus", () => {
      void accessibilitySetupController.run();
    });
  }

  function ready() {
    hookPreload(electron.session.defaultSession);
    for (const win of electron.BrowserWindow.getAllWindows()) hookWindow(win, source);
    if (isIncognito()) raiseOurWindows();
    else void accessibilitySetupController?.run();
  }
  if (electron.app.isReady()) ready();
  else void electron.app.whenReady().then(ready);
}

const startupGate = attachElectron();
if (typeof module !== "undefined") {
  module.exports = {
    startupGate,
    createAccessibilitySetupController,
    readInstalledRuntimeIdentity,
    prepareIncognitoSession,
    runtimeOwnedSessionEnv,
    burnIncognitoSession,
    cleanupExitedSession,
    sessionProcessIdsFromPs: instance.sessionProcessIdsFromPs,
  };
}
startupGate.catch((error) => {
  console.error("[incodex] main attach failed", error);
});
