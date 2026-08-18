import { homedir } from "node:os";
import { join } from "node:path";

export const DEFAULT_APP = "/Applications/ChatGPT.app";
export const USER_ROOT = join(homedir(), ".incodex");
export const BACKUP_DIR = join(USER_ROOT, "backup");
export const STATE_PATH = join(USER_ROOT, "install-state.json");
export const LIVE_PREV = join(USER_ROOT, "ChatGPT.app.pre-live");
export const LIVE_RECORD_PATH = join(USER_ROOT, "live-install.json");
export const INSTALLATIONS_DIR = join(USER_ROOT, "installations");
export const INCOGNITO_HOME = join(USER_ROOT, "incognito-home");
export const INCOGNITO_CHROMIUM = join(USER_ROOT, "incognito-chromium");
export const MAIN_NAME = "incodex-main.cjs";
export const PRELOAD_NAME = "incodex-preload.cjs";
export const SAFE_HOME_NAME = "incodex-safe-home.cjs";
export const IPC_GUARD_NAME = "incodex-ipc-guard.cjs";
export const INSTANCE_NAME = "incodex-instance.cjs";

export const ASAR_REL = "Contents/Resources/app.asar";
export const PLIST_REL = "Contents/Info.plist";
export const LOADER_NAME = "incodex-loader.cjs";
export const INJECT_NAME = "incodex-inject.js";
export const MARKER_KEY = "__incodex";
