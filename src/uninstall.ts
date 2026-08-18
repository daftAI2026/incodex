import { existsSync, cpSync, rmSync, renameSync } from "node:fs";
import { join } from "node:path";
import { inspectApp } from "./app-identity";
import { readPackageMain } from "./asar";
import { signApp } from "./codesign";
import { isOfficialApp, quitOfficialApp } from "./install";
import { canRestoreOfficial, loadLiveInstallRecord } from "./live-source";
import { ASAR_REL, BACKUP_DIR, LIVE_PREV, PLIST_REL } from "./paths";

export function uninstall(appPath: string): void {
  if (isOfficialApp(appPath)) {
    uninstallOfficial(appPath);
    return;
  }
  restoreFromAsarBackup(appPath);
}

function uninstallOfficial(appPath: string): void {
  quitOfficialApp();
  const current = inspectApp(appPath);
  const backup = existsSync(LIVE_PREV) ? inspectApp(LIVE_PREV) : null;
  const record = loadLiveInstallRecord();
  const decision = canRestoreOfficial({ current, backup, record });
  if (!decision.ok) throw new Error(decision.reason);

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
}

function restoreFromAsarBackup(appPath: string): void {
  const asarBackup = join(BACKUP_DIR, "app.asar");
  const plistBackup = join(BACKUP_DIR, "Info.plist");
  if (!existsSync(asarBackup) || !existsSync(plistBackup)) {
    throw new Error("no backup in ~/.incodex/backup; reinstall official Codex to restore");
  }
  if (readPackageMain(asarBackup).alreadyPatched) {
    throw new Error("backup asar is already patched; refusing to restore it");
  }
  cpSync(asarBackup, join(appPath, ASAR_REL));
  cpSync(plistBackup, join(appPath, PLIST_REL));
  signApp(appPath);
}
