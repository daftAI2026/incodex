// @ts-nocheck
/**
 * [INPUT]: 依赖 Electron screen API、私有事务目录、nonce 请求与目录通知。
 * [OUTPUT]: 提供显式安装 preflight 与绑定 requestId、nonce、进程代际的只读回执。
 * [POS]: 主进程只读探针；合并目录事件并限次重读，不请求授权或展示 UI。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
"use strict";
const fs = require("node:fs");
const path = require("node:path");
const APP_PATH = "/Applications/ChatGPT.app";
const APP_EXECUTABLE_PATH = "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT";
const INSTALL_PERMISSION_PREFLIGHT_ENV = "INCODEX_INTERNAL_PERMISSION_PREFLIGHT";
const INSTALL_PERMISSION_PROBE_TIMEOUT_MS = 30_000;
const UUID = /^[a-f0-9]{8}-[a-f0-9]{4}-[1-5][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/i;
const REQUEST = "permission-probe-request.json";
const RESPONSE = "permission-probe-response.json";
const MAX_BYTES = 8 * 1024;
const PROBE_READ_DELAY_MS = 25;
const REQUEST_KEYS = ["schemaVersion", "nonce", "installId", "requestId", "appPath", "pid", "processStartIdentity", "requestedAtMs"].sort();

function isInstallPermissionPreflight(env, platform) {
  const value = env?.[INSTALL_PERMISSION_PREFLIGHT_ENV];
  return platform === "darwin" && typeof value === "string" && value.length > 0;
}

function installPermissionPreflightRequest(value) {
  if (typeof value !== "string" || value.length === 0 || Buffer.byteLength(value, "utf8") > 512) {
    throw new Error("[incodex] invalid install permission preflight request");
  }
  let request;
  try { request = JSON.parse(value); }
  catch { throw new Error("[incodex] invalid install permission preflight request"); }
  if (
    !request || typeof request !== "object" || Array.isArray(request) ||
    Object.keys(request).sort().join(",") !== "installId,requestId" ||
    !UUID.test(request.installId || "") || !UUID.test(request.requestId || "")
  ) {
    throw new Error("[incodex] invalid install permission preflight request");
  }
  return { installId: request.installId, requestId: request.requestId };
}

function runInstallPermissionProbe(options = {}) {
  const {
    env = process.env, platform = process.platform, execPath, incognito, identity,
    electron, pid, processStartIdentity, root, versions, readMarker,
  } = options;
  if (
    !isInstallPermissionPreflight(env, platform) || incognito ||
    path.resolve(execPath || "") !== APP_EXECUTABLE_PATH
  ) {
    throw new Error("[incodex] install permission preflight is unavailable");
  }
  const request = installPermissionPreflightRequest(env[INSTALL_PERMISSION_PREFLIGHT_ENV]);
  if (!electron?.app || !electron.systemPreferences) {
    throw new Error("[incodex] install permission preflight is unavailable");
  }
  if (!identity || identity.installId !== request.installId || identity.appPath !== APP_PATH) {
    throw new Error("[incodex] install permission preflight identity mismatch");
  }
  const marker = typeof readMarker === "function" ? readMarker(identity) : null;
  if (
    marker?.kind !== "cli-owned" || marker.marker?.requestId !== request.requestId ||
    !["pending", "awaiting-user"].includes(marker.marker?.state)
  ) {
    throw new Error("[incodex] install permission preflight marker mismatch");
  }
  if (typeof processStartIdentity !== "string" || processStartIdentity.length === 0) {
    throw new Error("[incodex] install permission preflight process identity unavailable");
  }
  if (typeof electron.app.setActivationPolicy === "function") {
    electron.app.setActivationPolicy("prohibited");
  }

  return new Promise((resolve, reject) => {
    let responder = null;
    let settled = false;
    const timeout = setTimeout(() => finish(new Error("[incodex] install permission preflight timed out")),
      INSTALL_PERMISSION_PROBE_TIMEOUT_MS);
    function dispose() {
      clearTimeout(timeout);
      try { responder?.dispose(); } catch { /* probe exit still owns this process. */ }
      responder = null;
    }
    function finish(error) {
      if (settled) return;
      settled = true;
      dispose();
      if (error) {
        reject(error);
        return;
      }
      try {
        electron.app.quit();
        resolve();
      } catch (quitError) {
        reject(quitError);
      }
    }
    try {
      responder = createPermissionProbeResponder({
        root,
        ...identity,
        requestId: request.requestId,
        pid,
        processStartIdentity,
        systemPreferences: electron.systemPreferences,
        versions,
        platform,
        incognito: false,
        onRequestRetired: () => finish(),
      });
      if (!responder) throw new Error("[incodex] install permission preflight responder unavailable");
    } catch (error) {
      finish(error instanceof Error ? error : new Error(String(error)));
    }
  });
}

function readScreenPermission(preferences) {
  let rawStatus = "unknown";
  try {
    const raw = preferences?.getMediaAccessStatus?.("screen");
    if (["granted", "denied", "not-determined", "restricted"].includes(raw)) rawStatus = raw;
  } catch { /* 缺 API 或异常不是明确拒绝。 */ }
  return { rawStatus, status: rawStatus === "granted" ? "granted" : rawStatus === "unknown" ? "unknown" : "not-granted" };
}

function createPermissionProbeResponder(options = {}) {
  const { root, installId, requestId: expectedRequestId, appPath, pid, processStartIdentity, systemPreferences } = options;
  if ((options.platform || process.platform) !== "darwin" || options.incognito ||
      appPath !== APP_PATH || !UUID.test(installId || "") || !path.isAbsolute(root || "") ||
      (expectedRequestId !== undefined && !UUID.test(expectedRequestId)) ||
      !Number.isSafeInteger(pid) || pid <= 0 || typeof processStartIdentity !== "string" || !processStartIdentity) return null;
  const uid = process.getuid?.();
  if (!Number.isInteger(uid)) return null;
  const directory = path.join(root, "transactions", installId);
  const requestPath = path.join(directory, REQUEST);
  const responsePath = path.join(directory, RESPONSE);
  const markerPath = path.join(directory, "accessibility-setup.json");
  const now = options.now || Date.now;
  let disposed = false, watcher = null, lastNonce = null;
  let responseReadTimer = null, missingRetryAvailable = false;

  function clearResponseReadTimer() {
    if (responseReadTimer !== null) {
      clearTimeout(responseReadTimer);
      responseReadTimer = null;
    }
    missingRetryAvailable = false;
  }

  function directories() {
    return [root, path.join(root, "transactions"), directory].map(file => {
      const stat = fs.lstatSync(file);
      if (!stat.isDirectory() || stat.isSymbolicLink() || stat.uid !== uid || (stat.mode & 0o077)) throw new Error("unsafe probe directory");
      return `${stat.dev}:${stat.ino}`;
    }).join("/");
  }
  function read(file) {
    const descriptor = fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
    try {
      const stat = fs.fstatSync(descriptor);
      if (!stat.isFile() || stat.uid !== uid || (stat.mode & 0o077) || stat.size > MAX_BYTES) throw new Error("unsafe probe file");
      const bytes = Buffer.alloc(MAX_BYTES + 1);
      const count = fs.readSync(descriptor, bytes, 0, bytes.length, 0);
      if (count > MAX_BYTES) throw new Error("oversized probe file");
      return bytes.subarray(0, count).toString("utf8");
    } finally { fs.closeSync(descriptor); }
  }
  function markerMatches(request) {
    const marker = JSON.parse(read(markerPath));
    return marker.schemaVersion === 1 && marker.installId === installId && marker.appPath === APP_PATH &&
      marker.presentationOwner === "cli" && marker.requestId === request.requestId &&
      (expectedRequestId === undefined || request.requestId === expectedRequestId) &&
      ["pending", "awaiting-user"].includes(marker.state);
  }
  function requestIsAbsent() {
    try {
      fs.lstatSync(requestPath);
      return false;
    } catch (error) {
      return error?.code === "ENOENT";
    }
  }
  function retireIfRequestRemoved() {
    if (disposed || !lastNonce || typeof options.onRequestRetired !== "function") return false;
    try {
      const ancestry = directories();
      if (!requestIsAbsent() || directories() !== ancestry || !requestIsAbsent()) return false;
      disposed = true;
      clearResponseReadTimer();
      watcher?.close();
      watcher = null;
      options.onRequestRetired(lastNonce);
      return true;
    } catch { return false; }
  }
  function respond() {
    if (disposed) return "done";
    if (retireIfRequestRemoved()) return "done";
    let temporary = null;
    let outcome = "done";
    try {
      const ancestry = directories();
      let original;
      try { original = read(requestPath); }
      catch (error) {
        if (error?.code === "ENOENT") outcome = "missing";
        return outcome;
      }
      const request = JSON.parse(original);
      if (!request || Object.keys(request).sort().join() !== REQUEST_KEYS.join() || request.schemaVersion !== 1 ||
          !/^[a-f0-9]{32}$/.test(request.nonce || "") || request.nonce === lastNonce ||
          request.installId !== installId || !UUID.test(request.requestId || "") || request.appPath !== APP_PATH ||
          (expectedRequestId !== undefined && request.requestId !== expectedRequestId) ||
          request.pid !== pid || request.processStartIdentity !== processStartIdentity ||
          !Number.isSafeInteger(request.requestedAtMs) || request.requestedAtMs > now() || now() - request.requestedAtMs > 5000 ||
          !markerMatches(request)) return;
      const checked = readScreenPermission(systemPreferences);
      if (read(requestPath) !== original || !markerMatches(request) || directories() !== ancestry || disposed) return;
      // 仅发布本请求的读数。另一请求、旧进程和 CLI 完成后的读数均退休。
      const response = { ...request, ...checked, observedAtMs: now(), versions: {
        electron: String(options.versions?.electron || "").slice(0, 64),
        chrome: String(options.versions?.chrome || "").slice(0, 64),
      } };
      // 专用回传文件可被新请求替换；不消费旧内容，CLI 只认新 nonce。
      try { read(responsePath); } catch (error) { if (error.code !== "ENOENT") return; }
      temporary = path.join(directory, `.permission-probe-response.${pid}.${request.nonce}.tmp`);
      const descriptor = fs.openSync(temporary, "wx", 0o600);
      try { fs.writeFileSync(descriptor, `${JSON.stringify(response)}\n`); fs.fsyncSync(descriptor); }
      finally { fs.closeSync(descriptor); }
      if (read(requestPath) !== original || !markerMatches(request) || directories() !== ancestry || disposed) return;
      fs.renameSync(temporary, responsePath); temporary = null;
      lastNonce = request.nonce;
    } catch { /* 不可信、未就绪或退休的请求不生成猜测结果。 */ }
    finally { if (temporary) { try { fs.unlinkSync(temporary); } catch {} } }
    return outcome;
  }
  function scheduleRespond(retry = false) {
    if (disposed || responseReadTimer !== null) return;
    if (!retry) missingRetryAvailable = true;
    responseReadTimer = setTimeout(() => {
      responseReadTimer = null;
      if (disposed) {
        missingRetryAvailable = false;
        return;
      }
      const outcome = respond();
      if (outcome === "missing" && missingRetryAvailable) {
        missingRetryAvailable = false;
        scheduleRespond(true);
      } else {
        missingRetryAvailable = false;
      }
    }, PROBE_READ_DELAY_MS);
  }
  if (options.watch !== false) {
    try {
      directories();
      watcher = fs.watch(directory, { persistent: false }, () => scheduleRespond());
      watcher.on("error", () => {
        clearResponseReadTimer();
        watcher?.close();
        watcher = null;
      });
    } catch { return null; }
  }
  respond();
  return { respond, dispose() {
    disposed = true;
    clearResponseReadTimer();
    watcher?.close();
    watcher = null;
  } };
}
export { createPermissionProbeResponder, isInstallPermissionPreflight, readScreenPermission, runInstallPermissionProbe };
