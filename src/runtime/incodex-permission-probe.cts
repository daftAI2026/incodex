// @ts-nocheck
/**
 * [INPUT]: 依赖纯权限策略、精确主进程身份与 Electron 录屏自查。
 * [OUTPUT]: 单 watcher 回传绑定请求与进程代际的录屏状态。
 * [POS]: 稳定 IO 宿主；每个请求捕获策略，换代不重置 nonce 或监听器。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
"use strict";
const fs = require("node:fs");
const path = require("node:path");
const APP_PATH = "/Applications/ChatGPT.app";
const UUID = /^[a-f0-9]{8}-[a-f0-9]{4}-[1-5][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/i;
const REQUEST = "permission-probe-request.json";
const RESPONSE = "permission-probe-response.json";
const MAX_BYTES = 8 * 1024;
const REQUEST_KEYS = ["schemaVersion", "nonce", "installId", "requestId", "appPath", "pid", "processStartIdentity", "requestedAtMs"].sort();

function createPermissionProbeResponder(options = {}) {
  const { root, installId, appPath, pid, processStartIdentity, systemPreferences } = options;
  if ((options.platform || process.platform) !== "darwin" || options.incognito ||
      appPath !== APP_PATH || !UUID.test(installId || "") || !path.isAbsolute(root || "") ||
      !Number.isSafeInteger(pid) || pid <= 0 || typeof processStartIdentity !== "string" || !processStartIdentity ||
      typeof options.policyForRequest !== "function") return null;
  const uid = process.getuid?.();
  if (!Number.isInteger(uid)) return null;
  const directory = path.join(root, "transactions", installId);
  const requestPath = path.join(directory, REQUEST);
  const responsePath = path.join(directory, RESPONSE);
  const markerPath = path.join(directory, "accessibility-setup.json");
  const now = options.now || Date.now;
  let disposed = false, watcher = null, lastNonce = null;

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
      ["pending", "awaiting-user"].includes(marker.state);
  }
  function respond() {
    if (disposed) return;
    let temporary = null;
    try {
      const ancestry = directories();
      const original = read(requestPath);
      const request = JSON.parse(original);
      if (!request || Object.keys(request).sort().join() !== REQUEST_KEYS.join() || request.schemaVersion !== 1 ||
          !/^[a-f0-9]{32}$/.test(request.nonce || "") || request.nonce === lastNonce ||
          request.installId !== installId || typeof request.requestId !== "string" || request.requestId.length < 1 ||
          request.requestId.length > 128 || request.appPath !== APP_PATH ||
          request.pid !== pid || request.processStartIdentity !== processStartIdentity ||
          !Number.isSafeInteger(request.requestedAtMs) || request.requestedAtMs > now() || now() - request.requestedAtMs > 5000 ||
          !markerMatches(request)) return;
      // 与动作分派共用一个 active 指针；一次请求只捕获一个代际。
      const policy = options.policyForRequest();
      if (policy?.protocol !== 1 || typeof policy.requestIdValid !== "function" ||
          typeof policy.interpretScreenPermission !== "function" || !policy.requestIdValid(request.requestId)) return;
      // 系统能力留在稳定宿主。候选只得到读数，不能得到 Electron 授权入口。
      let observed = "unknown";
      try {
        const raw = systemPreferences?.getMediaAccessStatus?.("screen");
        if (["granted", "denied", "not-determined", "restricted"].includes(raw)) observed = raw;
      } catch { /* 缺 API 或异常不是明确拒绝。 */ }
      const checked = policy.interpretScreenPermission(observed);
      const rawStatus = checked?.rawStatus, status = checked?.status;
      // 策略不能替换真实读数，也不能把 unknown 或 denied 声明为 granted。
      // 各属性只读一次；校验与发布使用同一组值，避免 getter 前后改口。
      if (rawStatus !== observed ||
          status !== (observed === "granted" ? "granted" : observed === "unknown" ? "unknown" : "not-granted")) return;
      if (read(requestPath) !== original || !markerMatches(request) || directories() !== ancestry || disposed) return;
      // 仅发布本请求的读数。另一请求、旧进程和 CLI 完成后的读数均退休。
      const response = { ...request, rawStatus: observed, status, observedAtMs: now(), versions: {
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
  }
  if (options.watch !== false) {
    try {
      directories();
      watcher = fs.watch(directory, { persistent: false }, (_event, file) => {
        if (file === null || String(file) === REQUEST) respond();
      });
      watcher.on("error", () => { watcher?.close(); watcher = null; });
    } catch { return null; }
  }
  respond();
  return { respond, dispose() { disposed = true; watcher?.close(); watcher = null; } };
}
export { createPermissionProbeResponder };
