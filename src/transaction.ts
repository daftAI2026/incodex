import { existsSync, readdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { ensureDir } from "./asar";
import { USER_ROOT } from "./paths";

export const PHASES = [
  "DISCOVERED",
  "BACKUP_COMMITTED",
  "STAGED",
  "PATCHED",
  "SIGNED",
  "VERIFIED",
  "SWAPPED",
  "TARGET_VERIFIED",
  "COMMITTED",
] as const;

export type Phase = (typeof PHASES)[number];

export type Journal = {
  schemaVersion: 1;
  installId: string;
  targetRealPath: string;
  stagedApp: string;
  originalSnapshot: string;
  phase: Phase;
  updatedAt: string;
};

export type Recovery = "continue" | "rollback" | "refuse" | "done";

export function journalPath(installId: string, root = USER_ROOT): string {
  return join(root, "transactions", `${installId}.json`);
}

export function parseJournal(raw: unknown): Journal | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as Partial<Journal>;
  if (value.schemaVersion !== 1) return null;
  if (typeof value.installId !== "string" || !value.installId) return null;
  if (typeof value.targetRealPath !== "string" || !value.targetRealPath) return null;
  if (typeof value.stagedApp !== "string" || !value.stagedApp) return null;
  if (typeof value.originalSnapshot !== "string" || !value.originalSnapshot) return null;
  if (!PHASES.includes(value.phase as Phase)) return null;
  return value as Journal;
}

export function recoverAction(journal: Journal): Recovery {
  switch (journal.phase) {
    case "DISCOVERED":
    case "BACKUP_COMMITTED":
    case "STAGED":
    case "PATCHED":
    case "SIGNED":
      return "rollback";
    case "VERIFIED":
      return "continue";
    case "SWAPPED":
      return "continue";
    case "TARGET_VERIFIED":
      return "continue";
    case "COMMITTED":
      return "done";
    default:
      return "refuse";
  }
}

export function writeJournal(journal: Journal, root = USER_ROOT): void {
  const path = journalPath(journal.installId, root);
  ensureDir(dirname(path));
  const staged = `${path}.tmp`;
  writeFileSync(staged, `${JSON.stringify(journal, null, 2)}\n`);
  renameSync(staged, path);
}

export function loadJournal(installId: string, root = USER_ROOT): Journal | null {
  const path = journalPath(installId, root);
  if (!existsSync(path)) return null;
  try {
    return parseJournal(JSON.parse(readFileSync(path, "utf8")));
  } catch {
    return null;
  }
}

export function listJournals(root = USER_ROOT): Journal[] {
  const dir = join(root, "transactions");
  if (!existsSync(dir)) return [];
  const out: Journal[] = [];
  for (const name of readdirSync(dir)) {
    if (!name.endsWith(".json") || name.endsWith(".tmp")) continue;
    const journal = loadJournal(name.replace(/\.json$/, ""), root);
    if (journal) out.push(journal);
  }
  return out;
}

export function advanceJournal(journal: Journal, phase: Phase, root = USER_ROOT): Journal {
  const next: Journal = { ...journal, phase, updatedAt: new Date().toISOString() };
  writeJournal(next, root);
  return next;
}
