import { describe, expect, test } from "bun:test";
import { lstatSync, mkdirSync, mkdtempSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { EXTERNAL_RUNTIME_FILES, verifyExternalRuntime } from "./external-runtime";
import {
  loadPackagedArtifacts,
  packagedRuntimeVersion,
  publishPackagedRuntime,
  resolvePackagedDistDir,
} from "./packaged-runtime";

const ASAR_FILES = [
  "incodex-loader.cjs",
  "incodex-inject.js",
  "incodex-main.cjs",
  "incodex-preload.cjs",
  "incodex-safe-home.cjs",
  "incodex-ipc-guard.cjs",
  "incodex-owner-core.cjs",
  "incodex-owner-recovery.cjs",
  "incodex-instance.cjs",
  "incodex-runtime-load.cjs",
  "incodex-window-kind.cjs",
] as const;

function writeIsolatedDist(version = "9.9.9"): string {
  const dist = join(mkdtempSync(join(tmpdir(), "incodex-packaged-")), "dist");
  mkdirSync(dist, { recursive: true });
  const files: Record<string, string> = {};
  for (const name of ASAR_FILES) {
    files[name] = `// ${name} v${version}\n`;
    writeFileSync(join(dist, name), files[name]);
  }
  writeFileSync(
    join(dist, "runtime-manifest.json"),
    `${JSON.stringify({ runtimeVersion: version, files: {} }, null, 2)}\n`,
  );
  return dist;
}

describe("packaged runtime", () => {
  test("honors INCODEX_DIST instead of the repo dist folder", () => {
    const dist = writeIsolatedDist("8.8.8");
    expect(resolvePackagedDistDir({ INCODEX_DIST: dist })).toBe(dist);
  });

  test("reads version from runtime-manifest.json without a package.json next to it", () => {
    const dist = writeIsolatedDist("3.2.1");
    expect(packagedRuntimeVersion(dist)).toBe("3.2.1");
  });

  test("loads asar artifacts from an isolated dist directory", () => {
    const dist = writeIsolatedDist("1.0.0");
    const artifacts = loadPackagedArtifacts(dist);
    expect(artifacts.loader).toContain("incodex-loader.cjs");
    expect(artifacts.main).toContain("incodex-main.cjs");
    expect(artifacts.inject).toContain("incodex-inject.js");
  });

  test("publishes ~/.incodex/runtime from that isolated dist", () => {
    const dist = writeIsolatedDist("4.0.0");
    const userRoot = mkdtempSync(join(tmpdir(), "incodex-home-"));
    const current = publishPackagedRuntime(userRoot, dist);
    expect(current.version).toBe("4.0.0");
    const verified = verifyExternalRuntime(userRoot);
    expect(verified.current.version).toBe("4.0.0");
    for (const name of EXTERNAL_RUNTIME_FILES) {
      expect(verified.current.files[name]).toMatch(/^[0-9a-f]{64}$/);
    }
  });

  test("second publish of the same files is a no-op", () => {
    const dist = writeIsolatedDist("4.1.0");
    const userRoot = mkdtempSync(join(tmpdir(), "incodex-home-"));
    const first = publishPackagedRuntime(userRoot, dist);
    const releaseDir = join(userRoot, "runtime", first.release);
    const ino = lstatSync(releaseDir).ino;
    const second = publishPackagedRuntime(userRoot, dist);
    expect(second).toEqual(first);
    expect(lstatSync(releaseDir).ino).toBe(ino);
  });

  test("missing dist file names the file and does not mention bun", () => {
    const dist = writeIsolatedDist("1.0.0");
    unlinkSync(join(dist, "incodex-main.cjs"));
    expect(() => loadPackagedArtifacts(dist)).toThrow(/incodex-main\.cjs/);
    expect(() => loadPackagedArtifacts(dist)).not.toThrow(/bun|build-runtime/);
  });
});
