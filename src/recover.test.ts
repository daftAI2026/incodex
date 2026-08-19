import { describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applyRecovery } from "./recover";
import { transactionOutgoing } from "./swap";
import { recoverAction, writeJournal, type Journal } from "./transaction";

function scratch(): string {
  return mkdtempSync(join(tmpdir(), "incodex-recover-"));
}

function seed(root: string, phase: Journal["phase"]): { journal: Journal; target: string; staged: string } {
  const target = join(root, "ChatGPT.app");
  const staged = join(root, "staged.app");
  const original = join(root, "original.app");
  mkdirSync(target);
  mkdirSync(staged);
  mkdirSync(original);
  writeFileSync(join(target, "marker"), phase === "SWAPPED" || phase === "TARGET_VERIFIED" ? "PATCHED" : "ORIGINAL");
  writeFileSync(join(staged, "marker"), "STAGED");
  writeFileSync(join(original, "marker"), "ORIGINAL");
  const journal: Journal = {
    schemaVersion: 1,
    installId: "11111111-2222-3333-4444-555555555555",
    targetRealPath: target,
    stagedApp: staged,
    originalSnapshot: original,
    outgoingApp: transactionOutgoing(root, "11111111-2222-3333-4444-555555555555"),
    phase,
    updatedAt: "now",
  };
  writeJournal(journal, root);
  return { journal, target, staged };
}

describe("recover rolls back anything not committed", () => {
  test("continue is not a recovery action", () => {
    const base = seed(scratch(), "DISCOVERED").journal;
    const unfinished = [
      "DISCOVERED",
      "BACKUP_COMMITTED",
      "STAGED",
      "PATCHED",
      "SIGNED",
      "VERIFIED",
      "TARGET_MOVED_OUT",
      "SWAPPED",
      "TARGET_VERIFIED",
    ] as const;
    for (const phase of unfinished) {
      expect(recoverAction({ ...base, phase })).toBe("rollback");
    }
    expect(recoverAction({ ...base, phase: "COMMITTED" })).toBe("done");
    expect(recoverAction({ ...base, phase: "ROLLED_BACK" })).toBe("done");
  });

  test("VERIFIED recover deletes staging and leaves the original target", () => {
    const root = scratch();
    const { journal, target, staged } = seed(root, "VERIFIED");
    const result = applyRecovery(journal, root);
    expect(result.action).toBe("rollback");
    expect(existsSync(staged)).toBe(false);
    expect(readFileSync(join(target, "marker"), "utf8")).toBe("ORIGINAL");
    expect(result.journal.phase).toBe("ROLLED_BACK");
  });

  test("SWAPPED recover copies the snapshot back over the patched target", () => {
    const root = scratch();
    const { journal, target, staged } = seed(root, "SWAPPED");
    const result = applyRecovery(journal, root);
    expect(result.action).toBe("rollback");
    expect(existsSync(staged)).toBe(false);
    expect(readFileSync(join(target, "marker"), "utf8")).toBe("ORIGINAL");
    expect(readFileSync(join(journal.originalSnapshot, "marker"), "utf8")).toBe("ORIGINAL");
    expect(result.journal.phase).toBe("ROLLED_BACK");
  });

  test("TARGET_MOVED_OUT recover restores outgoing when the snapshot is not there yet", () => {
    const root = scratch();
    const target = join(root, "ChatGPT.app");
    const staged = join(root, "staged.app");
    const original = join(root, "missing-original.app");
    const outgoing = transactionOutgoing(root, "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee");
    mkdirSync(staged);
    mkdirSync(outgoing, { recursive: true });
    writeFileSync(join(staged, "marker"), "STAGED");
    writeFileSync(join(outgoing, "marker"), "ORIGINAL");
    const journal: Journal = {
      schemaVersion: 1,
      installId: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
      targetRealPath: target,
      stagedApp: staged,
      originalSnapshot: original,
      outgoingApp: outgoing,
      phase: "TARGET_MOVED_OUT",
      updatedAt: "now",
    };
    const result = applyRecovery(journal, root);
    expect(result.action).toBe("rollback");
    expect(readFileSync(join(target, "marker"), "utf8")).toBe("ORIGINAL");
    expect(existsSync(staged)).toBe(false);
    expect(result.journal.phase).toBe("ROLLED_BACK");
  });

  test("recover is idempotent after ROLLED_BACK", () => {
    const root = scratch();
    const { journal, target } = seed(root, "SWAPPED");
    const first = applyRecovery(journal, root);
    const again = applyRecovery(first.journal, root);
    expect(again.action).toBe("done");
    expect(readFileSync(join(target, "marker"), "utf8")).toBe("ORIGINAL");
  });
});
