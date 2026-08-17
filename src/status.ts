import { existsSync } from "node:fs";
import { join } from "node:path";
import { readPackageMain } from "./asar";
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
  const state = loadState();
  if (state) {
    console.log("last install:", state.installedAt);
    console.log("last target:", state.appPath);
  }
}
