// @ts-nocheck
/**
 * [INPUT]: 依赖构建期adapter pin、固定官方module路径与现有helper登记。
 * [OUTPUT]: 显式激活content-addressed adapter，并原位路由OSN公钥/签名。
 * [POS]: 主动作工厂的Mac feature；兼容adapter换代，不持有key或授权事实。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
"use strict";

const ADAPTER_ROOT = ".incodex/helpers/macos-device-key-adapter";
const ADAPTER_FILE = "incodex-device-key-adapter.node";
const ADAPTER_MANIFEST = "manifest.json";
const REGISTRATION = ".incodex/macos-device-key/registration.json";
const OFFICIAL_MODULE = "native/remote-control-device-key.node";
const DEVICE_KEY_OWNER = Symbol.for("incodex.device-key-hot.owner.v1");
const METHOD_NAMES = ["getDeviceKeyPublic", "signDeviceKey"];
const OSN_KEY_ID = /^dk_osn_[A-Za-z0-9_-]{43}$/;
const ERROR_CODES = new Set([
  "remote_control_device_key_unusable",
  "remote_control_device_key_store_unavailable",
]);

function createDeviceKeyHotFeature(pin, loadRuntime = loadNodeRuntime) {
  let activation = null;

  async function activate() {
    if (activation) return activation;
    activation = Promise.resolve().then(install).finally(() => { activation = null; });
    return activation;
  }

  async function install() {
    let runtime;
    let helperSha256 = null;
    let outcome = "unavailable";
    try {
      runtime = loadRuntime();
      if (runtime.process?.platform !== "darwin" || !validPin(pin)) return unavailable();

      const home = runtime.os.homedir();
      const userRoot = runtime.path.join(home, ".incodex");
      helperSha256 = readHelperIdentity(runtime, home);
      const verified = readVerifiedAdapter(runtime, home, pin);
      if (!verified) return unavailable();
      const officialPath = runtime.path.join(runtime.process.resourcesPath, OFFICIAL_MODULE);
      const official = runtime.requireModule(officialPath);
      const descriptor = Object.getOwnPropertyDescriptor(official, DEVICE_KEY_OWNER);
      const owner = descriptor?.value;

      if (owner) {
        if (!isCompatibleOwner(owner, official)) return unavailable();
        if (owner.active.manifestSha256 === pin.manifestSha256 &&
            owner.active.adapterSha256 === pin.adapterSha256) {
          outcome = "already";
          logActivation(runtime, userRoot, owner.active, helperSha256, outcome);
          return { ok: true, code: "ALREADY_ACTIVE" };
        }
      } else if (!canInstallStableWrappers(official)) {
        return unavailable();
      }

      const adapter = runtime.requireModule(verified.adapterPath);
      if (!validAdapter(adapter) || adapter.prepareDeviceKeyAccess() !== true) return unavailable();
      const preparedHelperSha256 = readHelperIdentity(runtime, home);
      if (helperSha256 && preparedHelperSha256 && helperSha256 !== preparedHelperSha256) return unavailable();
      helperSha256 = preparedHelperSha256 || helperSha256;

      const binding = Object.freeze({
        protocol: 1,
        manifestSha256: pin.manifestSha256,
        adapterSha256: pin.adapterSha256,
        abiVersion: verified.manifest.abiVersion,
        helperSha256,
        adapter,
      });
      if (owner) {
        owner.active = binding;
        outcome = "replaced";
        logActivation(runtime, userRoot, binding, helperSha256, outcome);
        return { ok: true, code: "REPLACED" };
      }

      const installed = installStableWrappers(official, binding, runtime, userRoot, helperSha256);
      if (!installed) return unavailable();
      outcome = "installed";
      logActivation(runtime, userRoot, binding, helperSha256, outcome);
      return { ok: true, code: "INSTALLED" };
    } catch {
      return unavailable();
    }

    function unavailable() {
      if (runtime) logActivation(runtime, runtime.path.join(runtime.os.homedir(), ".incodex"),
        { adapterSha256: validPin(pin) ? pin.adapterSha256 : null }, helperSha256, outcome);
      return { ok: false, code: "UNAVAILABLE" };
    }
  }

  return Object.freeze({ activate });
}

function loadNodeRuntime() {
  const fs = require("node:fs");
  const crypto = require("node:crypto");
  const path = require("node:path");
  const os = require("node:os");
  return { fs, crypto, path, os, process, requireModule: require };
}

function validPin(pin) {
  return pin && validHash(pin.manifestSha256) && validHash(pin.adapterSha256);
}

function validHash(value) {
  return typeof value === "string" && /^[a-f0-9]{64}$/.test(value);
}

function isPrivateOwner(runtime, stat) {
  const uid = typeof runtime.process.getuid === "function" ? runtime.process.getuid() : null;
  return Number.isInteger(uid) && stat.uid === uid && (stat.mode & 0o777) === 0o700;
}

function privateDirectory(runtime, directory) {
  try {
    const stat = runtime.fs.lstatSync(directory);
    return stat.isDirectory() && !stat.isSymbolicLink() && isPrivateOwner(runtime, stat);
  } catch { return false; }
}

function privateFile(runtime, filename, maxBytes, expectedMode = 0o600) {
  try {
    const stat = runtime.fs.lstatSync(filename);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.nlink !== 1 ||
        stat.size < 0 || stat.size > maxBytes || !validPrivateFileOwner(runtime, stat, expectedMode)) return null;
    const bytes = runtime.fs.readFileSync(filename);
    const after = runtime.fs.lstatSync(filename);
    if (!Buffer.isBuffer(bytes) || bytes.length !== stat.size || after.isSymbolicLink() ||
        after.nlink !== 1 || !validPrivateFileOwner(runtime, after, expectedMode) ||
        after.dev !== stat.dev || after.ino !== stat.ino || after.size !== stat.size ||
        after.mtimeMs !== stat.mtimeMs) return null;
    return bytes;
  } catch { return null; }
}

function validPrivateFileOwner(runtime, stat, expectedMode = 0o600) {
  const uid = typeof runtime.process.getuid === "function" ? runtime.process.getuid() : null;
  return Number.isInteger(uid) && stat.uid === uid && (stat.mode & 0o777) === expectedMode;
}

function readVerifiedAdapter(runtime, home, pin) {
  const path = runtime.path;
  const root = path.join(home, ADAPTER_ROOT);
  const adapterDir = path.join(root, pin.adapterSha256);
  const requiredDirectories = [
    path.join(home, ".incodex"),
    path.join(home, ".incodex", "helpers"),
    root,
    adapterDir,
  ];
  if (!requiredDirectories.every(directory => privateDirectory(runtime, directory))) return null;

  const manifestPath = path.join(adapterDir, ADAPTER_MANIFEST);
  const adapterPath = path.join(adapterDir, ADAPTER_FILE);
  const manifestBytes = privateFile(runtime, manifestPath, 64 * 1024, 0o600);
  const adapterBytes = privateFile(runtime, adapterPath, 1024 * 1024, 0o600);
  if (!manifestBytes || !adapterBytes || digest(runtime, manifestBytes) !== pin.manifestSha256 ||
      digest(runtime, adapterBytes) !== pin.adapterSha256) return null;

  const manifest = JSON.parse(manifestBytes.toString("utf8"));
  if (manifest.schemaVersion !== 1 || manifest.platform !== "macos" || manifest.abiVersion !== 1 ||
      manifest.minimumMacOS !== "12.0" || JSON.stringify(manifest.architectures) !== '["arm64","x86_64"]' ||
      !validHash(manifest.sourceSha256) || Object.keys(manifest.files || {}).length !== 1 ||
      manifest.files[ADAPTER_FILE] !== pin.adapterSha256) return null;
  return { manifest, adapterPath };
}

function digest(runtime, bytes) {
  return runtime.crypto.createHash("sha256").update(bytes).digest("hex");
}

function readHelperIdentity(runtime, home) {
  const keychainDir = runtime.path.join(home, ".incodex", "macos-device-key");
  if (!privateDirectory(runtime, keychainDir)) return null;
  const bytes = privateFile(runtime, runtime.path.join(home, REGISTRATION), 4096);
  if (!bytes) return null;
  try {
    const registration = JSON.parse(bytes.toString("utf8"));
    if (Object.keys(registration).length !== 3 || registration.schemaVersion !== 1 ||
        registration.protocolVersion !== 1 || !validHash(registration.helperSha256)) return null;
    return registration.helperSha256;
  } catch { return null; }
}

function validAdapter(adapter) {
  return adapter && typeof adapter.prepareDeviceKeyAccess === "function" &&
    typeof adapter.getDeviceKeyPublic === "function" && typeof adapter.signDeviceKey === "function";
}

function canInstallStableWrappers(official) {
  if (!official || (typeof official !== "object" && typeof official !== "function") || !Object.isExtensible(official)) return false;
  if (Object.getOwnPropertyDescriptor(official, DEVICE_KEY_OWNER)) return false;
  return METHOD_NAMES.every(name => {
    const item = Object.getOwnPropertyDescriptor(official, name);
    return item && item.writable === true && typeof item.value === "function";
  });
}

function isCompatibleOwner(owner, official) {
  if (owner.protocol !== 1 || owner.active?.abiVersion !== 1 ||
      !owner.originalDescriptors || !owner.wrappers || typeof owner.record !== "function") return false;
  return METHOD_NAMES.every(name => {
    const descriptor = Object.getOwnPropertyDescriptor(official, name);
    return descriptor?.value === owner.wrappers[name] && descriptor.writable === true;
  });
}

function installStableWrappers(official, binding, runtime, userRoot, helperSha256) {
  const originalDescriptors = Object.fromEntries(METHOD_NAMES.map(name =>
    [name, Object.getOwnPropertyDescriptor(official, name)]));
  const owner = {
    protocol: 1,
    active: binding,
    originalDescriptors,
    wrappers: Object.create(null),
    record: null,
  };
  owner.record = (binding, operation, ok, error) => logOperation(runtime, userRoot, binding,
    binding.helperSha256 ?? helperSha256, operation, ok, error);
  for (const name of METHOD_NAMES) {
    const original = originalDescriptors[name].value;
    owner.wrappers[name] = function(...args) {
      if (typeof args[0] !== "string" || !OSN_KEY_ID.test(args[0])) {
        return Reflect.apply(original, this, args);
      }
      const active = owner.active;
      return invokeDeviceOperation(owner, active, name, this, args);
    };
  }

  const applied = [];
  try {
    for (const name of METHOD_NAMES) {
      Object.defineProperty(official, name, { ...originalDescriptors[name], value: owner.wrappers[name] });
      applied.push(name);
    }
    Object.defineProperty(official, DEVICE_KEY_OWNER, { value: owner, enumerable: false, writable: false, configurable: false });
    return true;
  } catch {
    for (const name of applied.reverse()) {
      try { Object.defineProperty(official, name, originalDescriptors[name]); } catch { /* preserve the original failure */ }
    }
    return false;
  }
}

function invokeDeviceOperation(owner, binding, name, receiver, args) {
  const operation = name === "getDeviceKeyPublic" ? "public" : "sign";
  let result;
  try {
    result = Reflect.apply(binding.adapter[name], binding.adapter, args);
  } catch (error) {
    owner.record(binding, operation, false, error);
    throw error;
  }
  if (result && typeof result.then === "function") {
    return result.then(value => {
      owner.record(binding, operation, true, null);
      return value;
    }, error => {
      owner.record(binding, operation, false, error);
      throw error;
    });
  }
  owner.record(binding, operation, true, null);
  return result;
}

function logActivation(runtime, userRoot, binding, helperSha256, status) {
  const entry = {
    pid: runtime.process.pid,
    adapterSha256: validHash(binding?.adapterSha256) ? binding.adapterSha256 : null,
    helperSha256: validHash(helperSha256) ? helperSha256 : null,
    status,
  };
  appendLog(runtime, userRoot, "device-key-hot-activation", entry);
}

function logOperation(runtime, userRoot, binding, helperSha256, operation, ok, error) {
  const rawCode = error?.errorCode;
  const entry = {
    pid: runtime.process.pid,
    adapterSha256: validHash(binding?.adapterSha256) ? binding.adapterSha256 : null,
    helperSha256: validHash(helperSha256) ? helperSha256 : null,
    operation,
    ok,
    errorCode: ok ? null : ERROR_CODES.has(rawCode) ? rawCode : "unknown",
  };
  appendLog(runtime, userRoot, "device-key-hot-operation", entry);
}

function appendLog(runtime, userRoot, event, value) {
  try {
    const safeHome = runtime.requireModule("./incodex-safe-home.cjs");
    const instance = runtime.requireModule("./incodex-instance.cjs");
    const stateDir = instance.targetStateDir(userRoot, runtime.process.execPath);
    safeHome.rotateAndAppendLog(stateDir,
      `${new Date().toISOString()} ${event} ${JSON.stringify(value)}\n`);
  } catch { /* Diagnostics must never change native operation results. */ }
}

export { createDeviceKeyHotFeature };
