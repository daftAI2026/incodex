import { describe, expect, test } from "bun:test";
import {
  mkdtempSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  acquireOwnerLease,
  connectExisting,
  connectExistingWithRetry,
  clearOwnerLock,
  currentOwner,
  LOCK_NAME,
  listenForRaise,
  ownerMatchesLive,
  readOwnerLock,
  readOwnerLockState,
  singleFlight,
  staleOwner,
  targetStateDir,
  writeOwnerLock,
} from "./runtime/incodex-instance.cts";

describe("instance owner", () => {
  test("currentOwner refuses to publish a lease without process identity", () => {
    const child = spawnSync(
      process.execPath,
      [
        "-e",
        `const { currentOwner } = require(process.env.INCODEX_TEST_MODULE);
try { currentOwner("identity", process.execPath); process.stdout.write("UNSAFE"); process.exit(2); }
catch (error) { process.stdout.write(String(error.message)); }`,
      ],
      {
        env: {
          ...process.env,
          INCODEX_TEST_MODULE: join(import.meta.dir, "runtime/incodex-instance.cts"),
          PATH: "/definitely-missing-incodex-ps",
        },
        encoding: "utf8",
      },
    );
    expect(child.status).toBe(0);
    expect(child.stdout).toMatch(/process identity/);
  });

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

  test("a partial live owner is unverifiable and cannot be taken over", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-partial-owner-"));
    const partial = {
      pid: process.pid,
      execPath: process.execPath,
      sessionId: "partial",
      token: "partial-live-token",
    };
    writeOwnerLock(root, partial);

    expect(readOwnerLockState(root).kind).toBe("unverifiable");
    expect(() => acquireOwnerLease(root, currentOwner("contender", process.execPath))).toThrow(/unverifiable/);
    expect(readOwnerLockState(root).owner?.token).toBe(partial.token);
  });

  test("legacy owner records require startedAt and executable identity", () => {
    const legacyRoot = mkdtempSync(join(tmpdir(), "incodex-legacy-owner-"));
    writeOwnerLock(legacyRoot, {
      pid: 999999,
      startedAt: "never",
      execPath: "/nope",
      sessionId: "legacy",
      token: "legacy-token",
    });
    expect(readOwnerLockState(legacyRoot).kind).toBe("valid");

    const missingStartRoot = mkdtempSync(join(tmpdir(), "incodex-missing-start-"));
    writeOwnerLock(missingStartRoot, {
      pid: process.pid,
      execPath: process.execPath,
      sessionId: "missing-start",
      token: "missing-start-token",
    });
    expect(readOwnerLockState(missingStartRoot).kind).toBe("unverifiable");

    const missingExecRoot = mkdtempSync(join(tmpdir(), "incodex-missing-exec-"));
    const live = currentOwner("missing-exec", process.execPath);
    delete (live as Record<string, unknown>).execPath;
    delete (live as Record<string, unknown>).execIdentity;
    writeOwnerLock(missingExecRoot, live);
    expect(readOwnerLockState(missingExecRoot).kind).toBe("unverifiable");
  });

  test("an old owner cannot clear a replacement lease", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-lease-"));
    const oldOwner = {
      pid: 12,
      startedAt: "Mon Aug 18 10:00:00 2026",
      execPath: "/A/ChatGPT",
      sessionId: "old",
      token: "old-token",
    };
    const newOwner = {
      pid: 13,
      startedAt: "Mon Aug 18 10:01:00 2026",
      execPath: "/A/ChatGPT",
      sessionId: "new",
      token: "new-token",
    };
    writeOwnerLock(root, newOwner);

    expect(clearOwnerLock(root, oldOwner)).toBe(false);
    expect(readOwnerLock(root)?.token).toBe(newOwner.token);
  });
});

describe("raise socket", () => {
  test("a client that hangs up does not crash the listener", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-sock-"));
    const owner = currentOwner("socket", process.execPath);
    writeOwnerLock(root, owner);
    const server = listenForRaise(root, () => {}, owner);
    await new Promise((resolve, reject) => {
      server.once("listening", resolve);
      server.once("error", reject);
    });
    const ok = await connectExisting(root, 500, owner.token);
    expect(ok).toBe(true);
    server.close();
  });

  test("a raise request must carry the current owner token", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-sock-token-"));
    const owner = {
      pid: process.pid,
      startedAt: "Mon Aug 18 10:00:00 2026",
      execPath: process.execPath,
      sessionId: "s",
      token: "owner-token",
    };
    writeOwnerLock(root, owner);
    let raised = false;
    const server = listenForRaise(root, () => {
      raised = true;
    }, owner);
    await new Promise((resolve, reject) => {
      server.once("listening", resolve);
      server.once("error", reject);
    });

    const ok = await connectExisting(root, 500, "wrong-token");

    expect(ok).toBe(false);
    expect(raised).toBe(false);
    server.close();
  });

  test("a live owner makes competing acquisition fail closed", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-live-owner-"));
    const current = currentOwner("current", process.execPath);
    const contender = currentOwner("contender", process.execPath);
    acquireOwnerLease(root, current);

    expect(() => acquireOwnerLease(root, contender)).toThrow(/another Incognito owner is active/);
    expect(readOwnerLock(root)?.token).toBe(current.token);
  });

  test("twenty contenders produce exactly one owner", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-contenders-"));
    const contenders = Array.from({ length: 20 }, (_, index) => currentOwner(`contender-${index}`, process.execPath));
    let winners = 0;
    let winner = null;
    for (const contender of contenders) {
      try {
        winner = acquireOwnerLease(root, contender);
        winners += 1;
      } catch (error) {
        expect((error as { code?: string }).code).toBe("OWNER_BUSY");
      }
    }

    expect(winners).toBe(1);
    expect(readOwnerLock(root)?.token).toBe(winner?.token);
    expect(clearOwnerLock(root, winner)).toBe(true);
  });

  test("a stale owner can be replaced without deleting the new lease", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-stale-owner-"));
    writeOwnerLock(root, {
      pid: 999999,
      startedAt: "never",
      execPath: "/nope",
      sessionId: "stale",
      token: "stale-token",
    });
    const replacement = currentOwner("replacement", process.execPath);

    acquireOwnerLease(root, replacement);

    expect(readOwnerLock(root)?.token).toBe(replacement.token);
    expect(clearOwnerLock(root, { token: "stale-token" })).toBe(false);
    expect(readOwnerLock(root)?.token).toBe(replacement.token);
  });

  test("a truncated lock is quarantined so a crash cannot poison the next launch", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-truncated-lock-"));
    const truncated = "{\"pid\":";
    writeFileSync(join(root, LOCK_NAME), truncated);
    const replacement = currentOwner("replacement", process.execPath);

    acquireOwnerLease(root, replacement);

    const quarantine = readdirSync(root).find((name) => name.startsWith(`.${LOCK_NAME}.invalid.`));
    expect(quarantine).toBeString();
    expect(readFileSync(join(root, quarantine as string), "utf8")).toBe(truncated);
    expect(readOwnerLock(root)?.token).toBe(replacement.token);
  });

  test("a malformed owner is recoverable by the acquisition path", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-malformed-owner-"));
    writeFileSync(join(root, LOCK_NAME), "{\"pid\":");
    const replacement = currentOwner("malformed-recovery", process.execPath);

    expect(() => acquireOwnerLease(root, replacement)).not.toThrow();
    expect(readOwnerLock(root)?.token).toBe(replacement.token);
  });

  test("main preflight delegates malformed owners to acquisition recovery", () => {
    const mainSource = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    expect(mainSource).toContain('if (state.kind === "invalid") return false;');
  });

  test("retrying a delayed socket does not create a second owner", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-delayed-socket-"));
    const owner = currentOwner("delayed", process.execPath);
    writeOwnerLock(root, owner);
    const serverPromise = new Promise<ReturnType<typeof listenForRaise>>((resolve, reject) => {
      setTimeout(() => {
        try {
          const server = listenForRaise(root, () => {}, owner);
          server.once("error", reject);
          resolve(server);
        } catch (error) {
          reject(error);
        }
      }, 80);
    });

    const connected = await connectExistingWithRetry(root, owner.token, {
      attempts: 8,
      timeoutMs: 40,
      delayMs: 30,
    });
    const server = await serverPromise;

    expect(connected).toBe(true);
    expect(readOwnerLock(root)?.token).toBe(owner.token);
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
