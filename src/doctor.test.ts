import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { diagnose } from "./doctor";
import { targetId } from "./installation";
import { writeJournal } from "./transaction";

describe("doctor", () => {
  test("reports interrupted journals, orphan sessions, and leftover chromium", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-doctor-"));
    const app = join(root, "Missing.app");
    writeJournal({
      schemaVersion: 1,
      installId: "tx-1",
      targetRealPath: app,
      stagedApp: join(root, "staged"),
      originalSnapshot: join(root, "original"),
      phase: "PATCHED",
      updatedAt: new Date().toISOString(),
    }, root);
    const session = join(root, "sessions", targetId(app), "s-orphan");
    mkdirSync(join(session, "chromium"), { recursive: true });
    writeFileSync(join(session, "owner.json"), `${JSON.stringify({ pid: 999999, sessionId: "s-orphan" })}\n`);

    const report = diagnose(app, root);
    expect(report.exists).toBe(false);
    expect(report.interruptedTransactions).toEqual([
      { installId: "tx-1", phase: "PATCHED", action: "rollback" },
    ]);
    expect(report.orphanSessions.some((path) => path.endsWith("s-orphan"))).toBe(true);
    expect(report.leftoverChromium.some((path) => path.endsWith("s-orphan"))).toBe(true);
    expect(report.stalePid).toBe(false);
    expect(report.codesignOk).toBe(false);
    expect(report.spctl).toBeNull();
    expect(report.signing).toBeNull();
    expect(report.asarLoaderOnly).toBeNull();
    expect(report.externalRuntime.ok).toBe(false);
    expect(report.externalRuntime.present).toBe(false);
  });
});
