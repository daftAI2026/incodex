// @ts-nocheck
"use strict";
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { privateDirectory, privateFile } = require("./incodex-permission-native.cts");
const nativeName = "incodex-remote-key-compat.node";
const manifestName = "incodex-remote-key-manifest.json";
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
    // Loading the exact official module early preserves its original API/cache.
    // The native gate additionally checks the host and this module's signatures.
    const official = path.join(resourcesPath, "native/remote-control-device-key.node");
    load(official);
    return load(addon).install(official) === true;
  } catch {
    // Compatibility is optional: official startup and genuine errors survive.
    return false;
  }
}
export { installRemoteKeyCompatibility };
