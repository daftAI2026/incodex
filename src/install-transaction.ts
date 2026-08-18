import { spawnSync } from "node:child_process";
import { existsSync, renameSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { randomUUID } from "node:crypto";
import type { AppInspection } from "./app-identity";
import { fileSha256, inspectApp } from "./app-identity";
import { ensureDir, headerHash } from "./asar";
import { signApp, verifyApp } from "./codesign";
import {
  manifestFromIdentity,
  originalAppPath,
  snapshotOriginalApp,
  writeInstallation,
} from "./installation";
import { ASAR_REL, USER_ROOT } from "./paths";
import { saveState } from "./state";
import { swapBundle, type SwapOps } from "./swap";
import { advanceJournal, writeJournal, type Journal, type Phase } from "./transaction";

export type InstallFaultKind =
  | "disk-full"
  | "permission-denied"
  | "ditto"
  | "plist"
  | "codesign"
  | "verify"
  | "process-running"
  | "rename"
  | "rollback-rename"
  | "state-write"
  | "kill";

export type InstallFault = {
  phase: Phase | "SWAP" | "ROLLBACK" | "STATE";
  kind: InstallFaultKind;
};

export type CloneInstallDeps = {
  inspect: (appPath: string) => AppInspection;
  copyBundle: (src: string, dest: string) => void;
  snapshot: (src: string, dest: string) => void;
  patch: (stagedApp: string, installId: string) => Promise<{ originalMain: string; hash: string }>;
  sign: (appPath: string) => void;
  verify: (appPath: string) => boolean;
  targetRunning: (appPath: string) => boolean;
  swap: (stagedApp: string, targetApp: string, ops?: SwapOps) => void;
  writeState: (state: Parameters<typeof saveState>[0], root: string) => void;
  writeInstall: typeof writeInstallation;
};

export const defaultCloneDeps: CloneInstallDeps = {
  inspect: inspectApp,
  copyBundle,
  snapshot: snapshotOriginalApp,
  patch: async () => {
    throw new Error("patch must be provided by install()");
  },
  sign: signApp,
  verify: verifyApp,
  targetRunning: appIsRunning,
  swap: swapBundle,
  writeState: saveState,
  writeInstall: writeInstallation,
};

export function copyBundle(src: string, dest: string): void {
  if (existsSync(dest)) rmSync(dest, { recursive: true, force: true });
  ensureDir(dirname(dest));
  const copied = spawnSync("ditto", [src, dest], { encoding: "utf8" });
  if (copied.status !== 0) throw new Error(copied.stderr || "failed to stage app bundle");
}

export function appIsRunning(appPath: string): boolean {
  const listed = spawnSync("ps", ["-ax", "-o", "pid=,command="], { encoding: "utf8" });
  const needle = `${resolve(appPath)}/Contents/MacOS/`;
  return (listed.stdout || "").split("\n").some((line) => line.includes(needle));
}

export function ioFault(kind: "disk-full" | "permission-denied" | "ditto" | "plist" | "codesign" | "verify" | "process-running" | "rename" | "rollback-rename" | "state-write" | "kill"): Error {
  const err = new Error(kind) as NodeJS.ErrnoException;
  if (kind === "disk-full") err.code = "ENOSPC";
  if (kind === "permission-denied") err.code = "EACCES";
  return err;
}

export function depsWithFault(base: CloneInstallDeps, fault: InstallFault): CloneInstallDeps {
  const fail = () => {
    throw ioFault(fault.kind);
  };
  const hit = (phase: InstallFault["phase"], ...kinds: InstallFaultKind[]) =>
    fault.phase === phase && kinds.includes(fault.kind);
  let verifyCount = 0;

  return {
    inspect: (appPath) => {
      if (hit("DISCOVERED", "plist", "permission-denied", "kill")) fail();
      return base.inspect(appPath);
    },
    snapshot: (src, dest) => {
      if (hit("BACKUP_COMMITTED", "ditto", "disk-full", "permission-denied", "kill")) fail();
      base.snapshot(src, dest);
    },
    copyBundle: (src, dest) => {
      if (hit("STAGED", "ditto", "disk-full", "permission-denied", "kill")) fail();
      base.copyBundle(src, dest);
    },
    patch: async (stagedApp, installId) => {
      if (hit("PATCHED", "disk-full", "permission-denied", "kill")) fail();
      return base.patch(stagedApp, installId);
    },
    sign: (appPath) => {
      if (hit("SIGNED", "codesign", "permission-denied", "kill")) fail();
      base.sign(appPath);
    },
    verify: (appPath) => {
      verifyCount += 1;
      if (hit("VERIFIED", "verify", "kill") && verifyCount === 1) return false;
      if (hit("TARGET_VERIFIED", "verify", "kill") && verifyCount >= 2) return false;
      return base.verify(appPath);
    },
    targetRunning: (appPath) => {
      if (hit("SWAPPED", "process-running")) return true;
      return base.targetRunning(appPath);
    },
    swap: (staged, target, ops) => {
      if (hit("SWAPPED", "kill", "rename")) fail();
      if (hit("ROLLBACK", "rollback-rename") || hit("SWAPPED", "rollback-rename")) {
        let renames = 0;
        base.swap(staged, target, {
          rename: (from, to) => {
            renames += 1;
            if (renames >= 2) throw ioFault("rollback-rename");
            renameSync(from, to);
          },
          remove: (path) => rmSync(path, { recursive: true, force: true }),
        });
        return;
      }
      base.swap(staged, target, ops);
    },
    writeState: (state, root) => {
      if (hit("COMMITTED", "state-write", "disk-full", "kill") || hit("STATE", "state-write", "disk-full", "kill")) {
        fail();
      }
      base.writeState(state, root);
    },
    writeInstall: (options) => {
      if (hit("COMMITTED", "state-write", "disk-full") || hit("STATE", "state-write", "disk-full")) fail();
      return base.writeInstall(options);
    },
  };
}

export async function runCloneInstall(
  appPath: string,
  options: {
    root?: string;
    deps?: Partial<CloneInstallDeps>;
    runtimeVersion: string;
    patch: CloneInstallDeps["patch"];
  },
): Promise<{ installId: string; journal: Journal }> {
  const root = options.root ?? USER_ROOT;
  const deps: CloneInstallDeps = {
    ...defaultCloneDeps,
    patch: options.patch,
    ...options.deps,
  };
  const info = deps.inspect(appPath);
  const identity = info.identity;
  if (!identity) throw new Error(`could not read app identity: ${appPath}`);

  const installId = randomUUID();
  const originalDest = originalAppPath(appPath, installId, root);
  const stagedApp = join(root, "scratch", `ChatGPT.app.staged-${installId}`);
  let journal: Journal = {
    schemaVersion: 1,
    installId,
    targetRealPath: resolve(appPath),
    stagedApp,
    originalSnapshot: originalDest,
    phase: "DISCOVERED",
    updatedAt: new Date().toISOString(),
  };
  writeJournal(journal, root);
  const before = headerHash(join(appPath, ASAR_REL));
  try {
    deps.snapshot(appPath, originalDest);
    journal = advanceJournal(journal, "BACKUP_COMMITTED", root);
    deps.copyBundle(appPath, stagedApp);
    journal = advanceJournal(journal, "STAGED", root);
    const patched = await deps.patch(stagedApp, installId);
    journal = advanceJournal(journal, "PATCHED", root);
    deps.sign(stagedApp);
    journal = advanceJournal(journal, "SIGNED", root);
    if (!deps.verify(stagedApp)) {
      throw new Error("staged app: codesign --verify failed; refusing to touch the real target");
    }
    journal = advanceJournal(journal, "VERIFIED", root);
    if (deps.targetRunning(appPath)) {
      throw new Error("target process is still running; refusing to swap");
    }
    deps.swap(stagedApp, appPath);
    journal = advanceJournal(journal, "SWAPPED", root);
    if (!deps.verify(appPath)) {
      throw new Error("installed app: codesign --verify failed; refusing to commit");
    }
    journal = advanceJournal(journal, "TARGET_VERIFIED", root);
    const createdAt = new Date().toISOString();
    const patchedAsarFileHash = fileSha256(join(appPath, ASAR_REL));
    deps.writeInstall({
      appPath,
      root,
      manifest: manifestFromIdentity({
        installId,
        targetRealPath: appPath,
        original: identity,
        originalAsarHeaderHash: before,
        patchedAsarHeaderHash: patched.hash,
        patchedAsarFileHash,
        originalMain: patched.originalMain,
        runtimeVersion: options.runtimeVersion,
        createdAt,
      }),
      runtime: {
        installId,
        originalMain: patched.originalMain,
        patchedAsarHeaderHash: patched.hash,
        patchedAsarFileHash,
      },
    });
    deps.writeState(
      {
        appPath,
        originalMain: patched.originalMain,
        asarHashBefore: before,
        asarHashAfter: patched.hash,
        installedAt: createdAt,
        installId,
        bundleIdentifier: identity.bundleIdentifier,
        appVersion: identity.appVersion,
        appBuild: identity.appBuild,
        architecture: identity.architecture,
        originalAsarFileHash: identity.asarFileHash,
        originalPlistFileHash: identity.plistFileHash,
      },
      root,
    );
    journal = advanceJournal(journal, "COMMITTED", root);
    return { installId, journal };
  } catch (error) {
    if (existsSync(stagedApp) && journal.phase !== "SWAPPED" && journal.phase !== "TARGET_VERIFIED" && journal.phase !== "COMMITTED") {
      rmSync(stagedApp, { recursive: true, force: true });
    }
    throw error;
  }
}
