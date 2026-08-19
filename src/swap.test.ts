import { describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applyRecovery } from "./recover";
import { defaultSwapOps, outgoingPath, swapBundle, transactionOutgoing } from "./swap";
import type { Journal } from "./transaction";

describe("swap rollback", () => {
  test("swap does not delete an existing outgoing bundle before moving the target", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-keep-out-"));
    const target = join(root, "app");
    const staged = join(root, "staged");
    const outgoing = outgoingPath(target);
    mkdirSync(target);
    mkdirSync(staged);
    mkdirSync(outgoing);
    writeFileSync(join(target, "marker"), "current");
    writeFileSync(join(staged, "marker"), "staged");
    writeFileSync(join(outgoing, "marker"), "only-original");
    expect(() => swapBundle(staged, target)).toThrow(/outgoing already exists/i);
    expect(readFileSync(join(outgoing, "marker"), "utf8")).toBe("only-original");
    expect(readFileSync(join(target, "marker"), "utf8")).toBe("current");
  });

  test("swap keeps outgoing after the staged bundle is in place", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-keep-after-"));
    const target = join(root, "app");
    const staged = join(root, "staged");
    mkdirSync(target);
    mkdirSync(staged);
    writeFileSync(join(target, "marker"), "original");
    writeFileSync(join(staged, "marker"), "staged");
    swapBundle(staged, target);
    expect(readFileSync(join(target, "marker"), "utf8")).toBe("staged");
    expect(readFileSync(join(outgoingPath(target), "marker"), "utf8")).toBe("original");
  });

  test("transaction outgoing is scoped to the install id, not a global ChatGPT.app.outgoing", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tx-out-"));
    expect(transactionOutgoing(root, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")).toBe(
      join(root, "transactions", "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "outgoing", "ChatGPT.app"),
    );
  });

  test("afterTargetMoved runs after the original is aside and before the staged bundle lands", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-hook-"));
    const target = join(root, "app");
    const staged = join(root, "staged");
    mkdirSync(target);
    mkdirSync(staged);
    writeFileSync(join(target, "marker"), "original");
    writeFileSync(join(staged, "marker"), "staged");
    let hooked = false;
    swapBundle(staged, target, defaultSwapOps, {
      afterTargetMoved: () => {
        expect(existsSync(target)).toBe(false);
        expect(readFileSync(join(outgoingPath(target), "marker"), "utf8")).toBe("original");
        hooked = true;
      },
    });
    expect(hooked).toBe(true);
    expect(readFileSync(join(target, "marker"), "utf8")).toBe("staged");
  });

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
    const result = applyRecovery(journal, root);
    expect(result.outgoingRestored).toBe(true);
    expect(readFileSync(join(target, "marker"), "utf8")).toBe("original");
  });
});
