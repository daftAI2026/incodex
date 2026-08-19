import { existsSync, rmSync } from "node:fs";
import { withTargetLock } from "./mutation-lock";
import { USER_ROOT } from "./paths";
import { outgoingPath, restoreOutgoingIfNeeded } from "./swap";
import { loadJournal, recoverAction, type Journal, type Recovery } from "./transaction";

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
      return applyRecovery(fresh);
    },
  );
}

export function applyRecovery(journal: Journal): RecoverResult {
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

  const outgoingRestored = restoreOutgoingIfNeeded(
    journal.targetRealPath,
    journal.outgoingApp ?? outgoingPath(journal.targetRealPath),
  );
  let stagedRemoved = !existsSync(journal.stagedApp);
  if (action === "rollback" && existsSync(journal.stagedApp)) {
    rmSync(journal.stagedApp, { recursive: true, force: true });
    stagedRemoved = true;
  }

  return {
    action,
    journal,
    targetUntouched: existsSync(journal.targetRealPath),
    backupIntact: existsSync(journal.originalSnapshot),
    stagedRemoved,
    outgoingRestored,
  };
}
