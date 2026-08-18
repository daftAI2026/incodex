import { existsSync, readFileSync, writeFileSync } from "node:fs";
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

export function loadState(): InstallState | null {
  if (!existsSync(STATE_PATH)) return null;
  return JSON.parse(readFileSync(STATE_PATH, "utf8")) as InstallState;
}

export function saveState(state: InstallState): void {
  ensureDir(USER_ROOT);
  writeFileSync(STATE_PATH, `${JSON.stringify(state, null, 2)}\n`);
}
