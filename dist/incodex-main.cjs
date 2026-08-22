// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const safeHome = require("./incodex-safe-home.cjs");
const ipcGuard = require("./incodex-ipc-guard.cjs");
const instance = require("./incodex-instance.cjs");
const windowKind = require("./incodex-window-kind.cjs");
const USER_ROOT = path.join(os.homedir(), ".incodex");
const DEFAULT_CODEX_HOME = path.join(os.homedir(), ".codex");
const READY_TIMEOUT_MS = 15_000;
let capturedSourceHome = null;
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
    if (capturedSourceHome)
        return capturedSourceHome;
    if (process.env.INCODEX_SOURCE_HOME) {
        return safeHome.resolveSourceHome(process.env.INCODEX_SOURCE_HOME, DEFAULT_CODEX_HOME);
    }
    return resolvedCodexHome();
}
function captureSourceHome() {
    if (isIncognito())
        return;
    capturedSourceHome = resolvedCodexHome();
}
function isIncognito() {
    if (process.env.INCODEX_INCOGNITO === "1")
        return true;
    return safeHome.isManagedSessionHome(resolvedCodexHome(), USER_ROOT);
}
function captureChildOwnerSnapshot() {
    if (!isIncognito() || typeof instance.processIdentity !== "function")
        return null;
    const live = instance.processIdentity(process.pid);
    if (!live?.processStartIdentity)
        return null;
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
    if (!home || !sessionId)
        return null;
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
    if (!fs.existsSync(file))
        return "";
    return fs.readFileSync(file, "utf8");
}
function readLocaleOverride() {
    const file = path.join(sourceHome(), "config.toml");
    if (!fs.existsSync(file))
        return "";
    try {
        const match = fs.readFileSync(file, "utf8").match(/^\s*localeOverride\s*=\s*"([^"]+)"/m);
        return match?.[1]?.trim() ?? "";
    }
    catch {
        return "";
    }
}
function burnIncognitoSession(session, ownerSnapshot, userRoot = USER_ROOT) {
    if (!session ||
        !ownerSnapshot ||
        !Number.isInteger(ownerSnapshot.pid) ||
        ownerSnapshot.pid <= 0 ||
        typeof ownerSnapshot.processStartIdentity !== "string" ||
        !ownerSnapshot.processStartIdentity) {
        return false;
    }
    const expected = {
        userRoot,
        sessionId: session.sessionId,
        ino: session.ino,
        dev: session.dev,
    };
    const removed = safeHome.burnSessionHomeWithOwner(session.root, expected, ownerSnapshot);
    if (removed && !safeHome.writeBurnProof(session.root, expected)) {
        logLaunch("burn-proof-write-failed", { home: session.root });
    }
    return removed;
}
function burnIncognitoHome() {
    const session = sessionFromEnv();
    const home = session?.root || session?.home || process.env.CODEX_HOME;
    if (!home)
        return;
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
    }
    catch (error) {
        logLaunch("burn-refused", { error: String(error), home });
    }
}
function markSessionReady() {
    const session = sessionFromEnv();
    if (!session?.root)
        return;
    try {
        safeHome.writeReady(session.root);
    }
    catch {
        /* already written */
    }
}
async function writePid() {
    try {
        return await instance.acquireOwnerLease(stateRoot(), instance.currentOwner(process.env.INCODEX_SESSION_ID, process.execPath));
    }
    catch (error) {
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
        if (server?.listening)
            server.close();
    }
    catch {
        /* Server shutdown is best effort when no managed lease remains. */
    }
}
async function incognitoAlreadyRunning() {
    const records = instance.readOwnerRecords(stateRoot());
    if (records.some(({ state }) => state.kind === "unverifiable")) {
        throw new Error("owner lease is unverifiable");
    }
    const owners = records.filter(({ state }) => state.kind === "valid").map(({ state }) => state.owner);
    for (const owner of owners) {
        if (await instance.connectExistingWithRetry(stateRoot(), instance.ownerToken(owner)))
            return true;
    }
    if (owners.length === 0 || owners.every((owner) => instance.staleOwnerRecord(owner)))
        return false;
    throw new Error("owner lease is active but its raise socket is unavailable");
}
function raisePid(pid) {
    if (!Number.isInteger(pid) || pid <= 0)
        return;
    if (process.platform !== "darwin")
        return;
    spawn("osascript", [
        "-e",
        `tell application "System Events" to set frontmost of (first process whose unix id is ${pid}) to true`,
    ], { detached: true, stdio: "ignore" }).unref();
}
function isAuxiliaryWindow(win) {
    if (!win || win.isDestroyed())
        return true;
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
    }
    catch {
        return false;
    }
}
function mainWindows(electron) {
    return electron.BrowserWindow.getAllWindows().filter((win) => !isAuxiliaryWindow(win));
}
function hideAuxiliaryWindows(electron) {
    for (const win of electron.BrowserWindow.getAllWindows()) {
        if (!isAuxiliaryWindow(win))
            continue;
        try {
            win.hide();
        }
        catch {
            /* ignore */
        }
    }
}
function raiseOurWindows() {
    let electron;
    try {
        electron = require("electron");
    }
    catch {
        raisePid(process.pid);
        return;
    }
    hideAuxiliaryWindows(electron);
    try {
        if (process.platform === "darwin")
            electron.app.focus({ steal: true });
    }
    catch {
        /* ignore */
    }
    for (const win of mainWindows(electron)) {
        try {
            if (win.isMinimized())
                win.restore();
            win.show();
            win.focus();
            if (typeof win.moveTop === "function")
                win.moveTop();
        }
        catch {
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
    if (!Number.isInteger(pid) || pid <= 0)
        return;
    for (const delay of [150, 400, 800, 1400]) {
        setTimeout(() => raisePid(pid), delay);
    }
}
function logLaunch(message, extra) {
    try {
        safeHome.rotateAndAppendLog(stateRoot(), `${new Date().toISOString()} ${message}${extra ? ` ${JSON.stringify(extra)}` : ""}\n`);
    }
    catch {
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
        if (!win || win.isDestroyed())
            return "";
        const b = win.getBounds();
        return `${b.x},${b.y},${b.width},${b.height}`;
    }
    catch {
        return "";
    }
}
function readSourceBounds() {
    const raw = process.env.INCODEX_SOURCE_BOUNDS;
    if (!raw)
        return null;
    const parts = raw.split(",").map(Number);
    if (parts.length !== 4 || parts.some((n) => !Number.isFinite(n)))
        return null;
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
    if (bounds.y < work.y)
        bounds.y = work.y;
    if (process.platform === "darwin") {
        bounds.height = Math.min(work.height, bounds.height);
        if (bounds.x < work.x || bounds.x + bounds.width > work.x + work.width) {
            bounds.x = work.x;
        }
        if (bounds.y < work.y || bounds.y + bounds.height > work.y + work.height) {
            bounds.y = work.y;
        }
    }
    else {
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
    if (!win || win.isDestroyed())
        return;
    const source = readSourceBounds();
    if (!source)
        return;
    let screen;
    try {
        screen = require("electron").screen;
    }
    catch {
        return;
    }
    try {
        win.setBounds(chromeTileBounds(source, screen));
    }
    catch {
        /* ignore */
    }
}
const launchHolder = { current: null };
function launchIncognito() {
    return instance.singleFlight(launchHolder, launchIncognitoOnce);
}
function prepareIncognitoSession(options = {}) {
    const { userRoot = USER_ROOT, sourceHomePath = sourceHome(), appTarget = targetId(), pid = process.pid, createSessionHome = safeHome.createSessionHome, copySettings = safeHome.copySettings, burnSessionHome = safeHome.burnSessionHome, log = logLaunch, } = options;
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
    }
    catch (error) {
        if (session) {
            try {
                burnSessionHome(session.root, {
                    userRoot,
                    sessionId: session.sessionId,
                    ino: session.ino,
                    dev: session.dev,
                });
            }
            catch (cleanupError) {
                try {
                    log("prepare-burn-refused", {
                        error: String(cleanupError),
                        sessionId: session.sessionId,
                    });
                }
                catch {
                    /* Cleanup logging must not replace the preparation failure. */
                }
            }
        }
        try {
            log("prepare-failed", { error: String(error) });
        }
        catch {
            /* Logging is best effort; the caller still receives prepare-failed. */
        }
        return { ok: false, reason: "prepare-failed" };
    }
}
async function launchIncognitoOnce() {
    let alreadyRunning;
    try {
        alreadyRunning = await incognitoAlreadyRunning();
    }
    catch (error) {
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
    }
    catch (error) {
        logLaunch("janitor-failed", { error: String(error) });
    }
    const prepared = prepareIncognitoSession();
    if (!prepared.ok)
        return Promise.resolve(prepared);
    const { session } = prepared;
    const bin = process.execPath;
    if (!bin) {
        try {
            safeHome.burnSessionHome(session.root, {
                userRoot: USER_ROOT,
                sessionId: session.sessionId,
                ino: session.ino,
                dev: session.dev,
            });
        }
        catch {
            /* ignore */
        }
        return Promise.resolve({ ok: false, reason: "spawn-failed" });
    }
    const args = [`--user-data-dir=${session.chromium}`, "codex://new?mode=codex"];
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
            if (settled)
                return;
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
                    INCODEX_SESSION_INO: String(session.ino),
                    INCODEX_SESSION_DEV: String(session.dev),
                    CODEX_ELECTRON_USER_DATA_PATH: session.chromium,
                    INCODEX_SOURCE_BOUNDS: sourceBounds,
                    INCODEX_SOURCE_HOME: sourceHome(),
                },
            });
        }
        catch (error) {
            logLaunch("spawn-threw", { error: String(error) });
            try {
                safeHome.burnSessionHome(session.root, {
                    userRoot: USER_ROOT,
                    sessionId: session.sessionId,
                    ino: session.ino,
                    dev: session.dev,
                });
            }
            catch {
                /* ignore */
            }
            done({ ok: false, reason: "spawn-failed" });
            return;
        }
        if (!child.pid) {
            logLaunch("spawn-no-pid");
            try {
                safeHome.burnSessionHome(session.root, {
                    userRoot: USER_ROOT,
                    sessionId: session.sessionId,
                    ino: session.ino,
                    dev: session.dev,
                });
            }
            catch {
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
            const expected = {
                userRoot: USER_ROOT,
                sessionId: session.sessionId,
                ino: session.ino,
                dev: session.dev,
            };
            const childProof = safeHome.readBurnProof(session.root, USER_ROOT, session.sessionId);
            let originalRemoved = Boolean(childProof && childProof.ino === expected.ino && childProof.dev === expected.dev);
            const tryBurn = (attempt) => {
                const attemptExpected = safeHome.cleanupExpectedForAttempt(expected, originalRemoved);
                try {
                    const removed = childOwner && !originalRemoved
                        ? safeHome.burnSessionHomeWithOwner(session.root, attemptExpected, childOwner)
                        : safeHome.burnSessionHome(session.root, attemptExpected);
                    if (removed)
                        originalRemoved = true;
                }
                catch (error) {
                    if (attempt < 5) {
                        setTimeout(() => tryBurn(attempt + 1), 250 * attempt);
                        return;
                    }
                    logLaunch("parent-burn-refused", { error: String(error) });
                    return;
                }
                // Thread-writer / plugin cache can recreate the folder after a
                // successful rm. Keep sweeping for a short window.
                if (attempt < 5 && (originalRemoved || fs.existsSync(session.root))) {
                    setTimeout(() => tryBurn(attempt + 1), 400 * attempt);
                }
                else if (!fs.existsSync(session.root)) {
                    safeHome.clearBurnProof(USER_ROOT, session.sessionId);
                }
            };
            tryBurn(1);
            if (!settled)
                done({ ok: false, reason: "exited-early" });
        });
        try {
            childOwner = safeHome.handoffSessionOwner(session.root, child.pid);
        }
        catch (error) {
            logLaunch("owner-handoff-failed", { error: String(error), sessionId: session.sessionId });
            try {
                if (!child.killed)
                    child.kill();
            }
            catch {
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
function rememberWindow(win) {
    if (!win || typeof win.id !== "number")
        return;
    ipcGuard.bindWindowIdentity(allowedWindows, win, trustedOrigins);
    win.once("closed", () => allowedWindows.delete(win.id));
}
function authorizeEvent(event) {
    return ipcGuard.authorizeSender(ipcGuard.snapshotFromEvent(event), allowedWindows);
}
function hookPreload(session) {
    if (!session || session.__incodexPreload)
        return;
    session.__incodexPreload = true;
    const preload = pickFile("incodex-preload.cjs");
    if (!fs.existsSync(preload))
        return;
    try {
        if (typeof session.registerPreloadScript === "function") {
            session.registerPreloadScript({ filePath: preload, type: "frame" });
        }
        else if (typeof session.setPreloads === "function") {
            session.setPreloads([...(session.getPreloads?.() ?? []), preload]);
        }
    }
    catch (error) {
        console.error("[incodex] preload failed", error);
    }
}
function reportInjectionError(error) {
    logLaunch("ui-injection-failed", { error: String(error) });
}
function reportInjectionProbe(win) {
    return win.webContents.executeJavaScript("window.__incodexUiProbe", false).then((probe) => {
        logLaunch("ui-probe", probe);
    });
}
function hookWindow(win, source) {
    if (!win?.webContents || isAuxiliaryWindow(win))
        return;
    rememberWindow(win);
    hookPreload(win.webContents.session);
    const run = (report) => {
        if (!source || win.webContents.isDestroyed())
            return;
        if (!ipcGuard.bindWindowIdentity(allowedWindows, win, trustedOrigins))
            return;
        const locale = JSON.stringify(readLocaleOverride());
        const prefix = `window.__incodexIncognito=${isIncognito() ? "true" : "false"};window.__incodexLocale=${locale};`;
        win.webContents
            .executeJavaScript(prefix + source, false)
            .then(() => (report ? reportInjectionProbe(win) : undefined))
            .catch((error) => reportInjectionError(error));
    };
    win.webContents.on("dom-ready", () => run(false));
    win.webContents.on("did-finish-load", () => run(true));
    run(false);
}
async function attachElectron() {
    let electron;
    try {
        electron = require("electron");
    }
    catch {
        return;
    }
    const packagedOrigin = ipcGuard.navigationOrigin(require("node:url").pathToFileURL(electron.app.getAppPath()).href);
    packagedOrigin && trustedOrigins.add(packagedOrigin);
    captureSourceHome();
    if (!isIncognito()) {
        try {
            safeHome.sweepOrphanSessions(USER_ROOT, { targetId: targetId() });
        }
        catch (error) {
            logLaunch("janitor-failed", { error: String(error) });
        }
    }
    else {
        process.env.INCODEX_INCOGNITO = "1";
    }
    const source = injectSource();
    let ownerLease = null;
    let raiseServer = null;
    electron.ipcMain.handle("incodex-action", async (event, payload) => {
        const requestId = typeof payload?.requestId === "string" ? payload.requestId : "";
        const gate = authorizeEvent(event);
        if (!gate.ok)
            return ipcGuard.actionResponse(requestId, gate);
        const action = payload?.action;
        if (action === "open") {
            if (isIncognito()) {
                return ipcGuard.actionResponse(requestId, { ok: false, code: "ALREADY_INCOGNITO", reason: "already-incognito" });
            }
            const result = await launchIncognito();
            return ipcGuard.actionResponse(requestId, {
                ok: result.ok === true,
                code: result.ok ? "OK" : String(result.reason || "FAILED").toUpperCase(),
                reason: result.reason,
            });
        }
        if (action === "quit") {
            if (!isIncognito()) {
                return ipcGuard.actionResponse(requestId, { ok: false, code: "NOT_INCOGNITO", reason: "not-incognito" });
            }
            burnIncognitoHome();
            await clearPid(ownerLease, raiseServer);
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
                }
                catch {
                    /* ignore */
                }
            }
            return;
        }
        hookWindow(win, source);
        if (!isIncognito())
            return;
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
            if (mainWindows(electron).some((open) => open !== win && !open.isDestroyed()))
                return;
            burnIncognitoHome();
            void clearPid(ownerLease, raiseServer);
            electron.app.exit(0);
        });
    });
    if (isIncognito()) {
        ownerLease = await writePid();
        if (!ownerLease) {
            try {
                electron.app.exit(1);
            }
            catch {
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
                }
                catch {
                    /* ignore */
                }
            });
        }
        catch (error) {
            logLaunch("raise-socket-failed", { error: String(error) });
            void clearPid(ownerLease, raiseServer);
            try {
                electron.app.exit(1);
            }
            catch {
                /* ignore */
            }
            throw startupBlocked(error instanceof Error ? error : new Error(String(error)));
        }
        electron.app.on("window-all-closed", () => {
            burnIncognitoHome();
            void clearPid(ownerLease, raiseServer);
            electron.app.exit(0);
        });
        electron.app.on("before-quit", () => {
            burnIncognitoHome();
            void clearPid(ownerLease, raiseServer);
        });
    }
    const ready = () => {
        hookPreload(electron.session.defaultSession);
        for (const win of electron.BrowserWindow.getAllWindows())
            hookWindow(win, source);
        if (isIncognito())
            raiseOurWindows();
    };
    if (electron.app.isReady())
        ready();
    else
        void electron.app.whenReady().then(ready);
}
const startupGate = attachElectron();
if (typeof module !== "undefined") {
    module.exports = { startupGate, prepareIncognitoSession, burnIncognitoSession };
}
startupGate.catch((error) => {
    console.error("[incodex] main attach failed", error);
});
