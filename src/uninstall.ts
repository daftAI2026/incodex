import { existsSync, renameSync, rmSync } from "node:fs";
import { join } from "node:path";
import { fileSha256, inspectApp } from "./app-identity";
import { readPackageMain } from "./asar";
import { isOfficialApp, quitOfficialApp } from "./install";
import {
  canRestoreInstallation,
  loadCurrentInstallation,
  restoreOriginalApp,
} from "./installation";
import { canRestoreOfficial, loadLiveInstallRecord, resolveOfficialOriginal } from "./live-source";
import type { CommandResult } from "./command-result";
import { ASAR_REL, LIVE_PREV } from "./paths";

export function uninstall(appPath: string): CommandResult {
  const stored = loadCurrentInstallation(appPath);
  if (isOfficialApp(appPath)) {
    uninstallOfficial(appPath);
  } else {
    uninstallTarget(appPath);
  }
  return {
    action: "uninstall",
    installId: stored?.manifest.installId,
    runtimeVersion: stored?.manifest.runtimeVersion,
    app: appPath,
  };
}

function uninstallOfficial(appPath: string): void {
  quitOfficialApp();
  const current = inspectApp(appPath);
  const backupPath = resolveOfficialOriginal();
  const backup = backupPath ? inspectApp(backupPath) : null;
  const record = loadLiveInstallRecord();
  const allowed = canRestoreOfficial({ current, backup, record });
  if (!allowed.ok) throw new Error(allowed.reason);

  const stored = loadCurrentInstallation(appPath);
  if (stored) {
    assertRestoreMatches(appPath, current, stored.manifest);
    const original = join(stored.dir, "original", "ChatGPT.app");
    assertOriginalClean(original);
    restoreOriginalApp(original, appPath);
    return;
  }

  if (!backupPath || !existsSync(backupPath)) {
    throw new Error("no matching original backup for this Incodex install");
  }
  assertOriginalClean(backupPath);
  if (backupPath === LIVE_PREV) {
    const trash = `${appPath}.incodex-uninstall`;
    rmSync(trash, { recursive: true, force: true });
    renameSync(appPath, trash);
    try {
      renameSync(LIVE_PREV, appPath);
    } catch (error) {
      renameSync(trash, appPath);
      throw error;
    }
    rmSync(trash, { recursive: true, force: true });
    return;
  }
  restoreOriginalApp(backupPath, appPath);
}

function uninstallTarget(appPath: string): void {
  const stored = loadCurrentInstallation(appPath);
  if (!stored) {
    throw new Error(
      "no installation record for this target. refusing to use ~/.incodex/backup because it is not bound to this app",
    );
  }
  const current = inspectApp(appPath);
  assertRestoreMatches(appPath, current, stored.manifest);
  const original = join(stored.dir, "original", "ChatGPT.app");
  assertOriginalClean(original);
  restoreOriginalApp(original, appPath);
}

function assertRestoreMatches(
  appPath: string,
  current: ReturnType<typeof inspectApp>,
  manifest: Parameters<typeof canRestoreInstallation>[0]["manifest"],
): void {
  const asarPath = join(appPath, ASAR_REL);
  const check = canRestoreInstallation({
    targetRealPath: appPath,
    currentInstallId: current.installId,
    currentAppBuild: current.listing?.appBuild ?? null,
    currentAsarFileHash: current.identity?.asarFileHash ?? (existsSync(asarPath) ? fileSha256(asarPath) : null),
    currentOriginalMain: current.originalMain,
    manifest,
  });
  if (!check.ok) throw new Error(`${check.reason}. ${check.advice}`);
}

function assertOriginalClean(originalApp: string): void {
  const asar = join(originalApp, ASAR_REL);
  if (!existsSync(asar)) throw new Error(`original snapshot missing asar: ${originalApp}`);
  if (readPackageMain(asar).alreadyPatched) {
    throw new Error("original snapshot is already patched; refusing to restore it");
  }
}
