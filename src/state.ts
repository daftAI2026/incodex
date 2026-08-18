import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { ensureDir } from "./asar";
import { STATE_PATH, USER_ROOT } from "./paths";

export type InstallState = {
  appPath: string;
  originalMain: string;
  asarHashBefore: string;
  asarHashAfter: string;
  installedAt: string;
  installId?: string;
  bundleIdentifier?: string;
  appVersion?: string;
  appBuild?: string;
  architecture?: string;
  originalAsarFileHash?: string;
  originalPlistFileHash?: string;
};

export function statePath(root = USER_ROOT): string {
  return root === USER_ROOT ? STATE_PATH : join(root, "install-state.json");
}

export function loadState(root = USER_ROOT): InstallState | null {
  const path = statePath(root);
  if (!existsSync(path)) return null;
  return JSON.parse(readFileSync(path, "utf8")) as InstallState;
}

export function saveState(state: InstallState, root = USER_ROOT): void {
  const path = statePath(root);
  ensureDir(root);
  writeFileSync(path, `${JSON.stringify(state, null, 2)}\n`);
}
