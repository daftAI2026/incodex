import { describe, expect, test } from "bun:test";
import { createPackageWithOptions } from "@electron/asar";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileSha256 } from "./app-identity";
import { ASAR_REL, PLIST_REL } from "./paths";
import { applyRecovery } from "./recover";
import { canRestoreInstallation, originalAppPath } from "./installation";
import {
  copyBundle,
  defaultCloneDeps,
  depsWithFault,
  runCloneInstall,
  type CloneInstallDeps,
  type InstallFault,
} from "./install-transaction";
import { loadJournal } from "./transaction";

const MARKER = "Contents/MacOS/ChatGPT";

type Case = {
  name: string;
  fault: InstallFault;
  expectBackup: "absent" | "intact";
  expectAction: "rollback" | "none";
};

const cases: Case[] = [
  { name: "plist at discover", fault: { phase: "DISCOVERED", kind: "plist" }, expectBackup: "absent", expectAction: "none" },
  { name: "permission at discover", fault: { phase: "DISCOVERED", kind: "permission-denied" }, expectBackup: "absent", expectAction: "none" },
  { name: "kill at discover", fault: { phase: "DISCOVERED", kind: "kill" }, expectBackup: "absent", expectAction: "none" },
  { name: "ditto at backup", fault: { phase: "BACKUP_COMMITTED", kind: "ditto" }, expectBackup: "absent", expectAction: "rollback" },
  { name: "disk full at backup", fault: { phase: "BACKUP_COMMITTED", kind: "disk-full" }, expectBackup: "absent", expectAction: "rollback" },
  { name: "permission at backup", fault: { phase: "BACKUP_COMMITTED", kind: "permission-denied" }, expectBackup: "absent", expectAction: "rollback" },
  { name: "kill at backup", fault: { phase: "BACKUP_COMMITTED", kind: "kill" }, expectBackup: "absent", expectAction: "rollback" },
  { name: "ditto at stage", fault: { phase: "STAGED", kind: "ditto" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "disk full at stage", fault: { phase: "STAGED", kind: "disk-full" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "permission at stage", fault: { phase: "STAGED", kind: "permission-denied" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "kill at stage", fault: { phase: "STAGED", kind: "kill" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "disk full at patch", fault: { phase: "PATCHED", kind: "disk-full" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "permission at patch", fault: { phase: "PATCHED", kind: "permission-denied" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "kill at patch", fault: { phase: "PATCHED", kind: "kill" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "codesign at sign", fault: { phase: "SIGNED", kind: "codesign" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "permission at sign", fault: { phase: "SIGNED", kind: "permission-denied" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "kill at sign", fault: { phase: "SIGNED", kind: "kill" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "verify at verified", fault: { phase: "VERIFIED", kind: "verify" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "kill at verified", fault: { phase: "VERIFIED", kind: "kill" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "process still running", fault: { phase: "SWAPPED", kind: "process-running" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "rename at swap", fault: { phase: "SWAPPED", kind: "rename" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "kill at swap", fault: { phase: "SWAPPED", kind: "kill" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "rollback rename", fault: { phase: "SWAPPED", kind: "rollback-rename" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "verify after swap", fault: { phase: "TARGET_VERIFIED", kind: "verify" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "state write interrupt", fault: { phase: "COMMITTED", kind: "state-write" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "disk full at commit", fault: { phase: "COMMITTED", kind: "disk-full" }, expectBackup: "intact", expectAction: "rollback" },
  { name: "kill at commit", fault: { phase: "COMMITTED", kind: "kill" }, expectBackup: "intact", expectAction: "rollback" },
];

describe("install transaction fault injection", () => {
  test.each(cases)("$name leaves the target launchable and never auto-restores the wrong backup", async (item) => {
    const world = await setupWorld();
    const deps = depsWithFault(world.deps, item.fault);
    let thrown: unknown;
    try {
      await runCloneInstall(world.app, {
        root: world.root,
        runtimeVersion: "0.1.0",
        patch: world.deps.patch,
        deps,
      });
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeDefined();

    const journal = loadJournal(world.guessInstallId(world.root, world.app), world.root);
    if (item.expectAction === "none") {
      expect(journal).toBeNull();
      expect(existsSync(world.backupHint)).toBe(false);
      expect(readFileSync(join(world.app, MARKER), "utf8").trim()).toBe("ORIGINAL");
      return;
    }

    const recovered = applyRecovery(journal!, world.root);
    expect(recovered.action).toBe(item.expectAction);
    expect(recovered.targetUntouched).toBe(true);
    expect(existsSync(join(world.app, MARKER))).toBe(true);
    expect(readFileSync(join(world.app, MARKER), "utf8").trim()).toBe("ORIGINAL");
    expect(recovered.journal.phase).toBe("ROLLED_BACK");
    if (item.expectBackup === "intact") {
      expect(recovered.backupIntact).toBe(true);
      expect(readFileSync(join(journal!.originalSnapshot, MARKER), "utf8").trim()).toBe("ORIGINAL");
    }
    expect(
      canRestoreInstallation({
        targetRealPath: world.app,
        currentInstallId: null,
        currentAppBuild: "1",
        currentAsarFileHash: fileSha256(join(world.app, ASAR_REL)),
        currentOriginalMain: "index.js",
        manifest: {
          schemaVersion: 1,
          installId: journal!.installId,
          targetRealPath: world.app,
          bundleIdentifier: "com.test.codex",
          appVersion: "1.0.0",
          appBuild: "1",
          architecture: "arm64",
          originalAsarHeaderHash: "orig-header",
          originalAsarFileHash: "orig-file",
          originalPlistFileHash: "orig-plist",
          patchedAsarHeaderHash: "patched-header",
          patchedAsarFileHash: "patched-file",
          originalMain: "index.js",
          runtimeVersion: "0.1.0",
          createdAt: "now",
          transactionState: "committed",
        },
      }).ok,
    ).toBe(false);
  });

  test("a successful install can still refuse a backup from another target", async () => {
    const world = await setupWorld();
    await runCloneInstall(world.app, {
      root: world.root,
      runtimeVersion: "0.1.0",
      patch: world.deps.patch,
      deps: world.deps,
    });
    expect(readFileSync(join(world.app, MARKER), "utf8").trim()).toBe("PATCHED");
    expect(
      canRestoreInstallation({
        targetRealPath: join(world.root, "other.app"),
        currentInstallId: "nope",
        currentAppBuild: "1",
        currentAsarFileHash: "x",
        currentOriginalMain: "index.js",
        manifest: {
          schemaVersion: 1,
          installId: "nope",
          targetRealPath: world.app,
          bundleIdentifier: "com.test.codex",
          appVersion: "1.0.0",
          appBuild: "1",
          architecture: "arm64",
          originalAsarHeaderHash: "a",
          originalAsarFileHash: "b",
          originalPlistFileHash: "c",
          patchedAsarHeaderHash: "d",
          patchedAsarFileHash: "e",
          originalMain: "index.js",
          runtimeVersion: "0.1.0",
          createdAt: "now",
          transactionState: "committed",
        },
      }).ok,
    ).toBe(false);
  });
});

async function setupWorld(): Promise<{
  root: string;
  app: string;
  backupHint: string;
  deps: CloneInstallDeps;
  guessInstallId: (root: string, app: string) => string;
}> {
  const root = mkdtempSync(join(tmpdir(), "incodex-fault-"));
  const app = join(root, "ChatGPT.app");
  await writeFakeApp(app, "ORIGINAL");
  const identity = {
    bundleIdentifier: "com.test.codex",
    appVersion: "1.0.0",
    appBuild: "1",
    architecture: "arm64",
    asarFileHash: fileSha256(join(app, ASAR_REL)),
    plistFileHash: fileSha256(join(app, PLIST_REL)),
  };
  const deps: CloneInstallDeps = {
    ...defaultCloneDeps,
    inspect: (path) => ({
      path,
      exists: true,
      asarExists: true,
      patched: readFileSync(join(path, MARKER), "utf8").includes("PATCHED"),
      installId: null,
      listing: {
        bundleIdentifier: identity.bundleIdentifier,
        appVersion: identity.appVersion,
        appBuild: identity.appBuild,
        architecture: identity.architecture,
      },
      identity: {
        ...identity,
        asarFileHash: fileSha256(join(path, ASAR_REL)),
        plistFileHash: fileSha256(join(path, PLIST_REL)),
      },
      originalMain: "index.js",
    }),
    snapshot: copyBundle,
    copyBundle,
    patch: async (staged) => {
      writeFileSync(join(staged, MARKER), "PATCHED\n");
      return { originalMain: "index.js", hash: "patched-header" };
    },
    sign: () => undefined,
    verify: () => true,
    targetRunning: () => false,
    writeState: defaultCloneDeps.writeState,
    writeInstall: defaultCloneDeps.writeInstall,
    swap: defaultCloneDeps.swap,
  };
  return {
    root,
    app,
    backupHint: originalAppPath(app, "unknown", root),
    deps,
    guessInstallId: (userRoot) => {
      const dir = join(userRoot, "transactions");
      if (!existsSync(dir)) return "missing";
      const name = readdirSync(dir).find((file) => file.endsWith(".json") && !file.endsWith(".tmp"));
      return name ? name.replace(/\.json$/, "") : "missing";
    },
  };
}

async function writeFakeApp(appPath: string, marker: string): Promise<void> {
  mkdirSync(join(appPath, "Contents/MacOS"), { recursive: true });
  mkdirSync(join(appPath, "Contents/Resources"), { recursive: true });
  writeFileSync(join(appPath, MARKER), `${marker}\n`);
  writeFileSync(
    join(appPath, PLIST_REL),
    `<?xml version="1.0"?><plist><dict><key>CFBundleIdentifier</key><string>com.test.codex</string></dict></plist>\n`,
  );
  const src = join(appPath, "../asar-src");
  mkdirSync(src, { recursive: true });
  writeFileSync(join(src, "package.json"), `${JSON.stringify({ main: "index.js" })}\n`);
  writeFileSync(join(src, "index.js"), "module.exports = 1\n");
  await createPackageWithOptions(src, join(appPath, ASAR_REL), {});
}
