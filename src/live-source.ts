import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { ensureDir } from "./asar";
import {
  identitiesEqual,
  listingsEqual,
  type AppIdentity,
  type AppInspection,
} from "./app-identity";
import { LIVE_RECORD_PATH, USER_ROOT } from "./paths";

export const NOT_INCODEX_INSTALL = "当前应用已经不是 Incodex 安装态";

export type LiveInstallRecord = {
  schemaVersion: 1;
  installId: string;
  targetRealPath: string;
  original: AppIdentity;
  createdAt: string;
};

export type SourceDecision =
  | { action: "use-current" }
  | { action: "use-backup" }
  | { action: "reject"; reason: string };

export type RestoreDecision = { ok: true } | { ok: false; reason: string };

export function loadLiveInstallRecord(): LiveInstallRecord | null {
  if (!existsSync(LIVE_RECORD_PATH)) return null;
  try {
    return parseLiveInstallRecord(JSON.parse(readFileSync(LIVE_RECORD_PATH, "utf8")));
  } catch {
    return null;
  }
}

export function parseLiveInstallRecord(raw: unknown): LiveInstallRecord | null {
  if (!raw || typeof raw !== "object") return null;
  const record = raw as Partial<LiveInstallRecord>;
  return isLiveInstallRecord(record) ? record : null;
}

export function saveLiveInstallRecord(record: LiveInstallRecord): void {
  ensureDir(USER_ROOT);
  writeFileSync(LIVE_RECORD_PATH, `${JSON.stringify(record, null, 2)}\n`);
}

export function selectOfficialInstallSource(input: {
  current: AppInspection;
  backup: AppInspection | null;
  record: LiveInstallRecord | null;
}): SourceDecision {
  if (!input.current.patched) return { action: "use-current" };

  const match = officialBackupMatchesInstall(input);
  if (!match.ok) {
    return {
      action: "reject",
      reason:
        match.reason === NOT_INCODEX_INSTALL
          ? "current app is patched but no matching original backup; refusing to guess a source"
          : match.reason,
    };
  }
  return { action: "use-backup" };
}

export function canRestoreOfficial(input: {
  current: AppInspection;
  backup: AppInspection | null;
  record: LiveInstallRecord | null;
}): RestoreDecision {
  if (!input.current.patched) return { ok: false, reason: NOT_INCODEX_INSTALL };
  return officialBackupMatchesInstall(input);
}

export function officialBackupMatchesInstall(input: {
  current: AppInspection;
  backup: AppInspection | null;
  record: LiveInstallRecord | null;
}): RestoreDecision {
  const { current, backup, record } = input;
  if (!current.patched) return { ok: false, reason: NOT_INCODEX_INSTALL };
  if (record) return matchRecordedOriginal(current, backup, record);
  return matchLegacySameVersionOriginal(current, backup);
}

function matchRecordedOriginal(
  current: AppInspection,
  backup: AppInspection | null,
  record: LiveInstallRecord,
): RestoreDecision {
  if (!current.installId || !current.listing) {
    return { ok: false, reason: NOT_INCODEX_INSTALL };
  }
  if (current.installId !== record.installId) {
    return { ok: false, reason: NOT_INCODEX_INSTALL };
  }
  if (!listingsEqual(current.listing, record.original)) {
    return { ok: false, reason: NOT_INCODEX_INSTALL };
  }
  if (!backup?.exists || !backup.identity) {
    return { ok: false, reason: "no matching original backup for this Incodex install" };
  }
  if (backup.patched) {
    return { ok: false, reason: "original backup is patched; refusing to use it" };
  }
  if (!identitiesEqual(backup.identity, record.original)) {
    return { ok: false, reason: "original backup does not match this install record" };
  }
  return { ok: true };
}

function matchLegacySameVersionOriginal(
  current: AppInspection,
  backup: AppInspection | null,
): RestoreDecision {
  if (!current.listing || !backup?.exists || !backup.identity || backup.patched) {
    return { ok: false, reason: NOT_INCODEX_INSTALL };
  }
  if (!listingsEqual(current.listing, backup.identity)) {
    return { ok: false, reason: NOT_INCODEX_INSTALL };
  }
  return { ok: true };
}

function isLiveInstallRecord(raw: Partial<LiveInstallRecord>): raw is LiveInstallRecord {
  const original = raw.original;
  return (
    raw.schemaVersion === 1 &&
    typeof raw.installId === "string" &&
    raw.installId.length > 0 &&
    typeof raw.targetRealPath === "string" &&
    typeof raw.createdAt === "string" &&
    Boolean(original) &&
    typeof original?.bundleIdentifier === "string" &&
    typeof original.appVersion === "string" &&
    typeof original.appBuild === "string" &&
    typeof original.architecture === "string" &&
    typeof original.asarFileHash === "string" &&
    typeof original.plistFileHash === "string"
  );
}
