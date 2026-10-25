// @ts-nocheck
/**
 * [INPUT]: 依赖构建期adapter双pin、动作token、官方module与helper登记。
 * [OUTPUT]: 按激活token换绑业务；回执记录闭合阶段和失败码。
 * [POS]: 主动作工厂的Mac feature；不持有key、授权事实或wrapper链。
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

function createDeviceKeyHotFeature(pin: any, loadRuntime: () => any = loadNodeRuntime, activationToken = "") {
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
    let phase = "load_runtime";
    let failureReason = "runtime_load_failed";
    try {
      runtime = loadRuntime();
      phase = "validate_activation";
      if (runtime.process?.platform !== "darwin") return fail("unsupported_platform");
      if (!validPin(pin)) return fail("invalid_adapter_pin");
      if (!validActivationAction(activationToken)) return fail("invalid_activation_token");

      const home = runtime.os.homedir();
      const userRoot = runtime.path.join(home, ".incodex");
      phase = "read_helper_registration";
      helperSha256 = readHelperIdentity(runtime, home);
      phase = "verify_adapter";
      const verified = readVerifiedAdapter(runtime, home, pin);
      if (!verified) return fail("adapter_verification_failed");

      phase = "load_official_exports";
      const officialPath = runtime.path.join(runtime.process.resourcesPath, OFFICIAL_MODULE);
      const official = runtime.requireModule(officialPath);
      phase = "inspect_official_exports";
      const descriptor = Object.getOwnPropertyDescriptor(official, DEVICE_KEY_OWNER);
      const owner = descriptor?.value;

      if (owner) {
        if (!isCompatibleOwner(owner, official)) return fail("owner_protocol_incompatible");
        if (owner.active.activationToken === activationToken &&
            owner.active.manifestSha256 === pin.manifestSha256 &&
            owner.active.adapterSha256 === pin.adapterSha256) {
          outcome = "already";
          phase = "complete";
          failureReason = null;
          logActivation(runtime, userRoot, owner.active, helperSha256, outcome, phase, failureReason);
          return { ok: true, code: "ALREADY_ACTIVE" };
        }
      } else if (!canInstallStableWrappers(official)) {
        return fail("official_exports_unpatchable");
      }

      phase = "load_adapter";
      const adapter = runtime.requireModule(verified.adapterPath);
      if (!validAdapter(adapter)) return fail("adapter_api_invalid");
      phase = "prepare_adapter";
      if (adapter.prepareDeviceKeyAccess() !== true) return fail("native_prepare_failed");

      phase = "recheck_helper_registration";
      const preparedHelperSha256 = readHelperIdentity(runtime, home);
      if (helperSha256 && preparedHelperSha256 && helperSha256 !== preparedHelperSha256) {
        return fail("helper_registration_changed");
      }
      helperSha256 = preparedHelperSha256 || helperSha256;

      const binding = createDeviceKeyBinding({
        activationToken, pin, manifest: verified.manifest, helperSha256, adapter, runtime, userRoot,
      });
      if (owner) {
        phase = "replace_binding";
        owner.active = binding;
        outcome = "replaced";
        phase = "complete";
        failureReason = null;
        logActivation(runtime, userRoot, binding, helperSha256, outcome, phase, failureReason);
        return { ok: true, code: "REPLACED" };
      }

      phase = "bind_official_exports";
      const installed = installStableWrappers(official, binding);
      if (!installed) return fail("official_exports_binding_failed");
      outcome = "installed";
      phase = "complete";
      failureReason = null;
      logActivation(runtime, userRoot, binding, helperSha256, outcome, phase, failureReason);
      return { ok: true, code: "INSTALLED" };
    } catch {
      failureReason = failureReasonForPhase(phase);
      return unavailable();
    }

    function fail(reason) {
      failureReason = reason;
      return unavailable();
    }

    function unavailable() {
      if (runtime) logActivation(runtime, runtime.path.join(runtime.os.homedir(), ".incodex"),
        { activationToken, adapterSha256: validPin(pin) ? pin.adapterSha256 : null },
        helperSha256, outcome, phase, failureReason);
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

function validActivationAction(action) {
  return typeof action === "string" && /^activate-device-key:[a-f0-9]{64}$/.test(action);
}

function activationTokenHash(action) {
  return validActivationAction(action) ? action.slice("activate-device-key:".length) : null;
}

function failureReasonForPhase(phase) {
  switch (phase) {
    case "load_runtime": return "runtime_load_failed";
    case "validate_activation": return "activation_validation_failed";
    case "read_helper_registration": return "helper_registration_read_failed";
    case "verify_adapter": return "adapter_verification_failed";
    case "load_official_exports": return "official_module_load_failed";
    case "inspect_official_exports": return "official_exports_inspection_failed";
    case "load_adapter": return "adapter_module_load_failed";
    case "prepare_adapter": return "native_prepare_failed";
    case "recheck_helper_registration": return "helper_registration_recheck_failed";
    case "replace_binding": return "binding_replacement_failed";
    case "bind_official_exports": return "official_exports_binding_failed";
    default: return "activation_failed";
  }
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
      !owner.originalDescriptors || !owner.wrappers) return false;
  if (typeof owner.active.invoke !== "function" || !validAdapter(owner.active.adapter)) return false;
  return METHOD_NAMES.every(name => {
    const descriptor = Object.getOwnPropertyDescriptor(official, name);
    return descriptor?.value === owner.wrappers[name] && descriptor.writable === true;
  });
}

function createDeviceKeyBinding({ activationToken, pin, manifest, helperSha256, adapter, runtime, userRoot }) {
  let binding;
  const record = (operation, ok, error) =>
    logOperation(runtime, userRoot, binding, helperSha256, operation, ok, error);
  binding = Object.freeze({
    protocol: 1,
    activationToken,
    manifestSha256: pin.manifestSha256,
    adapterSha256: pin.adapterSha256,
    abiVersion: manifest.abiVersion,
    helperSha256,
    adapter,
    invoke(name, original, receiver, args) {
      if (typeof args[0] !== "string" || !OSN_KEY_ID.test(args[0])) {
        return Reflect.apply(original, receiver, args);
      }
      return invokeDeviceOperation(binding, name, args, record);
    },
  });
  return binding;
}

function installStableWrappers(official, binding) {
  const originalDescriptors = Object.fromEntries(METHOD_NAMES.map(name =>
    [name, Object.getOwnPropertyDescriptor(official, name)]));
  const owner = {
    protocol: 1,
    active: binding,
    originalDescriptors,
    wrappers: Object.create(null),
  };
  for (const name of METHOD_NAMES) {
    const original = originalDescriptors[name].value;
    owner.wrappers[name] = function(...args) {
      const active = owner.active;
      return active.invoke(name, original, this, args);
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

function invokeDeviceOperation(binding, name, args, record) {
  const operation = name === "getDeviceKeyPublic" ? "public" : "sign";
  let result;
  try {
    result = Reflect.apply(binding.adapter[name], binding.adapter, args);
  } catch (error) {
    record(operation, false, error);
    throw error;
  }
  if (result && typeof result.then === "function") {
    return result.then(value => {
      record(operation, true, null);
      return value;
    }, error => {
      record(operation, false, error);
      throw error;
    });
  }
  record(operation, true, null);
  return result;
}

function logActivation(runtime, userRoot, binding, helperSha256, status, phase, failureReason) {
  const entry = {
    pid: runtime.process.pid,
    activationToken: activationTokenHash(binding?.activationToken),
    adapterSha256: validHash(binding?.adapterSha256) ? binding.adapterSha256 : null,
    helperSha256: validHash(helperSha256) ? helperSha256 : null,
    status,
    phase,
    failureReason,
  };
  appendLog(runtime, userRoot, "device-key-hot-activation", entry);
}

function logOperation(runtime, userRoot, binding, helperSha256, operation, ok, error) {
  const rawCode = error?.errorCode;
  const entry = {
    pid: runtime.process.pid,
    activationToken: activationTokenHash(binding?.activationToken),
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
