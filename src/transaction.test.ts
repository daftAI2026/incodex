import { describe, expect, test } from "bun:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  advanceJournal,
  loadJournal,
  parseJournal,
  recoverAction,
  type Journal,
} from "./transaction";

function journal(over: Partial<Journal> = {}): Journal {
  return {
    schemaVersion: 1,
    installId: "id-1",
    targetRealPath: "/tmp/ChatGPT.app",
    stagedApp: "/tmp/staged.app",
    originalSnapshot: "/tmp/original.app",
    phase: "DISCOVERED",
    updatedAt: "2026-08-18T00:00:00.000Z",
    ...over,
  };
}

describe("install journal", () => {
  test("rejects a journal that is only a guessed phase", () => {
    expect(parseJournal({ installId: "x", phase: "MAYBE" })).toBeNull();
    expect(parseJournal(journal())).not.toBeNull();
  });

  test("unverified work rolls back so the real target is unchanged", () => {
    expect(recoverAction(journal({ phase: "STAGED" }))).toBe("rollback");
    expect(recoverAction(journal({ phase: "PATCHED" }))).toBe("rollback");
    expect(recoverAction(journal({ phase: "SIGNED" }))).toBe("rollback");
  });

  test("verified staged work may continue; committed work is done", () => {
    expect(recoverAction(journal({ phase: "VERIFIED" }))).toBe("continue");
    expect(recoverAction(journal({ phase: "SWAPPED" }))).toBe("continue");
    expect(recoverAction(journal({ phase: "TARGET_VERIFIED" }))).toBe("continue");
    expect(recoverAction(journal({ phase: "COMMITTED" }))).toBe("done");
  });

  test("every unfinished phase has a recovery action", () => {
    const base = {
      schemaVersion: 1 as const,
      installId: "x",
      targetRealPath: "/tmp/app",
      stagedApp: "/tmp/staged",
      originalSnapshot: "/tmp/orig",
      updatedAt: "now",
    };
    expect(recoverAction({ ...base, phase: "DISCOVERED" })).toBe("rollback");
    expect(recoverAction({ ...base, phase: "BACKUP_COMMITTED" })).toBe("rollback");
    expect(recoverAction({ ...base, phase: "STAGED" })).toBe("rollback");
    expect(recoverAction({ ...base, phase: "PATCHED" })).toBe("rollback");
    expect(recoverAction({ ...base, phase: "SIGNED" })).toBe("rollback");
    expect(recoverAction({ ...base, phase: "VERIFIED" })).toBe("continue");
    expect(recoverAction({ ...base, phase: "SWAPPED" })).toBe("continue");
    expect(recoverAction({ ...base, phase: "TARGET_VERIFIED" })).toBe("continue");
    expect(recoverAction({ ...base, phase: "COMMITTED" })).toBe("done");
  });

  test("journal advances atomically and can be reloaded", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tx-"));
    const next = advanceJournal(journal(), "BACKUP_COMMITTED", root);
    expect(next.phase).toBe("BACKUP_COMMITTED");
    expect(loadJournal("id-1", root)?.phase).toBe("BACKUP_COMMITTED");
  });
});
