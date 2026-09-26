import { afterEach, beforeEach, expect, test } from "bun:test";
import { dlopen, FFIType } from "bun:ffi";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { loadPermissionNativeLibrary, resolvePermissionNativeHostPath, resolvePermissionNativePath } from "./incodex-permission-native.cts";

const hash = (body: string | Buffer) => createHash("sha256").update(body).digest("hex");
const name = "incodex-permission-ui.dylib";
const hostName = "incodex-permission-host";
const manifestName = "runtime-native-manifest.json";
const processGenerationKey = Symbol.for("incodex.permission-native.process-generation.v1");

function resetLoaderGenerationForIsolatedTest() {
  const state = (globalThis as any)[processGenerationKey];
  if (state) state.generation = null;
}

beforeEach(resetLoaderGenerationForIsolatedTest);
afterEach(resetLoaderGenerationForIsolatedTest);

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

function dualFixture(run: (directory: string, manifest: any) => void) {
  const directory = mkdtempSync(join(realpathSync(tmpdir()), "incodex-native-dual-"));
  const bytes = Buffer.from([0xca, 0xfe, 0xba, 0xbe, 0, 255, 1]);
  const host = Buffer.from([0xca, 0xfe, 0xba, 0xbe, 2, 4, 8]);
  const manifest = { schemaVersion: 1, platform: "macos", abiVersion: 1,
    minimumMacOS: "12.0", architectures: ["arm64", "x86_64"], sourceSha256: "a".repeat(64),
    hostSourceSha256: "b".repeat(64),
    files: { [name]: hash(bytes), [hostName]: hash(host) } };
  writeFileSync(join(directory, name), bytes, { mode: 0o600 });
  writeFileSync(join(directory, hostName), host, { mode: 0o700 });
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

test("native permission resolves the optional executable host while retaining dylib compatibility", () => dualFixture((directory) => {
  expect(resolvePermissionNativePath(directory, "darwin")).toBe(join(directory, name));
  expect(resolvePermissionNativeHostPath(directory, "darwin")).toBe(join(directory, hostName));
  chmodSync(join(directory, hostName), 0o600);
  expect(() => resolvePermissionNativeHostPath(directory, "darwin")).toThrow("unsafe");
  chmodSync(join(directory, hostName), 0o755);
  expect(() => resolvePermissionNativeHostPath(directory, "darwin")).toThrow("unsafe");
}));

test("native library requires main thread and uses the verified path exactly once", () => fixture((directory) => {
  let mainThread = false;
  const loaded: string[] = [];
  const library = { IncodexPermissionFlightView: {}, IncodexPermissionArrowView: {} };
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

test("native library refuses a new dylib generation at the same path in one process", () => fixture((directory, manifest) => {
  const loaded: string[] = [];
  const firstLibrary = { generation: 1, IncodexPermissionFlightView: {}, IncodexPermissionArrowView: {} };
  const objc = { NobjcLibrary: new Proxy(function NobjcLibrary() {}, {
    construct(_target, [path]: [string]) {
      loaded.push(path);
      if (path.includes("Foundation.framework")) return { NSThread: { isMainThread: () => true } };
      return firstLibrary;
    }
  }) };

  expect(loadPermissionNativeLibrary(objc, directory, "darwin")).toBe(firstLibrary);

  const nextBytes = Buffer.from([0xca, 0xfe, 0xba, 0xbe, 1, 2, 3]);
  writeFileSync(join(directory, name), nextBytes, { mode: 0o600 });
  seal(directory, { ...manifest, sourceSha256: "b".repeat(64), files: { [name]: hash(nextBytes) } });

  expect(() => loadPermissionNativeLibrary(objc, directory, "darwin")).toThrow("generation changed; restart required");
  expect(loaded.filter(path => path === join(directory, name))).toHaveLength(1);
}));

test("failed native class lookup still locks the attempted process generation", () => fixture((directory, manifest) => {
  const firstLoads: string[] = [];
  const brokenObjc = {
    NobjcLibrary: new Proxy(function NobjcLibrary() {}, {
      construct(_target, [path]: [string]) {
        if (path.includes("Foundation.framework")) return { NSThread: { isMainThread: () => true } };
        firstLoads.push(path);
        return new Proxy({}, { get(_target, name) {
          if (name === "IncodexPermissionFlightView") return {};
          if (name === "IncodexPermissionArrowView") throw new Error("fixture bridge failed after image load");
          return undefined;
        } });
      },
    }),
  };
  expect(() => loadPermissionNativeLibrary(brokenObjc, directory, "darwin"))
    .toThrow("fixture bridge failed after image load");
  expect(firstLoads).toEqual([join(directory, name)]);

  const nextBytes = Buffer.from([0xca, 0xfe, 0xba, 0xbe, 4, 5, 6]);
  writeFileSync(join(directory, name), nextBytes, { mode: 0o600 });
  seal(directory, { ...manifest, sourceSha256: "c".repeat(64), files: { [name]: hash(nextBytes) } });
  const secondLoads: string[] = [];
  const nextObjc = { NobjcLibrary: new Proxy(function NobjcLibrary() {}, {
    construct(_target, [path]: [string]) {
      if (path.includes("Foundation.framework")) return { NSThread: { isMainThread: () => true } };
      secondLoads.push(path);
      return { IncodexPermissionFlightView: {}, IncodexPermissionArrowView: {} };
    },
  }) };
  expect(() => loadPermissionNativeLibrary(nextObjc, directory, "darwin"))
    .toThrow("generation changed; restart required");
  expect(secondLoads).toEqual([]);
}));

test("embedded loaders share the process cache and reject a second ObjC class generation", async () => {
  const root = mkdtempSync(join(realpathSync(tmpdir()), "incodex-native-cross-generation-"));
  const source = `
#import <Foundation/Foundation.h>
@interface IncodexPermissionFlightView : NSObject
+ (int)generation;
@end
@implementation IncodexPermissionFlightView
+ (int)generation { return INCODEX_FIXTURE_GENERATION; }
@end
@interface IncodexPermissionArrowView : NSObject
@end
@implementation IncodexPermissionArrowView
@end
int incodex_fixture_generation(void) { return [IncodexPermissionFlightView generation]; }
`;
  const loaded: string[] = [];
  const makeObjcWrapper = () => ({
    NobjcLibrary: new Proxy(function NobjcLibrary() {}, {
      construct(_target, [path]: [string]) {
        loaded.push(path);
        if (path.includes("Foundation.framework")) return { NSThread: { isMainThread: () => true } };
        const { symbols } = dlopen(path, {
          incodex_fixture_generation: { args: [], returns: FFIType.i32 },
        });
        const generation = symbols.incodex_fixture_generation();
        return { IncodexPermissionFlightView: {}, IncodexPermissionArrowView: {}, fixtureGeneration: generation };
      },
    }),
  });
  const objc = makeObjcWrapper();
  const anotherObjcWrapper = makeObjcWrapper();
  const crossGenerationWrapper = makeObjcWrapper();

  try {
    const loaderCopy = join(root, "incodex-permission-native-copy.cts");
    writeFileSync(loaderCopy, readFileSync(new URL("./incodex-permission-native.cts", import.meta.url)));
    const secondLoader = await import(pathToFileURL(loaderCopy).href);

    for (const [generation, directory] of [[1, "first"], [2, "second"]] as const) {
      const runtime = join(root, directory);
      mkdirSync(runtime, { mode: 0o700 });
      const dylib = join(runtime, name);
      const compiled = spawnSync("xcrun", [
        "clang", "-x", "objective-c", "-dynamiclib", "-fobjc-arc", "-framework", "Foundation",
        `-DINCODEX_FIXTURE_GENERATION=${generation}`, "-o", dylib, "-",
      ], { input: source, encoding: "utf8" });
      if (compiled.status !== 0) throw new Error(`ObjC fixture compile failed: ${compiled.stderr}`);
      const bytes = readFileSync(dylib);
      const manifest = {
        schemaVersion: 1, platform: "macos", abiVersion: 1, minimumMacOS: "12.0",
        architectures: ["arm64", "x86_64"], sourceSha256: String(generation).repeat(64),
        files: { [name]: hash(bytes) },
      };
      seal(runtime, manifest);
    }

    const first = loadPermissionNativeLibrary(objc, join(root, "first"), "darwin");
    expect(first.fixtureGeneration).toBe(1);
    expect(secondLoader.loadPermissionNativeLibrary(objc, join(root, "first"), "darwin")).toBe(first);
    expect(secondLoader.loadPermissionNativeLibrary(anotherObjcWrapper, join(root, "first"), "darwin").fixtureGeneration)
      .toBe(1);
    expect(() => secondLoader.loadPermissionNativeLibrary(crossGenerationWrapper, join(root, "second"), "darwin"))
      .toThrow("generation changed; restart required");
    expect(loaded.filter(path => path.endsWith(name))).toEqual([join(root, "first", name), join(root, "first", name)]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

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
