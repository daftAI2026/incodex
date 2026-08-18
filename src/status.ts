import { existsSync } from "node:fs";
import { join } from "node:path";
import { readPackageMain } from "./asar";
import { loadCurrentInstallation, targetId } from "./installation";
import { ASAR_REL, DEFAULT_APP } from "./paths";
import { loadState } from "./state";

export function printStatus(appPath = DEFAULT_APP): void {
  const asarPath = join(appPath, ASAR_REL);
  console.log("app:", appPath);
  console.log("exists:", existsSync(appPath));
  if (!existsSync(asarPath)) {
    console.log("asar: missing");
    return;
  }
  const pkg = readPackageMain(asarPath);
  console.log("patched:", pkg.alreadyPatched);
  console.log("main:", pkg.main);
  if (pkg.installId) console.log("install id:", pkg.installId);
  console.log("target id:", targetId(appPath));
  const stored = loadCurrentInstallation(appPath);
  if (stored) {
    console.log("stored install:", stored.manifest.installId);
    console.log("stored version:", stored.manifest.appVersion, stored.manifest.appBuild);
    console.log("stored original asar file hash:", stored.manifest.originalAsarFileHash);
    console.log("stored patched asar file hash:", stored.manifest.patchedAsarFileHash);
  }
  const state = loadState();
  if (state) {
    console.log("last install:", state.installedAt);
    console.log("last target:", state.appPath);
    if (state.installId) console.log("last install id:", state.installId);
    if (state.appVersion) console.log("last app version:", state.appVersion, state.appBuild ?? "");
  }
}
