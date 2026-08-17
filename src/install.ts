import { existsSync, cpSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { headerHash, patchAsar, ensureDir } from "./asar";
import { signApp, verifyApp } from "./codesign";
import { writeAsarIntegrity } from "./integrity";
import { ASAR_REL, BACKUP_DIR, DEFAULT_APP, PLIST_REL, USER_ROOT } from "./paths";
import { saveState } from "./state";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

export type InstallOptions = {
  appPath: string;
  clone: boolean;
  live: boolean;
};

export function resolveTarget(options: { clone?: boolean; live?: boolean; app?: string }): string {
  if (options.app) return options.app;
  if (options.clone) return join(USER_ROOT, "scratch", "ChatGPT.app");
  return DEFAULT_APP;
}

export function cloneOfficialApp(dest: string): void {
  if (!existsSync(DEFAULT_APP)) throw new Error(`Codex app not found: ${DEFAULT_APP}`);
  if (existsSync(dest)) {
    spawnSync("rm", ["-rf", dest], { stdio: "inherit" });
  }
  ensureDir(dirname(dest));
  const cloned = spawnSync("cp", ["-cR", DEFAULT_APP, dest], { encoding: "utf8" });
  if (cloned.status !== 0) {
    const fallback = spawnSync("cp", ["-R", DEFAULT_APP, dest], { encoding: "utf8" });
    if (fallback.status !== 0) {
      throw new Error(fallback.stderr || "failed to copy ChatGPT.app");
    }
  }
}

export function backupApp(appPath: string): void {
  ensureDir(BACKUP_DIR);
  const asarPath = join(appPath, ASAR_REL);
  const plistPath = join(appPath, PLIST_REL);
  cpSync(asarPath, join(BACKUP_DIR, "app.asar"));
  cpSync(plistPath, join(BACKUP_DIR, "Info.plist"));
}

function ensureRuntime(): void {
  const loaderPath = join(repoRoot, "dist/incodex-loader.cjs");
  const injectPath = join(repoRoot, "dist/incodex-inject.js");
  if (existsSync(loaderPath) && existsSync(injectPath)) return;
  const built = spawnSync("bun", ["src/build-runtime.ts"], {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: "inherit",
  });
  if (built.status !== 0) throw new Error("failed to build runtime");
}

export async function install(appPath: string): Promise<void> {
  ensureRuntime();
  const asarPath = join(appPath, ASAR_REL);
  const loader = readFileSync(join(repoRoot, "dist/incodex-loader.cjs"), "utf8");
  const inject = readFileSync(join(repoRoot, "dist/incodex-inject.js"), "utf8");
  const before = headerHash(asarPath);
  backupApp(appPath);
  const patched = await patchAsar({
    asarPath,
    loaderSource: loader,
    injectSource: inject,
  });
  writeAsarIntegrity(appPath, patched.hash);
  signApp(appPath);
  if (!verifyApp(appPath)) {
    console.warn("codesign --verify failed; the copy may still open after Gatekeeper bypass");
  }
  saveState({
    appPath,
    originalMain: patched.originalMain,
    asarHashBefore: before,
    asarHashAfter: patched.hash,
    installedAt: new Date().toISOString(),
  });
}
