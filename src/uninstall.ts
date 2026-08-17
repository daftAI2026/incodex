import { existsSync, cpSync } from "node:fs";
import { join } from "node:path";
import { ASAR_REL, BACKUP_DIR, PLIST_REL } from "./paths";
import { signApp } from "./codesign";

export function uninstall(appPath: string): void {
  const asarBackup = join(BACKUP_DIR, "app.asar");
  const plistBackup = join(BACKUP_DIR, "Info.plist");
  if (!existsSync(asarBackup) || !existsSync(plistBackup)) {
    throw new Error("no backup in ~/.incodex/backup; reinstall official Codex to restore");
  }
  cpSync(asarBackup, join(appPath, ASAR_REL));
  cpSync(plistBackup, join(appPath, PLIST_REL));
  signApp(appPath);
}
