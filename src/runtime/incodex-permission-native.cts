// @ts-nocheck
"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const NATIVE_NAME = "incodex-permission-ui.dylib";
const NATIVE_MANIFEST = "runtime-native-manifest.json";
const libraries = new WeakMap();
const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
const validHash = value => typeof value === "string" && /^[a-f0-9]{64}$/.test(value);

function privateFile(file, maximumSize) {
  const stat = fs.lstatSync(file);
  if (stat.isSymbolicLink()) throw new Error("Native permission refuses symlink file");
  if (!stat.isFile() || stat.size > maximumSize || stat.uid !== process.getuid() || (stat.mode & 0o022)) {
    throw new Error("Native permission file has unsafe ownership, mode or size");
  }
  return fs.readFileSync(file);
}

function privateDirectory(directory) {
  if (!path.isAbsolute(directory)) throw new Error("Native permission requires an absolute runtime directory");
  let current = path.resolve(directory);
  const leaf = current;
  for (;;) {
    const stat = fs.lstatSync(current);
    if (stat.isSymbolicLink()) throw new Error("Native permission refuses symlink ancestry");
    if (!stat.isDirectory()) throw new Error("Native permission ancestry is not a directory");
    if (current === leaf && (stat.uid !== process.getuid() || (stat.mode & 0o022))) {
      throw new Error("Native permission runtime directory is not private");
    }
    const parent = path.dirname(current);
    if (parent === current) break;
    current = parent;
  }
  return leaf;
}

// The library is always a sibling of this verified Runtime, never a path from
// environment variables or the permissions UI. Older installed loaders still
// verify the shared JS files; this gate verifies the additional native bytes.
function resolvePermissionNativePath(directory = __dirname, platform = process.platform) {
  if (platform !== "darwin") throw new Error("Native permission UI is macOS only");
  directory = privateDirectory(directory);
  const runtime = JSON.parse(privateFile(path.join(directory, "runtime-manifest.json"), 1024 * 1024));
  const nativeBytes = privateFile(path.join(directory, NATIVE_MANIFEST), 64 * 1024);
  if (!validHash(runtime?.files?.[NATIVE_MANIFEST]) || hash(nativeBytes) !== runtime.files[NATIVE_MANIFEST]) {
    throw new Error("Native permission manifest hash mismatch");
  }
  const manifest = JSON.parse(nativeBytes);
  if (manifest?.schemaVersion !== 1 || manifest.platform !== "macos" || manifest.abiVersion !== 1 ||
      manifest.minimumMacOS !== "12.0" || !validHash(manifest.sourceSha256) ||
      !Array.isArray(manifest.architectures) || manifest.architectures.length !== 2 ||
      !manifest.architectures.includes("arm64") || !manifest.architectures.includes("x86_64")) {
    throw new Error("Native permission manifest ABI is incompatible");
  }
  if (!manifest.files || Object.keys(manifest.files).length !== 1 || !validHash(manifest.files[NATIVE_NAME]) ||
      manifest.files[NATIVE_NAME] !== runtime.files[NATIVE_NAME]) {
    throw new Error("Native permission artifact manifest mismatch");
  }
  const file = path.join(directory, NATIVE_NAME);
  if (hash(privateFile(file, 16 * 1024 * 1024)) !== manifest.files[NATIVE_NAME]) {
    throw new Error("Native permission dylib hash mismatch");
  }
  return file;
}

function loadPermissionNativeLibrary(objc, directory = __dirname, platform = process.platform) {
  const file = resolvePermissionNativePath(directory, platform);
  const foundation = new objc.NobjcLibrary("/System/Library/Frameworks/Foundation.framework/Foundation");
  if (!foundation.NSThread.isMainThread()) throw new Error("Native permission UI requires the main thread");
  const cached = libraries.get(objc);
  if (cached) {
    // Objective-C class names are process-global. Never load two generations
    // into one process and accidentally resolve the previous Swift class.
    if (cached.file !== file) throw new Error("Native permission library generation changed; restart required");
    return cached.library;
  }
  const library = new objc.NobjcLibrary(file);
  if (!library.IncodexPermissionFlightView) throw new Error("Native permission flight class is missing");
  libraries.set(objc, { file, library });
  return library;
}

export { loadPermissionNativeLibrary, resolvePermissionNativePath };
