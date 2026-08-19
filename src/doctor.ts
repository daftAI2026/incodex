import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { inspectApp } from "./app-identity";
import { formatKv, formatSection, formatWarn } from "./cli-print";
import { asarHasOnlyLoader, headerHash, readPackageMain } from "./asar";
import { inspectExternalRuntime, type ExternalRuntimeReport } from "./external-runtime";
import { diagnoseSpctl, verifyApp, type SpctlDiagnosis } from "./codesign";
import { loadCurrentInstallation, loadSigningManifest, originalAppPath, targetId } from "./installation";
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
  asarLoaderOnly: boolean | null;
  externalRuntime: ExternalRuntimeReport;
  interruptedTransactions: Array<{ installId: string; phase: Journal["phase"]; action: string }>;
  signing: {
    verified: boolean;
    componentCount: number;
    hardenedRuntimeOk: boolean;
    unretainable: string[];
    spctl: SpctlDiagnosis;
  } | null;
  spctl: SpctlDiagnosis | null;
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
  const storedSigning = stored ? loadSigningManifest(stored.dir) : null;
  const spctl = info.exists ? diagnoseSpctl(appPath) : null;

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
    asarLoaderOnly: info.asarExists ? asarHasOnlyLoader(asarPath) : null,
    externalRuntime: inspectExternalRuntime(root),
    signing: storedSigning
      ? {
          verified: storedSigning.verified,
          componentCount: storedSigning.components.length,
          hardenedRuntimeOk: storedSigning.observations.every((item) => item.hardenedRuntime),
          unretainable: storedSigning.unretainableEntitlements.flatMap((item) => item.keys),
          spctl: storedSigning.spctl,
        }
      : null,
    spctl,
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
  const runtime = report.externalRuntime.ok
    ? `${report.externalRuntime.version} ${report.externalRuntime.release}`
    : report.externalRuntime.present
      ? "invalid"
      : "missing";
  const backup = report.backup
    ? report.backup.originalExists && report.backup.complete
      ? "ok"
      : "incomplete"
    : "none";
  console.log(formatSection("App"));
  console.log(formatKv("Path", report.target));
  console.log(formatKv("Exists", report.exists ? "yes" : "no"));
  console.log(formatKv("Installed", report.patched ? "yes" : "no"));
  console.log(formatKv("Bundle", report.bundleId ?? "unknown"));
  console.log(formatKv("Version", `${report.appVersion ?? "unknown"} ${report.appBuild ?? ""}`.trim()));
  console.log(formatKv("Arch", report.architecture ?? "unknown"));
  console.log("");
  console.log(formatSection("Runtime"));
  console.log(formatKv("Version", report.runtimeVersion ?? "unknown"));
  console.log(formatKv("External", runtime));
  if (report.externalRuntime.error) console.log(formatWarn(report.externalRuntime.error));
  console.log(formatKv("Loader", report.asarLoaderOnly === null ? "unknown" : report.asarLoaderOnly ? "asar only" : "mixed"));
  console.log(formatKv("Main", report.originalMain || "unknown"));
  console.log("");
  console.log(formatSection("Signing"));
  console.log(formatKv("Verify", report.codesignOk ? "ok" : "failed"));
  if (report.signing) {
    console.log(formatKv("Hardened", report.signing.hardenedRuntimeOk ? "yes" : "no"));
    console.log(formatKv("Dropped", report.signing.unretainable.length ? report.signing.unretainable.join(", ") : "none"));
  }
  if (report.spctl) {
    console.log(formatKv("Gatekeeper", report.spctl.accepted ? "accepted" : "not accepted (diagnostic)"));
  }
  console.log("");
  console.log(formatSection("Backup"));
  console.log(formatKv("State", backup));
  if (report.backup) {
    console.log(formatKv("Matches", report.backup.belongsToTarget ? "yes" : "no"));
  }
  console.log("");
  console.log(formatSection("Sessions"));
  console.log(formatKv("Orphans", String(report.orphanSessions.length)));
  console.log(formatKv("Chromium", String(report.leftoverChromium.length)));
  console.log(formatKv("Stale pid", report.stalePid ? "yes" : "no"));
  console.log(formatKv("Journals", String(report.interruptedTransactions.length)));
  for (const item of report.interruptedTransactions) {
    console.log(formatKv("Journal", `${item.installId}  ${item.phase} -> ${item.action}`));
  }
  if (report.interruptedTransactions.length) {
    console.log(formatWarn("Old install journals are leftover. They do not mean the current app is broken."));
  }
  console.log("");
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
