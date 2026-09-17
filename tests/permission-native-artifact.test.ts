import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { expect, test } from "bun:test";

const repositoryRoot = join(import.meta.dir, "..");
const nativeRoot = join(repositoryRoot, "native", "macos");
const sourcePath = join(nativeRoot, "permission-views.swift");
const distRoot = join(nativeRoot, "dist");
const dylibName = "incodex-permission-ui.dylib";
const dylibPath = join(distRoot, dylibName);
const manifestPath = join(distRoot, "runtime-native-manifest.json");
const abiSmokeSource = join(repositoryRoot, "tests", "native", "permission-views-abi-smoke.m");

type NativeManifest = {
  schemaVersion: number;
  platform: string;
  abiVersion: number;
  minimumMacOS: string;
  architectures: string[];
  sourceSha256: string;
  files: Record<string, string>;
};

function sha256(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function command(name: string, args: string[]) {
  const result = spawnSync(name, args, { encoding: "utf8" });
  return {
    status: result.status,
    output: `${result.stdout ?? ""}${result.stderr ?? ""}`,
  };
}

function requirePublishedFiles(): NativeManifest | null {
  expect(existsSync(sourcePath)).toBe(true);
  expect(existsSync(dylibPath)).toBe(true);
  expect(existsSync(manifestPath)).toBe(true);
  if (!existsSync(dylibPath) || !existsSync(manifestPath)) return null;

  return JSON.parse(readFileSync(manifestPath, "utf8")) as NativeManifest;
}

test.skipIf(process.platform !== "darwin")(
  "publishes a macOS-only universal permission native artifact with a bound manifest",
  () => {
    const manifest = requirePublishedFiles();
    if (!manifest) return;

    expect(manifest).toMatchObject({
      schemaVersion: 1,
      platform: "macos",
      abiVersion: 1,
      minimumMacOS: "12.0",
      architectures: ["arm64", "x86_64"],
    });
    expect(manifest.sourceSha256).toMatch(/^[0-9a-f]{64}$/);
    expect(manifest.sourceSha256).toBe(sha256(sourcePath));
    expect(manifest.files).toEqual({ [dylibName]: expect.any(String) });
    expect(manifest.files[dylibName]).toMatch(/^[0-9a-f]{64}$/);
    expect(manifest.files[dylibName]).toBe(sha256(dylibPath));
  },
);

test.skipIf(process.platform !== "darwin")(
  "contains arm64 and x86_64 slices built for macOS 12",
  () => {
    const manifest = requirePublishedFiles();
    if (!manifest) return;

    const lipoInfo = command("lipo", ["-info", dylibPath]);
    expect(lipoInfo.status).toBe(0);
    expect(lipoInfo.output).toContain("arm64");
    expect(lipoInfo.output).toContain("x86_64");

    for (const architecture of ["arm64", "x86_64"]) {
      const verified = command("lipo", [dylibPath, "-verify_arch", architecture]);
      expect(verified.status).toBe(0);
    }

    const loadCommands = command("otool", ["-l", dylibPath]);
    expect(loadCommands.status).toBe(0);
    expect((loadCommands.output.match(/minos\s+12\.0/g) ?? []).length).toBeGreaterThanOrEqual(2);
  },
);

test.skipIf(process.platform !== "darwin")(
  "has a verifiable ad-hoc code signature",
  () => {
    const manifest = requirePublishedFiles();
    if (!manifest) return;

    const verification = command("codesign", ["--verify", "--strict", "--", dylibPath]);
    expect(verification.status).toBe(0);

    const details = command("codesign", ["-dvvv", "--", dylibPath]);
    expect(details.status).toBe(0);
    expect(details.output).toMatch(/Signature=adhoc/);
  },
);

test.skipIf(process.platform !== "darwin")(
  "exports the public Objective-C flight-view class and selectors",
  () => {
    const manifest = requirePublishedFiles();
    if (!manifest) return;

    const symbols = command("nm", ["-gU", dylibPath]);
    expect(symbols.status).toBe(0);
    for (const className of [
      "IncodexPermissionFlightView",
      "IncodexPermissionInitialView",
      "IncodexPermissionHelperView",
    ]) {
      expect(symbols.output).toContain(className);
    }

    const strings = command("strings", [dylibPath]);
    expect(strings.status).toBe(0);
    expect(strings.output).toContain("setSourceImage:targetImage:");
    expect(strings.output).toContain("updateProgress:cornerRadius:reduceTransparency:");
    expect(strings.output).toContain("configureWithCopy:appIcon:permissionIcon:actionTarget:");
    expect(strings.output).toContain("setContentWithTitle:body:allowEnabled:settingsPlaceholder:");
    expect(strings.output).toContain("configureWithCopy:appIcon:actionTarget:");
  },
);

test.skipIf(process.platform !== "darwin")(
  "loads the committed dylib and exercises its Objective-C ABI on the host architecture",
  () => {
    const directory = mkdtempSync(join(tmpdir(), "incodex-permission-abi-"));
    try {
      const architecture = process.arch === "arm64" ? "arm64" : process.arch === "x64" ? "x86_64" : "";
      expect(architecture).not.toBe("");
      if (!architecture) return;

      const executable = join(directory, "permission-views-abi-smoke");
      const build = command("/usr/bin/clang", [
        "-arch", architecture,
        "-fobjc-arc",
        "-framework", "Cocoa",
        abiSmokeSource,
        "-o", executable,
      ]);
      expect(build.status, build.output).toBe(0);
      if (build.status !== 0) return;

      const run = spawnSync(executable, [dylibPath], {
        cwd: repositoryRoot,
        encoding: "utf8",
        timeout: 30_000,
      });
      const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
      expect(run.status, output || String(run.error ?? "Objective-C ABI smoke failed")).toBe(0);
      expect(output).toContain("permission native Objective-C ABI smoke passed");
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
  90_000,
);
