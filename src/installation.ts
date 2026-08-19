import { createHash } from "node:crypto";
import { existsSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { canonicalPath, isOfficialApp } from "./canonical-target";
import { spawnSync } from "node:child_process";
import type { AppIdentity } from "./app-identity";
import { ensureDir } from "./asar";
import type { SigningManifest } from "./codesign";
import { DEFAULT_APP, INSTALLATIONS_DIR, USER_ROOT } from "./paths";

export type TransactionState = "committed";

export type InstallManifest = {
  schemaVersion: 1;
  installId: string;
  targetRealPath: string;
  bundleIdentifier: string;
  appVersion: string;
  appBuild: string;
  architecture: string;
  originalAsarHeaderHash: string;
  originalAsarFileHash: string;
  originalPlistFileHash: string;
  patchedAsarHeaderHash: string;
  patchedAsarFileHash: string;
  originalMain: string;
  runtimeVersion: string;
  createdAt: string;
  transactionState: TransactionState;
};

export type RuntimeManifest = {
  installId: string;
  originalMain: string;
  patchedAsarHeaderHash: string;
  patchedAsarFileHash: string;
};

export type RestoreCheck = { ok: true } | { ok: false; reason: string; advice: string };

export type LoadedInstallation = {
  dir: string;
  manifest: InstallManifest;
};

const HASH_LEN = 12;

export function targetId(appPath: string): string {
  if (isOfficialApp(appPath)) {
    const digest = createHash("sha256").update(resolve(DEFAULT_APP)).digest("hex").slice(0, HASH_LEN);
    return `official-${digest}`;
  }
  const real = canonicalPath(appPath);
  const digest = createHash("sha256").update(real).digest("hex").slice(0, HASH_LEN);
  return `app-${digest}`;
}

export function targetStoreDir(appPath: string, root = USER_ROOT): string {
  return join(root, "installations", targetId(appPath));
}

export function installDir(appPath: string, installId: string, root = USER_ROOT): string {
  return join(targetStoreDir(appPath, root), installId);
}

export function originalAppPath(appPath: string, installId: string, root = USER_ROOT): string {
  return join(installDir(appPath, installId, root), "original", "ChatGPT.app");
}

export function parseInstallManifest(raw: unknown): InstallManifest | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as Partial<InstallManifest>;
  if (value.schemaVersion !== 1) return null;
  const fields: (keyof InstallManifest)[] = [
    "installId",
    "targetRealPath",
    "bundleIdentifier",
    "appVersion",
    "appBuild",
    "architecture",
    "originalAsarHeaderHash",
    "originalAsarFileHash",
    "originalPlistFileHash",
    "patchedAsarHeaderHash",
    "patchedAsarFileHash",
    "originalMain",
    "runtimeVersion",
    "createdAt",
    "transactionState",
  ];
  for (const field of fields) {
    if (typeof value[field] !== "string" || !value[field]) return null;
  }
  if (value.transactionState !== "committed") return null;
  return value as InstallManifest;
}

export function canRestoreInstallation(input: {
  targetRealPath: string;
  currentInstallId: string | null;
  currentAppBuild: string | null;
  currentAsarFileHash: string | null;
  currentOriginalMain: string;
  manifest: InstallManifest;
}): RestoreCheck {
  if (canonicalPath(input.targetRealPath) !== canonicalPath(input.manifest.targetRealPath)) {
    return refuse(
      "backup does not belong to this target",
      "do not reuse another app's installation record; install this target again",
    );
  }
  if (!input.currentInstallId || input.currentInstallId !== input.manifest.installId) {
    return refuse(
      "current Incodex install ID does not match this backup",
      "the recorded original will not be written over this app",
    );
  }
  if (!input.currentAppBuild || input.currentAppBuild !== input.manifest.appBuild) {
    return refuse(
      "current bundle build does not match this backup",
      "the app is a different build than the one Incodex installed",
    );
  }
  if (!input.currentAsarFileHash || input.currentAsarFileHash !== input.manifest.patchedAsarFileHash) {
    return refuse(
      "current ASAR file hash does not match this backup",
      "the app package changed after install; refusing to guess a restore",
    );
  }
  if (input.currentOriginalMain && input.currentOriginalMain !== input.manifest.originalMain) {
    return refuse(
      "originalMain does not match this backup",
      "the recorded original will not be written over this app",
    );
  }
  return { ok: true };
}

export function loadCurrentInstallation(appPath: string, root = USER_ROOT): LoadedInstallation | null {
  const pointer = join(targetStoreDir(appPath, root), "current.json");
  if (!existsSync(pointer)) return null;
  try {
    const current = JSON.parse(readFileSync(pointer, "utf8")) as { installId?: string };
    if (!current.installId) return null;
    return loadInstallation(appPath, current.installId, root);
  } catch {
    return null;
  }
}

export function loadInstallation(
  appPath: string,
  installId: string,
  root = USER_ROOT,
): LoadedInstallation | null {
  const dir = installDir(appPath, installId, root);
  const manifestPath = join(dir, "manifest.json");
  if (!existsSync(manifestPath)) return null;
  try {
    const manifest = parseInstallManifest(JSON.parse(readFileSync(manifestPath, "utf8")));
    if (!manifest || manifest.installId !== installId) return null;
    return { dir, manifest };
  } catch {
    return null;
  }
}

export function writeInstallation(options: {
  appPath: string;
  manifest: InstallManifest;
  runtime: RuntimeManifest;
  signing?: SigningManifest;
  root?: string;
}): string {
  const root = options.root ?? USER_ROOT;
  if (targetId(options.appPath) !== targetId(options.manifest.targetRealPath)) {
    throw new Error("installation target does not match manifest.targetRealPath");
  }
  const dir = installDir(options.appPath, options.manifest.installId, root);
  if (existsSync(join(dir, "manifest.json"))) {
    throw new Error(`install ${options.manifest.installId} already exists and is immutable`);
  }
  ensureDir(join(dir, "original"));
  ensureDir(join(dir, "patched"));
  writeFileSync(join(dir, "patched", "runtime-manifest.json"), `${JSON.stringify(options.runtime, null, 2)}\n`);
  if (options.signing) {
    writeFileSync(
      join(dir, "patched", "signing-manifest.json"),
      `${JSON.stringify(options.signing, null, 2)}\n`,
      { mode: 0o600 },
    );
  }
  writeFileSync(join(dir, "manifest.json"), `${JSON.stringify(options.manifest, null, 2)}\n`, { mode: 0o600 });
  const pointer = join(targetStoreDir(options.appPath, root), "current.json");
  const staged = `${pointer}.tmp`;
  writeFileSync(staged, `${JSON.stringify({ installId: options.manifest.installId }, null, 2)}\n`);
  renameSync(staged, pointer);
  return dir;
}

export function loadSigningManifest(dir: string): SigningManifest | null {
  try {
    const raw = JSON.parse(readFileSync(join(dir, "patched", "signing-manifest.json"), "utf8")) as SigningManifest;
    if (raw.schemaVersion !== 1 || !raw.spctl || raw.spctl.usedAsSuccessGate !== false) return null;
    return raw;
  } catch {
    return null;
  }
}

export function snapshotOriginalApp(sourceApp: string, destApp: string): void {
  if (existsSync(destApp)) {
    throw new Error(`refusing to overwrite original snapshot: ${destApp}`);
  }
  ensureDir(dirname(destApp));
  const copied = spawnSync("ditto", [sourceApp, destApp], { encoding: "utf8" });
  if (copied.status !== 0) throw new Error(copied.stderr || `failed to snapshot ${sourceApp}`);
}

export function restoreOriginalApp(sourceApp: string, destApp: string): void {
  if (!existsSync(sourceApp)) throw new Error(`original snapshot missing: ${sourceApp}`);
  const staged = `${destApp}.incodex-restore`;
  rmSync(staged, { recursive: true, force: true });
  const copied = spawnSync("ditto", [sourceApp, staged], { encoding: "utf8" });
  if (copied.status !== 0) throw new Error(copied.stderr || "failed to stage original snapshot");
  const trash = `${destApp}.incodex-uninstall`;
  rmSync(trash, { recursive: true, force: true });
  if (existsSync(destApp)) renameSync(destApp, trash);
  try {
    renameSync(staged, destApp);
  } catch (error) {
    if (existsSync(trash)) renameSync(trash, destApp);
    throw error;
  }
  rmSync(trash, { recursive: true, force: true });
}

export function manifestFromIdentity(input: {
  installId: string;
  targetRealPath: string;
  original: AppIdentity;
  originalAsarHeaderHash: string;
  patchedAsarHeaderHash: string;
  patchedAsarFileHash: string;
  originalMain: string;
  runtimeVersion: string;
  createdAt: string;
}): InstallManifest {
  return {
    schemaVersion: 1,
    installId: input.installId,
    targetRealPath: canonicalPath(input.targetRealPath),
    bundleIdentifier: input.original.bundleIdentifier,
    appVersion: input.original.appVersion,
    appBuild: input.original.appBuild,
    architecture: input.original.architecture,
    originalAsarHeaderHash: input.originalAsarHeaderHash,
    originalAsarFileHash: input.original.asarFileHash,
    originalPlistFileHash: input.original.plistFileHash,
    patchedAsarHeaderHash: input.patchedAsarHeaderHash,
    patchedAsarFileHash: input.patchedAsarFileHash,
    originalMain: input.originalMain,
    runtimeVersion: input.runtimeVersion,
    createdAt: input.createdAt,
    transactionState: "committed",
  };
}

export function installationsRoot(root = USER_ROOT): string {
  return root === USER_ROOT ? INSTALLATIONS_DIR : join(root, "installations");
}

function refuse(reason: string, advice: string): RestoreCheck {
  return { ok: false, reason, advice };
}
