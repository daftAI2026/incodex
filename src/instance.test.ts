import { describe, expect, test } from "bun:test";
import { createServer } from "node:net";
import { mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  acquireOwnerLease,
  clearOwnerLock,
  connectExisting,
  connectExistingWithRetry,
  currentOwner,
  LOCK_NAME,
  listenForRaise,
  ownerMatchesLive,
  ownerPortFromExec,
  readOwnerLock,
  readOwnerRecords,
  readOwnerLockState,
  singleFlight,
  staleOwner,
  targetStateDir,
  writeOwnerLock,
  writeOwnerLockExclusive,
} from "./runtime/incodex-instance.cts";

const target = (root: string) => join(root, "target-executable");

describe("instance owner metadata", () => {
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
        env: { ...process.env, INCODEX_TEST_MODULE: join(import.meta.dir, "runtime/incodex-instance.cts"), PATH: "/definitely-missing-incodex-ps" },
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

  test("normalizes legacy full executable identities before liveness checks", () => {
    const owner = {
      pid: 12,
      startedAt: "Mon Aug 18 10:00:00 2026",
      execIdentity: "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
      execPath: "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
    };
    const live = { ...owner, execIdentity: "ChatGPT" };
    expect(ownerMatchesLive(owner, live)).toBe(true);
  });

  test("complete owner metadata is diagnostic and reports a dead process", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-lock-"));
    writeOwnerLock(root, { pid: 999999, startedAt: "never", execPath: "/nope", sessionId: "s", token: "n" });
    expect(readOwnerLock(root)?.pid).toBe(999999);
    expect(staleOwner(root)).toBe(true);
    writeOwnerLock(root, { pid: 1, startedAt: "x", execPath: "/x", sessionId: "s", token: "next" });
    expect(readOwnerLock(root)?.token).toBe("next");
  });

  test("legacy and partial records remain unverifiable", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-partial-owner-"));
    writeOwnerLock(root, { pid: process.pid, execPath: process.execPath, sessionId: "partial", token: "partial-live-token" });
    expect(readOwnerLockState(root).kind).toBe("unverifiable");
    const legacy = mkdtempSync(join(tmpdir(), "incodex-legacy-owner-"));
    writeOwnerLock(legacy, { pid: 999999, startedAt: "never", execPath: "/nope", sessionId: "legacy", token: "legacy-token" });
    expect(readOwnerLockState(legacy).kind).toBe("valid");
  });

  test("exclusive diagnostic publication refuses a concurrent record", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-exclusive-owner-"));
    const existing = currentOwner("existing", process.execPath);
    const candidate = currentOwner("candidate", process.execPath);
    writeOwnerLock(root, existing);
    expect(() => writeOwnerLockExclusive(root, candidate)).toThrow();
    expect(readOwnerLock(root)?.token).toBe(existing.token);
  });

  test("main preflight lets malformed owners reach acquisition recovery", () => {
    const source = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    expect(source).toContain("instance.readOwnerRecords(stateRoot())");
    expect(source).toContain('state.kind === "unverifiable"');
  });
});

describe("raise listener", () => {
  test("connectExisting bounds a streaming foreign response", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-streaming-raise-"));
    const owner = currentOwner("streaming-raise", target(root));
    writeOwnerLock(root, owner);
    const foreign = createServer((socket) => {
      const timer = setInterval(() => socket.write("x".repeat(128)), 10);
      socket.once("close", () => clearInterval(timer));
    });
    await new Promise<void>((resolve) => foreign.listen(ownerPortFromExec(owner.execPath), "127.0.0.1", resolve));
    try {
      const result = await Promise.race([
        connectExisting(root, 100, owner.token),
        new Promise<boolean>((resolve) => setTimeout(() => resolve(true), 1_000)),
      ]);
      expect(result).toBe(false);
    } finally {
      foreign.close();
    }
  });

  test("a valid owner listener accepts only its token", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tcp-sock-"));
    const owner = currentOwner("socket", target(root));
    await acquireOwnerLease(root, owner);
    let raised = false;
    const server = listenForRaise(root, () => {
      raised = true;
    }, owner);
    expect(server.listening).toBe(true);
    expect(await connectExisting(root, 500, "wrong-token")).toBe(false);
    expect(raised).toBe(false);
    expect(await connectExisting(root, 500, owner.token)).toBe(true);
    expect(raised).toBe(true);
    expect(ownerPortFromExec(owner.execPath)).toBeGreaterThan(0);
    expect(clearOwnerLock(root, owner)).toBe(true);
  });

  test("a live listener makes competing acquisition fail closed", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-live-owner-"));
    const current = currentOwner("current", target(root));
    const contender = currentOwner("contender", target(root));
    await acquireOwnerLease(root, current);
    await expect(acquireOwnerLease(root, contender)).rejects.toMatchObject({ code: "OWNER_BUSY" });
    expect(readOwnerLock(root)?.token).toBe(current.token);
    expect(clearOwnerLock(root, current)).toBe(true);
  });

  test("a truncated diagnostic record is preserved beside the active lease record", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-truncated-lock-"));
    const truncated = "{\"pid\":";
    writeFileSync(join(root, LOCK_NAME), truncated);
    const replacement = currentOwner("replacement", target(root));
    await acquireOwnerLease(root, replacement);
    expect(readOwnerLock(root)).toBeNull();
    expect(readOwnerRecords(root).some((record: any) => record.state.owner?.token === replacement.token)).toBe(true);
    expect(readFileSync(join(root, LOCK_NAME), "utf8")).toBe(truncated);
    expect(readdirSync(root).some((name) => name.includes("tmp"))).toBe(false);
    expect(clearOwnerLock(root, replacement)).toBe(true);
    expect(readFileSync(join(root, LOCK_NAME), "utf8")).toBe(truncated);
  });

  test("raise preflight skips a stale canonical record for the active sidecar lease", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-stale-diagnostic-"));
    const stale = currentOwner("stale", target(root));
    stale.pid = 99999999;
    writeOwnerLock(root, stale);
    const replacement = currentOwner("replacement", target(root));
    await acquireOwnerLease(root, replacement);
    let raised = false;
    listenForRaise(root, () => {
      raised = true;
    }, replacement);
    expect(await connectExisting(root, 500)).toBe(true);
    expect(raised).toBe(true);
    expect(clearOwnerLock(root, replacement)).toBe(true);
  });

  test("does not probe retained quarantines during raise retries", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-retained-quarantine-retry-"));
    for (const [index, token] of [
      "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
      "ffffffffffffffffffffffffffffffff",
      "11111111111111111111111111111111",
    ].entries()) {
      writeFileSync(
        join(root, `.incognito.lock.quarantine.retained-${index}`),
        `${JSON.stringify({
          pid: 910000 + index,
          startedAt: "dead-fixture",
          processStartIdentity: "dead-fixture",
          execPath: target(root),
          execIdentity: "target-executable",
          sessionId: `retained-${index}`,
          token,
          nonce: token,
        })}\n`,
      );
    }
    const port = ownerPortFromExec(target(root));
    let connections = 0;
    const server = createServer((socket) => {
      connections += 1;
      socket.destroy();
    });
    await new Promise<void>((resolve) => server.listen(port, "127.0.0.1", resolve));
    try {
      expect(await connectExistingWithRetry(root, "", { attempts: 5, timeoutMs: 100, delayMs: 0 })).toBe(false);
      expect(connections).toBe(0);
    } finally {
      server.close();
    }
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
    const [a, b, c] = await Promise.all([singleFlight(holder, start), singleFlight(holder, start), singleFlight(holder, start)]);
    expect(starts).toBe(1);
    expect([a, b, c]).toEqual([1, 1, 1]);
  });
});

describe("target isolation", () => {
  test("clone and live executables do not share state directories", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-targets-"));
    const live = targetStateDir(root, "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT");
    const clone = targetStateDir(root, "/Users/me/.incodex/scratch/ChatGPT.app/Contents/MacOS/ChatGPT");
    expect(live).not.toBe(clone);
  });
});
