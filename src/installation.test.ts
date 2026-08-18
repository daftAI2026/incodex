import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  canRestoreInstallation,
  loadCurrentInstallation,
  parseInstallManifest,
  targetId,
  loadSigningManifest,
  writeInstallation,
  type InstallManifest,
} from "./installation";

function manifest(over: Partial<InstallManifest> = {}): InstallManifest {
  return {
    schemaVersion: 1,
    installId: "install-a",
    targetRealPath: "/Applications/ChatGPT.app",
    bundleIdentifier: "com.openai.codex",
    appVersion: "1.0.0",
    appBuild: "100",
    architecture: "arm64",
    originalAsarHeaderHash: "header-orig",
    originalAsarFileHash: "file-orig",
    originalPlistFileHash: "plist-orig",
    patchedAsarHeaderHash: "header-patched",
    patchedAsarFileHash: "file-patched",
    originalMain: "index.js",
    runtimeVersion: "0.1.0",
    createdAt: "2026-08-18T00:00:00.000Z",
    transactionState: "committed",
    ...over,
  };
}

describe("target identity", () => {
  test("clone, live, and a custom path do not share a target id", () => {
    const official = targetId("/Applications/ChatGPT.app");
    const clone = targetId("/Users/me/.incodex/scratch/ChatGPT.app");
    const custom = targetId("/opt/Codex.app");
    expect(official.startsWith("official-")).toBe(true);
    expect(clone.startsWith("app-")).toBe(true);
    expect(new Set([official, clone, custom]).size).toBe(3);
  });

  test("the same real path always maps to the same target id", () => {
    expect(targetId("/Applications/ChatGPT.app")).toBe(targetId("/Applications/../Applications/ChatGPT.app"));
  });
});

describe("manifest schema", () => {
  test("rejects a header-hash-only record that is missing file hashes", () => {
    const raw = manifest();
    delete (raw as { originalAsarFileHash?: string }).originalAsarFileHash;
    expect(parseInstallManifest(raw)).toBeNull();
    expect(parseInstallManifest(manifest())).not.toBeNull();
  });
});

describe("restore verification", () => {
  const current = {
    targetRealPath: "/Applications/ChatGPT.app",
    currentInstallId: "install-a",
    currentAppBuild: "100",
    currentAsarFileHash: "file-patched",
    currentOriginalMain: "index.js",
    manifest: manifest(),
  };

  test("allows restore only when target, install ID, build, and patched file hash match", () => {
    expect(canRestoreInstallation(current)).toEqual({ ok: true });
  });

  test("refuses a backup that belongs to a different target", () => {
    const decision = canRestoreInstallation({
      ...current,
      targetRealPath: "/tmp/ChatGPT.app",
    });
    expect(decision.ok).toBe(false);
    if (decision.ok) throw new Error("expected refuse");
    expect(decision.reason).toContain("does not belong");
  });

  test("refuses when the current patched ASAR hash does not match", () => {
    const decision = canRestoreInstallation({
      ...current,
      currentAsarFileHash: "someone-else",
    });
    expect(decision.ok).toBe(false);
    if (decision.ok) throw new Error("expected refuse");
    expect(decision.advice).toContain("refusing to guess");
  });

  test("refuses when the install ID or build do not match", () => {
    expect(canRestoreInstallation({ ...current, currentInstallId: "other" }).ok).toBe(false);
    expect(canRestoreInstallation({ ...current, currentAppBuild: "200" }).ok).toBe(false);
    expect(canRestoreInstallation({ ...current, currentOriginalMain: "other.js" }).ok).toBe(false);
  });
});

describe("immutable per-target store", () => {
  test("two targets keep separate current pointers and originals", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-inst-"));
    const official = "/Applications/ChatGPT.app";
    const clone = join(root, "scratch", "ChatGPT.app");
    writeInstallation({
      root,
      appPath: official,
      manifest: manifest({ installId: "live-1" }),
      runtime: runtime("live-1"),
    });
    writeInstallation({
      root,
      appPath: clone,
      manifest: manifest({
        installId: "clone-1",
        targetRealPath: clone,
        patchedAsarFileHash: "clone-patched",
      }),
      runtime: runtime("clone-1", "clone-patched"),
    });

    expect(loadCurrentInstallation(official, root)?.manifest.installId).toBe("live-1");
    expect(loadCurrentInstallation(clone, root)?.manifest.installId).toBe("clone-1");
    expect(loadCurrentInstallation(official, root)?.manifest.patchedAsarFileHash).toBe("file-patched");
    expect(loadCurrentInstallation(clone, root)?.manifest.patchedAsarFileHash).toBe("clone-patched");
  });

  test("reinstalling the same target creates a new install dir and leaves the old original intact", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-inst-"));
    const appPath = "/Applications/ChatGPT.app";
    const first = writeInstallation({
      root,
      appPath,
      manifest: manifest({ installId: "first" }),
      runtime: runtime("first"),
    });
    const marker = join(first, "original", "keep-me.txt");
    mkdirSync(join(first, "original"), { recursive: true });
    writeFileSync(marker, "original-a");

    writeInstallation({
      root,
      appPath,
      manifest: manifest({ installId: "second", patchedAsarFileHash: "file-patched-2" }),
      runtime: runtime("second", "file-patched-2"),
    });

    expect(readFileSync(marker, "utf8")).toBe("original-a");
    expect(loadCurrentInstallation(appPath, root)?.manifest.installId).toBe("second");
    expect(() =>
      writeInstallation({
        root,
        appPath,
        manifest: manifest({ installId: "first" }),
        runtime: runtime("first"),
      }),
    ).toThrow(/immutable/);
  });

  test("persists a signing manifest next to the runtime manifest", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-inst-"));
    const appPath = "/Applications/ChatGPT.app";
    const dir = writeInstallation({
      root,
      appPath,
      manifest: manifest({ installId: "signed-1" }),
      runtime: runtime("signed-1"),
      signing: {
        schemaVersion: 1,
        appPath,
        verified: true,
        spctl: { status: 3, output: "rejected", accepted: false, usedAsSuccessGate: false },
        components: [],
        observations: [],
        unretainableEntitlements: [
          {
            relativePath: ".",
            keys: ["com.apple.developer.team-identifier"],
            reason: "adhoc identity cannot legally retain team-bound entitlements",
          },
        ],
      },
    });
    const stored = loadSigningManifest(dir);
    expect(stored?.verified).toBe(true);
    expect(stored?.spctl.usedAsSuccessGate).toBe(false);
    expect(stored?.unretainableEntitlements[0]?.keys).toContain("com.apple.developer.team-identifier");
  });
});

function runtime(installId: string, patchedAsarFileHash = "file-patched") {
  return {
    installId,
    originalMain: "index.js",
    patchedAsarHeaderHash: "header-patched",
    patchedAsarFileHash,
  };
}
