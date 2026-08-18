import { existsSync, readFileSync, rmSync, renameSync, symlinkSync } from "node:fs";
import { randomUUID } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { fileSha256, inspectApp } from "./app-identity";
import { headerHash, ensureDir, patchAsar } from "./asar";
import { assertLiveSupported, findSupportedBuild } from "./compatibility/supported-builds";
import { signApp, verifyApp } from "./codesign";
import {
  manifestFromIdentity,
  originalAppPath,
  snapshotOriginalApp,
  writeInstallation,
} from "./installation";
import { writeAsarIntegrity } from "./integrity";
import {
  loadLiveInstallRecord,
  resolveOfficialOriginal,
  saveLiveInstallRecord,
  selectOfficialInstallSource,
} from "./live-source";
import { runCloneInstall } from "./install-transaction";
import { ASAR_REL, DEFAULT_APP, LIVE_PREV, USER_ROOT } from "./paths";
import { saveState } from "./state";
import { advanceJournal, writeJournal, type Journal } from "./transaction";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
export { LIVE_PREV };

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

export function isOfficialApp(appPath: string): boolean {
  return resolve(appPath) === resolve(DEFAULT_APP);
}

export function cloneOfficialApp(dest: string): void {
  if (!existsSync(DEFAULT_APP)) throw new Error(`Codex app not found: ${DEFAULT_APP}`);
  if (existsSync(dest)) spawnSync("rm", ["-rf", dest], { stdio: "inherit" });
  ensureDir(dirname(dest));
  const cloned = spawnSync("ditto", [DEFAULT_APP, dest], { encoding: "utf8" });
  if (cloned.status !== 0) throw new Error(cloned.stderr || "failed to copy ChatGPT.app");
}

function ensureRuntime(): void {
  const built = spawnSync("bun", ["src/build-runtime.ts"], {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: "inherit",
  });
  if (built.status !== 0) throw new Error("failed to build runtime");
}

export function quitOfficialApp(): void {
  const listed = spawnSync("ps", ["-ax", "-o", "pid=,command="], { encoding: "utf8" });
  const needle = `${DEFAULT_APP}/Contents/MacOS/ChatGPT`;
  const pids = (listed.stdout || "")
    .split("\n")
    .filter((line) => line.includes(needle))
    .map((line) => Number(line.trim().split(/\s+/)[0]))
    .filter((pid) => Number.isInteger(pid) && pid > 0);
  if (pids.length === 0) return;
  console.log("quitting official Codex", pids.join(" "));
  for (const pid of pids) spawnSync("kill", [String(pid)]);
  spawnSync("sleep", ["1"]);
}

function runtimeSources(): {
  loader: string;
  inject: string;
  main: string;
  preload: string;
  safeHome: string;
  ipcGuard: string;
  instance: string;
  runtimeLoad: string;
  windowKind: string;
} {
  return {
    loader: readFileSync(join(repoRoot, "dist/incodex-loader.cjs"), "utf8"),
    inject: readFileSync(join(repoRoot, "dist/incodex-inject.js"), "utf8"),
    main: readFileSync(join(repoRoot, "dist/incodex-main.cjs"), "utf8"),
    preload: readFileSync(join(repoRoot, "dist/incodex-preload.cjs"), "utf8"),
    safeHome: readFileSync(join(repoRoot, "dist/incodex-safe-home.cjs"), "utf8"),
    ipcGuard: readFileSync(join(repoRoot, "dist/incodex-ipc-guard.cjs"), "utf8"),
    instance: readFileSync(join(repoRoot, "dist/incodex-instance.cjs"), "utf8"),
    runtimeLoad: readFileSync(join(repoRoot, "dist/incodex-runtime-load.cjs"), "utf8"),
    windowKind: readFileSync(join(repoRoot, "dist/incodex-window-kind.cjs"), "utf8"),
  };
}

async function patchAppBundle(
  appPath: string,
  resign: boolean,
  installId: string,
): Promise<{ originalMain: string; hash: string }> {
  const src = runtimeSources();
  const patched = await patchAsar({
    asarPath: join(appPath, ASAR_REL),
    loaderSource: src.loader,
    injectSource: src.inject,
    mainSource: src.main,
    preloadSource: src.preload,
    safeHomeSource: src.safeHome,
    ipcGuardSource: src.ipcGuard,
    instanceSource: src.instance,
    runtimeLoadSource: src.runtimeLoad,
    windowKindSource: src.windowKind,
    installId,
  });
  writeAsarIntegrity(appPath, patched.hash);
  if (resign) signApp(appPath);
  return patched;
}

function requireVerified(appPath: string, label: string): void {
  if (!verifyApp(appPath)) {
    throw new Error(`${label}: codesign --verify failed; refusing to touch the real target`);
  }
}



function swapOfficialWith(stagedApp: string, originalDest: string | null): void {
  const outgoing = join(USER_ROOT, "ChatGPT.app.outgoing");
  rmSync(outgoing, { recursive: true, force: true });
  renameSync(DEFAULT_APP, outgoing);
  try {
    renameSync(stagedApp, DEFAULT_APP);
  } catch (error) {
    renameSync(outgoing, DEFAULT_APP);
    throw error;
  }
  if (!originalDest) {
    rmSync(outgoing, { recursive: true, force: true });
    return;
  }
  ensureDir(dirname(originalDest));
  if (existsSync(originalDest)) {
    throw new Error(`refusing to overwrite original snapshot: ${originalDest}`);
  }
  renameSync(outgoing, originalDest);
}

function pointLivePrevAt(originalApp: string): void {
  rmSync(LIVE_PREV, { recursive: true, force: true });
  symlinkSync(originalApp, LIVE_PREV);
}

function runtimeVersion(): string {
  try {
    const manifest = JSON.parse(
      readFileSync(join(repoRoot, "dist/runtime-manifest.json"), "utf8"),
    ) as { runtimeVersion?: string };
    if (manifest.runtimeVersion) return manifest.runtimeVersion;
  } catch {
    /* fall back to package.json */
  }
  const pkg = JSON.parse(readFileSync(join(repoRoot, "package.json"), "utf8")) as { version?: string };
  return pkg.version || "0.0.0";
}

async function installLive(): Promise<void> {
  if (!existsSync(DEFAULT_APP)) throw new Error(`Codex app not found: ${DEFAULT_APP}`);
  quitOfficialApp();
  const current = inspectApp(DEFAULT_APP);
  const previousOriginal = resolveOfficialOriginal();
  const backup = previousOriginal ? inspectApp(previousOriginal) : null;
  const record = loadLiveInstallRecord();
  const decision = selectOfficialInstallSource({ current, backup, record });
  if (decision.action === "reject") throw new Error(decision.reason);

  const sourceApp = decision.action === "use-backup" ? previousOriginal! : DEFAULT_APP;
  const sourceIdentity = decision.action === "use-backup" ? backup?.identity : current.identity;
  if (!sourceIdentity) throw new Error(`could not read official app identity: ${sourceApp}`);

  const installId = randomUUID();
  const originalDest = originalAppPath(DEFAULT_APP, installId);
  const stagedApp = join(USER_ROOT, "ChatGPT.app.live");
  let journal: Journal = {
    schemaVersion: 1,
    installId,
    targetRealPath: resolve(DEFAULT_APP),
    stagedApp,
    originalSnapshot: originalDest,
    phase: "DISCOVERED",
    updatedAt: new Date().toISOString(),
  };
  writeJournal(journal);
  if (decision.action === "use-backup") {
    console.log("install source: matching original backup");
    snapshotOriginalApp(sourceApp, originalDest);
  } else {
    console.log("install source: current official app");
  }
  journal = advanceJournal(journal, "BACKUP_COMMITTED");

  const before = headerHash(join(sourceApp, ASAR_REL));
  rmSync(stagedApp, { recursive: true, force: true });
  console.log("copying official OpenAI-signed app to a writable staging bundle");
  const copied = spawnSync("ditto", [sourceApp, stagedApp], { encoding: "utf8" });
  if (copied.status !== 0) throw new Error(copied.stderr || "failed to stage official app");
  journal = advanceJournal(journal, "STAGED");
  const patched = await patchAppBundle(stagedApp, true, installId);
  journal = advanceJournal(journal, "PATCHED");
  journal = advanceJournal(journal, "SIGNED");
  requireVerified(stagedApp, "staged official app");
  journal = advanceJournal(journal, "VERIFIED");
  const patchedAsar = join(stagedApp, ASAR_REL);
  const patchedAsarFileHash = fileSha256(patchedAsar);
  console.log("replacing /Applications/ChatGPT.app with the patched bundle");
  swapOfficialWith(stagedApp, decision.action === "use-current" ? originalDest : null);
  journal = advanceJournal(journal, "SWAPPED");
  if (!existsSync(originalDest)) throw new Error("original snapshot missing after install");
  requireVerified(DEFAULT_APP, "installed official app");
  journal = advanceJournal(journal, "TARGET_VERIFIED");
  pointLivePrevAt(originalDest);
  const createdAt = new Date().toISOString();
  const manifest = manifestFromIdentity({
    installId,
    targetRealPath: DEFAULT_APP,
    original: sourceIdentity,
    originalAsarHeaderHash: before,
    patchedAsarHeaderHash: patched.hash,
    patchedAsarFileHash,
    originalMain: patched.originalMain,
    runtimeVersion: runtimeVersion(),
    createdAt,
  });
  writeInstallation({
    appPath: DEFAULT_APP,
    manifest,
    runtime: {
      installId,
      originalMain: patched.originalMain,
      patchedAsarHeaderHash: patched.hash,
      patchedAsarFileHash,
    },
  });
  saveLiveInstallRecord({
    schemaVersion: 1,
    installId,
    targetRealPath: resolve(DEFAULT_APP),
    original: sourceIdentity,
    createdAt,
  });
  saveState({
    appPath: DEFAULT_APP,
    originalMain: patched.originalMain,
    asarHashBefore: before,
    asarHashAfter: patched.hash,
    installedAt: createdAt,
    installId,
    bundleIdentifier: sourceIdentity.bundleIdentifier,
    appVersion: sourceIdentity.appVersion,
    appBuild: sourceIdentity.appBuild,
    architecture: sourceIdentity.architecture,
    originalAsarFileHash: sourceIdentity.asarFileHash,
    originalPlistFileHash: sourceIdentity.plistFileHash,
  });
  advanceJournal(journal, "COMMITTED");
  console.log("official app patched. reopen /Applications/ChatGPT.app");
  console.log("to restore official Codex: bun src/cli.ts uninstall --live");
}

export async function install(appPath: string, options?: { root?: string }): Promise<void> {
  if (!existsSync(appPath)) throw new Error(`Codex app not found: ${appPath}`);
  const listing = inspectApp(appPath).listing;
  if (isOfficialApp(appPath)) {
    assertLiveSupported(listing ?? {});
    ensureRuntime();
    await installLive();
    return;
  }
  if (!findSupportedBuild(listing ?? {})) {
    console.warn(
      `unknown Codex build ${listing?.appVersion ?? "unknown"} (${listing?.appBuild ?? "unknown"}); clone is experimental`,
    );
  }
  ensureRuntime();
  await runCloneInstall(appPath, {
    root: options?.root,
    runtimeVersion: runtimeVersion(),
    patch: (stagedApp, installId) => patchAppBundle(stagedApp, false, installId),
  });
}
