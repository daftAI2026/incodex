import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { canonicalPath } from "./canonical-target";
import { acquireTargetLock, releaseTargetLock, withTargetLock } from "./mutation-lock";

function scratch(): string {
  return mkdtempSync(join(tmpdir(), "incodex-lock-"));
}

describe("mutation lock", () => {
  test("a second lock on the same target fails while the first is held", () => {
    const root = scratch();
    const target = join(root, "ChatGPT.app");
    mkdirSync(target);
    const held = acquireTargetLock({ targetPath: target, root, command: "install" });
    expect(() => acquireTargetLock({ targetPath: target, root, command: "uninstall" })).toThrow(
      /another incodex command is modifying/i,
    );
    releaseTargetLock(held);
    const again = acquireTargetLock({ targetPath: target, root, command: "uninstall" });
    releaseTargetLock(again);
  });

  test("symlink aliases of the same app share one lock", () => {
    const root = scratch();
    const applications = join(root, "Applications");
    const official = join(applications, "ChatGPT.app");
    mkdirSync(official, { recursive: true });
    symlinkSync(applications, join(root, "apps"));
    const held = acquireTargetLock({
      targetPath: official,
      root,
      command: "install",
    });
    expect(() =>
      acquireTargetLock({
        targetPath: join(root, "apps", "ChatGPT.app"),
        root,
        command: "recover",
      }),
    ).toThrow(/another incodex command is modifying/i);
    releaseTargetLock(held);
  });

  test("a lock whose pid is dead is stolen", () => {
    const root = scratch();
    const target = join(root, "ChatGPT.app");
    mkdirSync(target);
    const locks = join(root, "locks");
    mkdirSync(locks);
    const digest = createHash("sha256").update(canonicalPath(target)).digest("hex");
    writeFileSync(
      join(locks, `${digest}.lock`),
      `${JSON.stringify({
        schemaVersion: 1,
        pid: 999999,
        processStart: "never",
        command: "install",
        requestedPath: target,
        realPath: target,
        createdAt: "2026-01-01T00:00:00.000Z",
      })}\n`,
    );
    const stolen = acquireTargetLock({ targetPath: target, root, command: "recover" });
    releaseTargetLock(stolen);
  });

  test("withTargetLock releases on throw", () => {
    const root = scratch();
    const target = join(root, "ChatGPT.app");
    mkdirSync(target);
    expect(() =>
      withTargetLock({ targetPath: target, root, command: "install" }, () => {
        throw new Error("boom");
      }),
    ).toThrow("boom");
    const held = acquireTargetLock({ targetPath: target, root, command: "uninstall" });
    releaseTargetLock(held);
  });
});
