import { describe, expect, test } from "bun:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  connectExisting,
  listenForRaise,
  ownerMatchesLive,
  readOwnerLock,
  singleFlight,
  staleOwner,
  targetStateDir,
  writeOwnerLock,
} from "./runtime/incodex-instance.cts";

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

describe("raise socket", () => {
  test("a client that hangs up does not crash the listener", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-sock-"));
    const server = listenForRaise(root, () => {});
    await new Promise((resolve, reject) => {
      server.once("listening", resolve);
      server.once("error", reject);
    });
    const ok = await connectExisting(root, 500);
    expect(ok).toBe(true);
    server.close();
  });
});

describe("concurrent launch", () => {
  test("overlapping calls share one in-flight launch", async () => {
    const holder: { current: Promise<number> | null } = { current: null };
    let starts = 0;
    const start = () => {
      starts += 1;
      return new Promise<number>((resolve) => setTimeout(() => resolve(starts), 20));
    };
    const [a, b, c] = await Promise.all([
      singleFlight(holder, start),
      singleFlight(holder, start),
      singleFlight(holder, start),
    ]);
    expect(starts).toBe(1);
    expect(a).toBe(1);
    expect(b).toBe(1);
    expect(c).toBe(1);
  });

  test("a later call after the first finishes may start again", async () => {
    const holder: { current: Promise<number> | null } = { current: null };
    let starts = 0;
    const start = async () => {
      starts += 1;
      return starts;
    };
    await singleFlight(holder, start);
    await singleFlight(holder, start);
    expect(starts).toBe(2);
  });
});

describe("target isolation", () => {
  test("clone and live executables do not share lock, socket, or log directories", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-lock-"));
    const live = targetStateDir(root, "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT");
    const clone = targetStateDir(root, "/Users/me/.incodex/scratch/ChatGPT.app/Contents/MacOS/ChatGPT");
    expect(live).not.toBe(clone);
    writeOwnerLock(live, {
      pid: 11,
      startedAt: "a",
      execPath: "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
      sessionId: "live",
      nonce: "n1",
    });
    expect(readOwnerLock(clone)).toBeNull();
    expect(readOwnerLock(live)?.sessionId).toBe("live");
  });
});
