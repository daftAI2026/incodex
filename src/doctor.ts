import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { inspectApp } from "./app-identity";
import { headerHash, readPackageMain } from "./asar";
import { verifyApp } from "./codesign";
import { loadCurrentInstallation, originalAppPath, targetId } from "./installation";
import { readAsarIntegrity } from "./integrity";
import { ASAR_REL, DEFAULT_APP, USER_ROOT } from "./paths";
import { staleOwner, targetStateDir } from "./runtime/incodex-instance.cts";
import { listJournals, recoverAction, type Journal } from "./transaction";

export type BackupReport = {
  belongsToTarget: boolean;
  complete: boolean;
  originalExists: boolean;
  runtimeVersion: string | null;
  originalAsarFileHash: string | null;
  patchedAsarFileHash: string | null;
};

export type Diagnosis = {
  target: string;
  targetId: string;
  exists: boolean;
  patched: boolean;
  bundleId: string | null;
  appVersion: string | null;
  appBuild: string | null;
  architecture: string | null;
  asarFileHash: string | null;
  asarHeaderHash: string | null;
  plistFileHash: string | null;
  plistIntegrityHash: string | null;
  runtimeVersion: string | null;
  originalMain: string;
  codesignOk: boolean;
  backup: BackupReport | null;
  stalePid: boolean;
  orphanSessions: string[];
  leftoverChromium: string[];
  interruptedTransactions: Array<{ installId: string; phase: Journal["phase"]; action: string }>;
};

export function diagnose(appPath = DEFAULT_APP, root = USER_ROOT): Diagnosis {
  const info = inspectApp(appPath);
  const stored = loadCurrentInstallation(appPath, root);
  const asarPath = join(appPath, ASAR_REL);
  let asarHeaderHash: string | null = null;
  let originalMain = info.originalMain;
  if (info.asarExists) {
    try {
      asarHeaderHash = headerHash(asarPath);
      originalMain = readPackageMain(asarPath).main;
    } catch {
      asarHeaderHash = null;
    }
  }

  let plistIntegrityHash: string | null = null;
  if (info.exists) {
    try {
      plistIntegrityHash = readAsarIntegrity(appPath);
    } catch {
      plistIntegrityHash = null;
    }
  }

  const backup = stored
    ? {
        belongsToTarget: stored.manifest.targetRealPath === info.path || stored.manifest.installId === info.installId,
        complete: Boolean(stored.manifest.originalAsarFileHash && stored.manifest.patchedAsarFileHash),
        originalExists: existsSync(originalAppPath(appPath, stored.manifest.installId, root)),
        runtimeVersion: stored.manifest.runtimeVersion,
        originalAsarFileHash: stored.manifest.originalAsarFileHash,
        patchedAsarFileHash: stored.manifest.patchedAsarFileHash,
      }
    : null;

  const id = targetId(appPath);
  const sessions = listSessionRoots(root, id);
  const leftoverChromium = sessions.filter((session) => existsSync(join(session, "chromium")));
  const orphanSessions = sessions.filter((session) => isOrphanSession(session));
  const execGuess = join(appPath, "Contents/MacOS/ChatGPT");
  const stateDir = targetStateDir(root, existsSync(execGuess) ? execGuess : appPath);

  return {
    target: appPath,
    targetId: id,
    exists: info.exists,
    patched: info.patched,
    bundleId: info.listing?.bundleIdentifier ?? null,
    appVersion: info.listing?.appVersion ?? null,
    appBuild: info.listing?.appBuild ?? null,
    architecture: info.listing?.architecture ?? null,
    asarFileHash: info.identity?.asarFileHash ?? null,
    asarHeaderHash,
    plistFileHash: info.identity?.plistFileHash ?? null,
    plistIntegrityHash,
    runtimeVersion: stored?.manifest.runtimeVersion ?? null,
    originalMain,
    codesignOk: info.exists ? verifyApp(appPath) : false,
    backup,
    stalePid: existsSync(join(stateDir, "incognito.lock")) ? staleOwner(stateDir) : false,
    orphanSessions,
    leftoverChromium,
    interruptedTransactions: listJournals(root)
      .filter((journal) => recoverAction(journal) !== "done")
      .map((journal) => ({
        installId: journal.installId,
        phase: journal.phase,
        action: recoverAction(journal),
      })),
  };
}

export function printDiagnosis(report: Diagnosis): void {
  console.log("target:", report.target);
  console.log("target id:", report.targetId);
  console.log("exists:", report.exists);
  console.log("patched:", report.patched);
  console.log("bundle:", report.bundleId ?? "unknown");
  console.log("version:", report.appVersion ?? "unknown", report.appBuild ?? "");
  console.log("arch:", report.architecture ?? "unknown");
  console.log("asar file hash:", report.asarFileHash ?? "unknown");
  console.log("asar header hash:", report.asarHeaderHash ?? "unknown");
  console.log("plist file hash:", report.plistFileHash ?? "unknown");
  console.log("plist integrity hash:", report.plistIntegrityHash ?? "unknown");
  console.log("runtime version:", report.runtimeVersion ?? "unknown");
  console.log("original main:", report.originalMain || "unknown");
  console.log("codesign verify:", report.codesignOk);
  console.log("backup:", report.backup ? (report.backup.originalExists && report.backup.complete ? "ok" : "incomplete") : "none");
  if (report.backup) {
    console.log("backup belongs to target:", report.backup.belongsToTarget);
    console.log("backup original asar:", report.backup.originalAsarFileHash);
  }
  console.log("stale pid:", report.stalePid);
  console.log("orphan sessions:", report.orphanSessions.length);
  for (const session of report.orphanSessions) console.log("  orphan:", session);
  console.log("chromium leftovers:", report.leftoverChromium.length);
  for (const leftover of report.leftoverChromium) console.log("  chromium:", leftover);
  console.log("interrupted transactions:", report.interruptedTransactions.length);
  for (const item of report.interruptedTransactions) {
    console.log(`  ${item.installId} ${item.phase} -> ${item.action}`);
  }
}

function listSessionRoots(root: string, id: string): string[] {
  const parent = join(root, "sessions", id);
  if (!existsSync(parent)) return [];
  return readdirSync(parent)
    .map((name) => join(parent, name))
    .filter((path) => {
      try {
        return statSync(path).isDirectory();
      } catch {
        return false;
      }
    });
}

function isOrphanSession(sessionRoot: string): boolean {
  const ownerPath = join(sessionRoot, "owner.json");
  if (!existsSync(ownerPath)) return true;
  try {
    const owner = JSON.parse(readFileSync(ownerPath, "utf8")) as { pid?: number };
    if (!owner.pid) return true;
    return !processAlive(owner.pid);
  } catch {
    return true;
  }
}

function processAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}
