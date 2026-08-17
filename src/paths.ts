import { homedir } from "node:os";
import { join } from "node:path";

export const DEFAULT_APP = "/Applications/ChatGPT.app";
export const USER_ROOT = join(homedir(), ".incodex");
export const BACKUP_DIR = join(USER_ROOT, "backup");
export const STATE_PATH = join(USER_ROOT, "install-state.json");

export const ASAR_REL = "Contents/Resources/app.asar";
export const PLIST_REL = "Contents/Info.plist";
export const LOADER_NAME = "incodex-loader.cjs";
export const INJECT_NAME = "incodex-inject.js";
export const MARKER_KEY = "__incodex";
