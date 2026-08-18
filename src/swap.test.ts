import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applyRecovery } from "./recover";
import { outgoingPath, swapBundle } from "./swap";
import type { Journal } from "./transaction";

describe("swap rollback", () => {
  test("a failed incoming rename puts the original target back", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-swap-"));
    const target = join(root, "app");
    const staged = join(root, "staged");
    mkdirSync(target);
    mkdirSync(staged);
    writeFileSync(join(target, "marker"), "original");
    writeFileSync(join(staged, "marker"), "staged");
    expect(() =>
      swapBundle(staged, target, {
        rename: (from, to) => {
          if (to === target && from === staged) throw new Error("rename failed");
          renameSync(from, to);
        },
        remove: () => undefined,
      }),
    ).toThrow(/rename failed/);
    expect(readFileSync(join(target, "marker"), "utf8")).toBe("original");
  });

  test("recover puts an outgoing bundle back when rollback rename failed", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-out-"));
    const target = join(root, "ChatGPT.app");
    const outgoing = outgoingPath(target);
    mkdirSync(outgoing);
    writeFileSync(join(outgoing, "marker"), "original");
    const journal: Journal = {
      schemaVersion: 1,
      installId: "tx",
      targetRealPath: target,
      stagedApp: join(root, "staged"),
      originalSnapshot: join(root, "original"),
      phase: "VERIFIED",
      updatedAt: "now",
    };
    const result = applyRecovery(journal);
    expect(result.outgoingRestored).toBe(true);
    expect(readFileSync(join(target, "marker"), "utf8")).toBe("original");
  });
});
