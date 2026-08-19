import { existsSync, rmSync } from "node:fs";
import { restoreOriginalApp } from "./installation";
import { withTargetLock } from "./mutation-lock";
import { USER_ROOT } from "./paths";
import { outgoingPath, restoreOutgoingIfNeeded } from "./swap";
import { advanceJournal, loadJournal, recoverAction, type Journal, type Phase, type Recovery } from "./transaction";

const NEEDS_ORIGINAL_BACK: Phase[] = ["TARGET_MOVED_OUT", "SWAPPED", "TARGET_VERIFIED"];

export type RecoverResult = {
  action: Recovery;
  journal: Journal;
  targetUntouched: boolean;
  backupIntact: boolean;
  stagedRemoved: boolean;
  outgoingRestored: boolean;
};

export function recoverTransaction(installId: string, root = USER_ROOT): RecoverResult {
  const journal = loadJournal(installId, root);
  if (!journal) throw new Error(`no journal for ${installId}`);
  return withTargetLock(
    { targetPath: journal.targetRealPath, root, command: "recover", installId },
    () => {
      const fresh = loadJournal(installId, root);
      if (!fresh) throw new Error(`no journal for ${installId}`);
      return applyRecovery(fresh, root);
    },
  );
}

export function applyRecovery(journal: Journal, root = USER_ROOT): RecoverResult {
  const action = recoverAction(journal);
  const backupIntact = existsSync(journal.originalSnapshot);
  if (action === "done") {
    return {
      action,
      journal,
      targetUntouched: existsSync(journal.targetRealPath),
      backupIntact,
      stagedRemoved: !existsSync(journal.stagedApp),
      outgoingRestored: false,
    };
  }
  if (action === "refuse") {
    throw new Error(`cannot recover transaction ${journal.installId} in phase ${journal.phase}`);
  }

  const outgoing = journal.outgoingApp ?? outgoingPath(journal.targetRealPath);
  let outgoingRestored = false;
  if (NEEDS_ORIGINAL_BACK.includes(journal.phase)) {
    if (existsSync(journal.originalSnapshot)) {
      restoreOriginalApp(journal.originalSnapshot, journal.targetRealPath);
    } else {
      outgoingRestored = restoreOutgoingIfNeeded(journal.targetRealPath, outgoing);
    }
    if (existsSync(outgoing)) rmSync(outgoing, { recursive: true, force: true });
  } else {
    outgoingRestored = restoreOutgoingIfNeeded(journal.targetRealPath, outgoing);
  }

  if (existsSync(journal.stagedApp)) {
    rmSync(journal.stagedApp, { recursive: true, force: true });
  }
  const next = advanceJournal(journal, "ROLLED_BACK", root);
  return {
    action,
    journal: next,
    targetUntouched: existsSync(next.targetRealPath),
    backupIntact: existsSync(next.originalSnapshot),
    stagedRemoved: !existsSync(next.stagedApp),
    outgoingRestored,
  };
}
