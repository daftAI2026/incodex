import { existsSync, rmSync, renameSync, symlinkSync } from "node:fs";
import { randomUUID } from "node:crypto";
import { dirname, join } from "node:path";
import { canonicalize, isOfficialApp } from "./canonical-target";
import { withTargetLockAsync } from "./mutation-lock";
import { spawnSync } from "node:child_process";
import { fileSha256, inspectApp } from "./app-identity";
import { asarHasOnlyLoader, headerHash, ensureDir } from "./asar";
import { signApp, verifyTarget, type SigningManifest } from "./codesign";
import {
  loadCurrentInstallation,
  manifestFromIdentity,
  originalAppPath,
  snapshotOriginalApp,
  writeInstallation,
} from "./installation";
import type { CommandResult } from "./command-result";
import { patchStagedBundle } from "./patcher";
import {
  loadLiveInstallRecord,
  resolveOfficialOriginal,
  saveLiveInstallRecord,
  selectOfficialInstallSource,
} from "./live-source";
import { runCloneInstall } from "./install-transaction";
import {
  loadPackagedArtifacts,
  packagedRuntimeVersion,
  publishPackagedRuntime,
  resolvePackagedDistDir,
  runtimeMatchesPackaged,
} from "./packaged-runtime";
import { formatKv, formatOk } from "./cli-print";
import { quitOfficialApp } from "./quit-official";
import { notifyLaunchServices } from "./launch-services";
import { ASAR_REL, DEFAULT_APP, LIVE_PREV, USER_ROOT } from "./paths";
import { saveState } from "./state";
import { advanceJournal, writeJournal, type Journal } from "./transaction";

export { LIVE_PREV };
export { isOfficialApp } from "./canonical-target";
export { listOfficialPids, quitOfficialApp } from "./quit-official";

export function officialInstallAlreadyCurrent(input: {
  patched: boolean;
  loaderOnly: boolean;
  runtimeCurrent: boolean;
}): boolean {
  return input.patched && input.loaderOnly && input.runtimeCurrent;
}

export function officialInstallWouldSkip(appPath: string, userRoot = USER_ROOT): boolean {
  if (!isOfficialApp(appPath) || !existsSync(appPath)) return false;
  const info = inspectApp(appPath);
  return officialInstallAlreadyCurrent({
    patched: info.patched,
    loaderOnly: info.asarExists && asarHasOnlyLoader(join(appPath, ASAR_REL)),
    runtimeCurrent: runtimeMatchesPackaged(userRoot),
  });
}

export type InstallOptions = {
  appPath: string;
  clone: boolean;
  live: boolean;
};

export function resolveTarget(options: { clone?: boolean; live?: boolean; app?: string }): string {
  if (options.app) return options.app;
  if (options.clone) return join(USER_ROOT, "scratch", "ChatGPT.app");
  return DEFAULT_APP;
}

export function cloneOfficialApp(dest: string): void {
  if (!existsSync(DEFAULT_APP)) throw new Error(`Codex app not found: ${DEFAULT_APP}`);
  if (existsSync(dest)) spawnSync("rm", ["-rf", dest], { stdio: "inherit" });
  ensureDir(dirname(dest));
  const cloned = spawnSync("ditto", [DEFAULT_APP, dest], { encoding: "utf8" });
  if (cloned.status !== 0) throw new Error(cloned.stderr || "failed to copy ChatGPT.app");
}

export function openOfficialApp(): void {
  const opened = spawnSync("open", [DEFAULT_APP], { encoding: "utf8" });
  if (opened.status !== 0) throw new Error(opened.stderr || "failed to open ChatGPT.app");
}

async function patchAppBundle(
  appPath: string,
  resign: boolean,
  installId: string,
): Promise<{ originalMain: string; hash: string; signing: SigningManifest | null }> {
  const patched = await patchStagedBundle({
    stagedApp: appPath,
    artifacts: loadPackagedArtifacts(resolvePackagedDistDir()),
    installId,
  });
  const signing = resign ? signApp(appPath) : null;
  return { ...patched, signing };
}



function swapOfficialWith(stagedApp: string, originalDest: string | null): void {
  const outgoing = join(USER_ROOT, "ChatGPT.app.outgoing");
  rmSync(outgoing, { recursive: true, force: true });
  renameSync(DEFAULT_APP, outgoing);
  try {
    renameSync(stagedApp, DEFAULT_APP);
  } catch (error) {
    renameSync(outgoing, DEFAULT_APP);
    throw error;
  }
  if (!originalDest) {
    rmSync(outgoing, { recursive: true, force: true });
    return;
  }
  ensureDir(dirname(originalDest));
  if (existsSync(originalDest)) {
    throw new Error(`refusing to overwrite original snapshot: ${originalDest}`);
  }
  renameSync(outgoing, originalDest);
}

function pointLivePrevAt(originalApp: string): void {
  rmSync(LIVE_PREV, { recursive: true, force: true });
  symlinkSync(originalApp, LIVE_PREV);
}

function publishBundledRuntime(userRoot: string) {
  const distDir = resolvePackagedDistDir();
  return publishPackagedRuntime(userRoot, distDir);
}

export function installExternalRuntime(userRoot = USER_ROOT): CommandResult {
  const skipped = runtimeMatchesPackaged(userRoot);
  const current = publishBundledRuntime(userRoot);
  return {
    action: "runtime",
    skipped,
    runtimeVersion: current.version,
  };
}

async function installLive(): Promise<{ installId: string; runtimeVersion: string }> {
  if (!existsSync(DEFAULT_APP)) throw new Error(`Codex app not found: ${DEFAULT_APP}`);
  quitOfficialApp();
  const current = inspectApp(DEFAULT_APP);
  const previousOriginal = resolveOfficialOriginal();
  const backup = previousOriginal ? inspectApp(previousOriginal) : null;
  const record = loadLiveInstallRecord();
  const decision = selectOfficialInstallSource({ current, backup, record });
  if (decision.action === "reject") throw new Error(decision.reason);

  const sourceApp = decision.action === "use-backup" ? previousOriginal! : DEFAULT_APP;
  const sourceIdentity = decision.action === "use-backup" ? backup?.identity : current.identity;
  if (!sourceIdentity) throw new Error(`could not read official app identity: ${sourceApp}`);

  const installId = randomUUID();
  const originalDest = originalAppPath(DEFAULT_APP, installId);
  const stagedApp = join(USER_ROOT, "ChatGPT.app.live");
  let journal: Journal = {
    schemaVersion: 1,
    installId,
    targetRealPath: canonicalize(DEFAULT_APP).realPath,
    stagedApp,
    originalSnapshot: originalDest,
    phase: "DISCOVERED",
    updatedAt: new Date().toISOString(),
  };
  writeJournal(journal);
  if (decision.action === "use-backup") {
    console.log(formatKv("Source", "matching original backup"));
    snapshotOriginalApp(sourceApp, originalDest);
  } else {
    console.log(formatKv("Source", "current official app"));
  }
  journal = advanceJournal(journal, "BACKUP_COMMITTED");

  const before = headerHash(join(sourceApp, ASAR_REL));
  rmSync(stagedApp, { recursive: true, force: true });
  const copied = spawnSync("ditto", [sourceApp, stagedApp], { encoding: "utf8" });
  if (copied.status !== 0) throw new Error(copied.stderr || "failed to stage official app");
  console.log(formatOk("Copied official app to staging"));
  journal = advanceJournal(journal, "STAGED");
  const patched = await patchAppBundle(stagedApp, true, installId);
  journal = advanceJournal(journal, "PATCHED");
  journal = advanceJournal(journal, "SIGNED");
  verifyTarget(stagedApp, "staged official app");
  journal = advanceJournal(journal, "VERIFIED");
  const patchedAsar = join(stagedApp, ASAR_REL);
  const patchedAsarFileHash = fileSha256(patchedAsar);
  swapOfficialWith(stagedApp, decision.action === "use-current" ? originalDest : null);
  console.log(formatOk("Replaced /Applications/ChatGPT.app"));
  journal = advanceJournal(journal, "SWAPPED");
  if (!existsSync(originalDest)) throw new Error("original snapshot missing after install");
  verifyTarget(DEFAULT_APP, "installed official app");
  journal = advanceJournal(journal, "TARGET_VERIFIED");
  pointLivePrevAt(originalDest);
  const createdAt = new Date().toISOString();
  const manifest = manifestFromIdentity({
    installId,
    targetRealPath: DEFAULT_APP,
    original: sourceIdentity,
    originalAsarHeaderHash: before,
    patchedAsarHeaderHash: patched.hash,
    patchedAsarFileHash,
    originalMain: patched.originalMain,
    runtimeVersion: packagedRuntimeVersion(resolvePackagedDistDir()),
    createdAt,
  });
  writeInstallation({
    appPath: DEFAULT_APP,
    manifest,
    runtime: {
      installId,
      originalMain: patched.originalMain,
      patchedAsarHeaderHash: patched.hash,
      patchedAsarFileHash,
    },
    signing: patched.signing ?? undefined,
  });
  saveLiveInstallRecord({
    schemaVersion: 1,
    installId,
    targetRealPath: canonicalize(DEFAULT_APP).realPath,
    original: sourceIdentity,
    createdAt,
  });
  saveState({
    appPath: DEFAULT_APP,
    originalMain: patched.originalMain,
    asarHashBefore: before,
    asarHashAfter: patched.hash,
    installedAt: createdAt,
    installId,
    bundleIdentifier: sourceIdentity.bundleIdentifier,
    appVersion: sourceIdentity.appVersion,
    appBuild: sourceIdentity.appBuild,
    architecture: sourceIdentity.architecture,
    originalAsarFileHash: sourceIdentity.asarFileHash,
    originalPlistFileHash: sourceIdentity.plistFileHash,
  });
  advanceJournal(journal, "COMMITTED");
  console.log(formatOk("Official app patched"));
  console.log(formatKv("Restore", "incodex uninstall"));
  return { installId, runtimeVersion: packagedRuntimeVersion(resolvePackagedDistDir()) };
}

export async function install(appPath: string, options?: { root?: string }): Promise<CommandResult> {
  if (!existsSync(appPath)) throw new Error(`Codex app not found: ${appPath}`);
  const userRoot = options?.root ?? USER_ROOT;
  const target = canonicalize(appPath);
  return withTargetLockAsync({ targetPath: target.realPath, root: userRoot, command: "install" }, async () => {
    if (target.isOfficial) {
      const info = inspectApp(target.realPath);
      const skip = officialInstallAlreadyCurrent({
        patched: info.patched,
        loaderOnly: info.asarExists && asarHasOnlyLoader(join(target.realPath, ASAR_REL)),
        runtimeCurrent: runtimeMatchesPackaged(userRoot),
      });
      const published = publishBundledRuntime(userRoot);
      if (skip) {
        const stored = loadCurrentInstallation(target.realPath, userRoot);
        return {
          action: "install",
          skipped: true,
          installId: stored?.manifest.installId,
          runtimeVersion: published.version,
          app: target.realPath,
        };
      }
      const live = await installLive();
      notifyLaunchServices(target.realPath);
      return {
        action: "install",
        installId: live.installId,
        runtimeVersion: live.runtimeVersion,
        app: target.realPath,
      };
    }
    const published = publishBundledRuntime(userRoot);
    const cloned = await runCloneInstall(target.realPath, {
      root: userRoot,
      runtimeVersion: packagedRuntimeVersion(resolvePackagedDistDir()),
      patch: (stagedApp, installId) => patchAppBundle(stagedApp, false, installId),
    });
    notifyLaunchServices(target.realPath);
    return {
      action: "install",
      installId: cloned.installId,
      runtimeVersion: published.version,
      app: target.realPath,
    };
  });
}
