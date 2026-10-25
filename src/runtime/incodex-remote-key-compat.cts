// @ts-nocheck
/**
 * [INPUT]: 依赖 manifest 验证的 native loader 与官方 secure-key 存储。
 * [OUTPUT]: 为指定设备密钥接入创建兼容与固定消费者；不代理其他密钥。
 * [POS]: 主 Runtime 的可选原生兼容入口；失败不阻塞官方启动。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
"use strict";
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { privateDirectory, privateFile } = require("./incodex-permission-native.cts");
const nativeName = "incodex-remote-key-compat.node";
const manifestName = "incodex-remote-key-manifest.json";
const routed = new WeakSet();
const osProtectedKey = /^dk_osn_[A-Za-z0-9_-]{43}$/;
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");

function installRemoteKeyCompatibility({ platform = process.platform, incognito = false, directory = __dirname,
  resourcesPath = process.resourcesPath, load = name => require(name) } = {}) {
  if (platform !== "darwin" || incognito) return false;
  try {
    privateDirectory(directory);
    const runtime = JSON.parse(privateFile(path.join(directory, "runtime-manifest.json"), 1024 * 1024));
    const bytes = privateFile(path.join(directory, manifestName), 64 * 1024);
    if (hash(bytes) !== runtime.files?.[manifestName]) return false;
    const manifest = JSON.parse(bytes);
    if (manifest.schemaVersion !== 1 || manifest.platform !== "macos" || manifest.abiVersion !== 1 ||
        manifest.minimumMacOS !== "12.0" || JSON.stringify(manifest.architectures) !== '["arm64","x86_64"]' ||
        Object.keys(manifest.files || {}).join() !== nativeName) return false;
    const addon = path.join(directory, nativeName);
    if (manifest.files[nativeName] !== runtime.files?.[nativeName] ||
        hash(privateFile(addon, 1024 * 1024)) !== manifest.files[nativeName]) return false;
    // --- 原位保留官方模块对象；固定消费者的身份由原生边界确认。 ---
    // The native gate additionally checks the host and this module's signatures.
    const official = path.join(resourcesPath, "native/remote-control-device-key.node");
    const exports = load(official);
    const native = load(addon);
    if (native.install(official) !== true) return false;
    if (!routed.has(exports) && typeof native.prepareDeviceKeyAccess === "function" &&
        typeof native.getDeviceKeyPublic === "function" && typeof native.signDeviceKey === "function" &&
        native.prepareDeviceKeyAccess() === true) {
      const names = ["getDeviceKeyPublic", "signDeviceKey"];
      const descriptors = names.map(name => Object.getOwnPropertyDescriptor(exports, name));
      // 两项都可写才提交，避免只换 public 或只换 sign 的半代际。
      if (descriptors.every(item => item && item.writable === true && typeof item.value === "function")) {
        const replacements = {};
        for (const [index, name] of names.entries()) {
          const original = descriptors[index].value;
          replacements[name] = { value: function(...args) {
            if (typeof args[0] !== "string" || !osProtectedKey.test(args[0])) return Reflect.apply(original, this, args);
            // 拒绝和取消必须返回调用方，不能换回宿主重试并重新弹授权。
            return Reflect.apply(native[name], native, args);
          } };
        }
        Object.defineProperties(exports, replacements);
        routed.add(exports);
      }
    }
    return true;
  } catch {
    // Compatibility is optional: official startup and genuine errors survive.
    return false;
  }
}
export { installRemoteKeyCompatibility };
