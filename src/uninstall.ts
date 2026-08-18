import { existsSync, cpSync, rmSync, renameSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { ASAR_REL, BACKUP_DIR, DEFAULT_APP, PLIST_REL } from "./paths";
import { signApp } from "./codesign";
import { isOfficialApp, LIVE_PREV, quitOfficialApp } from "./install";
import { readPackageMain } from "./asar";

export function uninstall(appPath: string): void {
  const asarBackup = join(BACKUP_DIR, "app.asar");
  const plistBackup = join(BACKUP_DIR, "Info.plist");
  if (isOfficialApp(appPath)) {
    quitOfficialApp();
    if (existsSync(LIVE_PREV)) {
      const prevAsar = join(LIVE_PREV, ASAR_REL);
      if (existsSync(prevAsar) && !readPackageMain(prevAsar).alreadyPatched) {
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
    }
  }
  if (!existsSync(asarBackup) || !existsSync(plistBackup)) {
    throw new Error("no backup in ~/.incodex/backup; reinstall official Codex to restore");
  }
  if (isOfficialApp(appPath) && readPackageMain(asarBackup).alreadyPatched) {
    throw new Error(
      "backup asar is already patched; refusing to restore it over official Codex. reinstall ChatGPT.app from OpenAI instead.",
    );
  }
  if (isOfficialApp(appPath)) {
    const staged = join(BACKUP_DIR, "..", "ChatGPT.app.restore");
    rmSync(staged, { recursive: true, force: true });
    const copied = spawnSync("ditto", [appPath, staged], { encoding: "utf8" });
    if (copied.status !== 0) throw new Error(copied.stderr || "failed to stage restore");
    cpSync(asarBackup, join(staged, ASAR_REL));
    cpSync(plistBackup, join(staged, PLIST_REL));
    signApp(staged);
    const trash = `${appPath}.incodex-uninstall`;
    rmSync(trash, { recursive: true, force: true });
    renameSync(appPath, trash);
    try {
      renameSync(staged, appPath);
    } catch (error) {
      renameSync(trash, appPath);
      throw error;
    }
    rmSync(trash, { recursive: true, force: true });
    return;
  }
  cpSync(asarBackup, join(appPath, ASAR_REL));
  cpSync(plistBackup, join(appPath, PLIST_REL));
  signApp(appPath);
}
