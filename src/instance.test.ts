import { describe, expect, test } from "bun:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  ownerMatchesLive,
  readOwnerLock,
  staleOwner,
  writeOwnerLock,
} from "./runtime/incodex-instance.cjs";

describe("instance owner", () => {
  test("a reused PID with a different start time is not the same process", () => {
    const owner = { pid: 12, startedAt: "Mon Aug 18 10:00:00 2026", execPath: "/A/ChatGPT" };
    const live = { pid: 12, startedAt: "Mon Aug 18 12:00:00 2026", execPath: "/A/ChatGPT" };
    expect(ownerMatchesLive(owner, live)).toBe(false);
  });

  test("the same pid, start time, and executable is the same process", () => {
    const owner = { pid: 12, startedAt: "Mon Aug 18 10:00:00 2026", execPath: "/A/ChatGPT" };
    expect(ownerMatchesLive(owner, owner)).toBe(true);
  });

  test("lock file is created exclusively and a dead owner is stale", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-lock-"));
    writeOwnerLock(root, {
      pid: 999999,
      startedAt: "never",
      execPath: "/nope",
      sessionId: "s",
      nonce: "n",
    });
    expect(readOwnerLock(root)?.pid).toBe(999999);
    expect(staleOwner(root)).toBe(true);
    expect(() =>
      writeOwnerLock(root, {
        pid: 1,
        startedAt: "x",
        execPath: "/x",
        sessionId: "s",
        nonce: "n",
      }),
    ).toThrow();
  });
});
