import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadPermissionNativeLibrary, resolvePermissionNativePath } from "./incodex-permission-native.cts";

const hash = (body: string | Buffer) => createHash("sha256").update(body).digest("hex");
const name = "incodex-permission-ui.dylib";
const manifestName = "runtime-native-manifest.json";

function fixture(run: (directory: string, manifest: any) => void) {
  const directory = mkdtempSync(join(realpathSync(tmpdir()), "incodex-native-loader-"));
  const bytes = Buffer.from([0xca, 0xfe, 0xba, 0xbe, 0, 255, 1]);
  const manifest = { schemaVersion: 1, platform: "macos", abiVersion: 1,
    minimumMacOS: "12.0", architectures: ["arm64", "x86_64"], sourceSha256: "a".repeat(64),
    files: { [name]: hash(bytes) } };
  writeFileSync(join(directory, name), bytes, { mode: 0o600 });
  seal(directory, manifest);
  try { run(directory, manifest); } finally { rmSync(directory, { recursive: true, force: true }); }
}

function seal(directory: string, manifest: any) {
  const body = JSON.stringify(manifest);
  writeFileSync(join(directory, manifestName), body, { mode: 0o600 });
  writeFileSync(join(directory, "runtime-manifest.json"), JSON.stringify({
    runtimeVersion: "1.0.1", sourceCommit: "", files: { ...manifest.files, [manifestName]: hash(body) },
  }), { mode: 0o600 });
}

test("native permission resolves only a sealed macOS runtime sibling", () => fixture((directory) => {
  expect(resolvePermissionNativePath(directory, "darwin")).toBe(join(directory, name));
  expect(() => resolvePermissionNativePath(directory, "win32")).toThrow("macOS");
}));

test("native permission rejects missing, altered or symlinked dylib before loading", () => fixture((directory) => {
  const file = join(directory, name);
  const bytes = readFileSync(file);
  writeFileSync(file, "tampered");
  expect(() => resolvePermissionNativePath(directory, "darwin")).toThrow();
  rmSync(file);
  expect(() => resolvePermissionNativePath(directory, "darwin")).toThrow();
  const other = join(directory, "other");
  writeFileSync(other, bytes);
  symlinkSync(other, file);
  expect(() => resolvePermissionNativePath(directory, "darwin")).toThrow("symlink");
}));

test("native permission manifest must agree with the release manifest and fixed ABI", () => fixture((directory, manifest) => {
  writeFileSync(join(directory, manifestName), JSON.stringify({ ...manifest, abiVersion: 99 }));
  expect(() => resolvePermissionNativePath(directory, "darwin")).toThrow();
  seal(directory, { ...manifest, abiVersion: 99 });
  expect(() => resolvePermissionNativePath(directory, "darwin")).toThrow("ABI");
  seal(directory, { ...manifest, files: { "../outside.dylib": "b".repeat(64) } });
  expect(() => resolvePermissionNativePath(directory, "darwin")).toThrow();
}));

test("native permission refuses symlink ancestry and writable native files", () => fixture((directory) => {
  chmodSync(join(directory, name), 0o666);
  expect(() => resolvePermissionNativePath(directory, "darwin")).toThrow();
  chmodSync(join(directory, name), 0o600);
  const alias = join(directory, "alias");
  symlinkSync(directory, alias);
  expect(() => resolvePermissionNativePath(alias, "darwin")).toThrow("symlink");
}));

test("native library requires main thread and uses the verified path exactly once", () => fixture((directory) => {
  let mainThread = false;
  const loaded: string[] = [];
  const library = { IncodexPermissionFlightView: {} };
  const objc = { NobjcLibrary: new Proxy(function NobjcLibrary() {}, {
    construct(_target, [path]: [string]) {
      loaded.push(path);
      if (path.includes("Foundation.framework")) return { NSThread: { isMainThread: () => mainThread } };
      return library;
    }
  }) };
  expect(() => loadPermissionNativeLibrary(objc, directory, "darwin")).toThrow("main thread");
  expect(loaded).not.toContain(join(directory, name));
  mainThread = true;
  expect(loadPermissionNativeLibrary(objc, directory, "darwin")).toBe(library);
  expect(loadPermissionNativeLibrary(objc, directory, "darwin")).toBe(library);
  expect(loaded.filter(path => path === join(directory, name))).toHaveLength(1);
}));

test("native library refuses a replaceable ancestor even when the release itself is private", () => fixture((directory) => {
  const release = join(directory, "release");
  mkdirSync(release, { mode: 0o700 });
  for (const file of [name, manifestName, "runtime-manifest.json"]) {
    copyFileSync(join(directory, file), join(release, file));
  }
  expect(resolvePermissionNativePath(release, "darwin")).toBe(join(release, name));
  chmodSync(directory, 0o777);
  try { expect(() => resolvePermissionNativePath(release, "darwin")).toThrow("ancestor"); }
  finally { chmodSync(directory, 0o700); }
}));
