import { describe, expect, test } from "bun:test";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  renameSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { spawn, spawnSync } from "node:child_process";
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

describe("cross-process owner contention", () => {
  test("twenty OS processes produce one winner and never steal its lease", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-os-contenders-"));
    const barrier = join(root, "start");
    const readyRoot = join(root, "ready");
    const doneRoot = join(root, "done");
    const releaseFile = join(root, "release");
    mkdirSync(readyRoot);
    mkdirSync(doneRoot);
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    const worker = String.raw`
      const { existsSync, writeFileSync } = require("node:fs");
      const { join } = require("node:path");
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const root = process.env.INCODEX_TEST_ROOT;
      const barrier = process.env.INCODEX_TEST_BARRIER;
      const readyRoot = process.env.INCODEX_TEST_READY_ROOT;
      const doneRoot = process.env.INCODEX_TEST_DONE_ROOT;
      const releaseFile = process.env.INCODEX_TEST_RELEASE_FILE;
      const index = process.env.INCODEX_TEST_INDEX;
      writeFileSync(join(readyRoot, index), "ready\n");
      while (!existsSync(barrier)) {
        const until = Date.now() + 5;
        while (Date.now() < until) {}
      }
      const owner = instance.currentOwner("worker", process.execPath);
      let exitCode = 0;
      try {
        instance.acquireOwnerLease(root, owner);
        process.stdout.write("WINNER\n");
      } catch (error) {
        process.stdout.write(String(error.code || "ERROR") + "\n");
        exitCode = error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 2 : 3;
      }
      writeFileSync(join(doneRoot, index), "done\n");
      const waiter = new Int32Array(new SharedArrayBuffer(4));
      while (!existsSync(releaseFile)) {
        Atomics.wait(waiter, 0, 0, 5);
      }
      process.exit(exitCode);
    `;
    const children = Array.from({ length: 20 }, (_, index) =>
      spawn(process.execPath, ["-e", worker], {
        env: {
          ...process.env,
          INCODEX_TEST_MODULE: modulePath,
          INCODEX_TEST_ROOT: root,
          INCODEX_TEST_BARRIER: barrier,
          INCODEX_TEST_READY_ROOT: readyRoot,
          INCODEX_TEST_DONE_ROOT: doneRoot,
          INCODEX_TEST_RELEASE_FILE: releaseFile,
          INCODEX_TEST_INDEX: String(index),
        },
        stdio: ["ignore", "pipe", "pipe"],
      }),
    );
    const results = children.map(
      (child) =>
        new Promise<{ code: number | null; stdout: string; stderr: string }>((resolve) => {
          let stdout = "";
          let stderr = "";
          child.stdout?.on("data", (chunk) => (stdout += String(chunk)));
          child.stderr?.on("data", (chunk) => (stderr += String(chunk)));
          child.once("close", (code) => resolve({ code, stdout, stderr }));
          child.once("error", (error) => resolve({ code: 3, stdout, stderr: String(error) }));
        }),
    );
    const waitForCount = async (directory: string, expected: number) => {
      const deadline = Date.now() + 5000;
      while (readdirSync(directory).length < expected && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
      return readdirSync(directory).length === expected;
    };
    const allReady = await waitForCount(readyRoot, children.length);
    writeFileSync(barrier, "go\n");
    const allDone = await waitForCount(doneRoot, children.length);
    writeFileSync(releaseFile, "release\n");
    const completed = await Promise.all(results);
    const winners = completed.filter((result) => result.stdout.trim() === "WINNER");
    const rejected = completed.filter((result) => ["OWNER_BUSY", "OWNER_RACE"].includes(result.stdout.trim()));

    expect(winners).toHaveLength(1);
    expect(rejected).toHaveLength(19);
    expect(allReady).toBe(true);
    expect(allDone).toBe(true);
    expect(completed.every((result) => result.code === 0 || result.code === 2)).toBe(true);
    expect(completed.every((result) => result.stderr === "")).toBe(true);
  }, 30_000);

  test("twenty OS processes replacing a stale owner preserve the winner lease", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-os-stale-contenders-"));
    const barrier = join(root, "start");
    const readyRoot = join(root, "ready");
    const doneRoot = join(root, "done");
    const releaseFile = join(root, "release");
    mkdirSync(readyRoot);
    mkdirSync(doneRoot);
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    writeFileSync(join(root, LOCK_NAME), JSON.stringify({
      pid: 999999,
      startedAt: "never",
      execPath: "/nope",
      sessionId: "stale",
      token: "stale-token",
    }));
    const worker = String.raw`
      const { existsSync, writeFileSync } = require("node:fs");
      const { join } = require("node:path");
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const root = process.env.INCODEX_TEST_ROOT;
      const barrier = process.env.INCODEX_TEST_BARRIER;
      const readyRoot = process.env.INCODEX_TEST_READY_ROOT;
      const doneRoot = process.env.INCODEX_TEST_DONE_ROOT;
      const releaseFile = process.env.INCODEX_TEST_RELEASE_FILE;
      const index = process.env.INCODEX_TEST_INDEX;
      writeFileSync(join(readyRoot, index), "ready\n");
      while (!existsSync(barrier)) {
        const until = Date.now() + 5;
        while (Date.now() < until) {}
      }
      const owner = instance.currentOwner("stale-replacement", process.execPath);
      let exitCode = 0;
      try {
        instance.acquireOwnerLease(root, owner);
        process.stdout.write("WINNER " + owner.token + "\n");
      } catch (error) {
        process.stdout.write(String(error.code || "ERROR") + "\n");
        exitCode = error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 2 : 3;
      }
      writeFileSync(join(doneRoot, index), "done\n");
      const waiter = new Int32Array(new SharedArrayBuffer(4));
      while (!existsSync(releaseFile)) {
        Atomics.wait(waiter, 0, 0, 5);
      }
      process.exit(exitCode);
    `;
    const children = Array.from({ length: 20 }, (_, index) =>
      spawn(process.execPath, ["-e", worker], {
        env: {
          ...process.env,
          INCODEX_TEST_MODULE: modulePath,
          INCODEX_TEST_ROOT: root,
          INCODEX_TEST_BARRIER: barrier,
          INCODEX_TEST_READY_ROOT: readyRoot,
          INCODEX_TEST_DONE_ROOT: doneRoot,
          INCODEX_TEST_RELEASE_FILE: releaseFile,
          INCODEX_TEST_INDEX: String(index),
        },
        stdio: ["ignore", "pipe", "pipe"],
      }),
    );
    const results = children.map(
      (child) =>
        new Promise<{ code: number | null; stdout: string; stderr: string }>((resolve) => {
          let stdout = "";
          let stderr = "";
          child.stdout?.on("data", (chunk) => (stdout += String(chunk)));
          child.stderr?.on("data", (chunk) => (stderr += String(chunk)));
          child.once("close", (code) => resolve({ code, stdout, stderr }));
          child.once("error", (error) => resolve({ code: 3, stdout, stderr: String(error) }));
        }),
    );
    const waitForCount = async (directory: string, expected: number) => {
      const deadline = Date.now() + 5000;
      while (readdirSync(directory).length < expected && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
      return readdirSync(directory).length === expected;
    };
    const allReady = await waitForCount(readyRoot, children.length);
    writeFileSync(barrier, "go\n");
    const allDone = await waitForCount(doneRoot, children.length);
    writeFileSync(releaseFile, "release\n");
    const completed = await Promise.all(results);
    const winners = completed.filter((result) => result.stdout.trim().startsWith("WINNER "));
    const rejected = completed.filter((result) => ["OWNER_BUSY", "OWNER_RACE"].includes(result.stdout.trim()));
    const winnerToken = winners[0]?.stdout.trim().slice("WINNER ".length);

    expect(winners).toHaveLength(1);
    expect(rejected).toHaveLength(19);
    expect(allReady).toBe(true);
    expect(allDone).toBe(true);
    expect(winnerToken).toBeString();
    expect(readOwnerLock(root)?.token).toBe(winnerToken);
    expect(completed.every((result) => result.code === 0 || result.code === 2)).toBe(true);
    expect(completed.every((result) => result.stderr === "")).toBe(true);
  }, 30_000);

  test("stale takeover claims have one OS winner and cannot delete its lease", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-os-stale-claim-"));
    const barrier = join(root, "start");
    const readyRoot = join(root, "ready");
    const doneRoot = join(root, "done");
    const releaseFile = join(root, "release");
    mkdirSync(readyRoot);
    mkdirSync(doneRoot);
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    const claimRoot = join(root, ".incognito.lock.takeover");
    mkdirSync(claimRoot);
    writeFileSync(
      join(claimRoot, "owner"),
      JSON.stringify({
        pid: 999999,
        startedAt: "never",
        processStartIdentity: "never",
        execIdentity: "/nope",
        token: "stale-claim-token",
      }),
    );
    writeFileSync(
      join(root, LOCK_NAME),
      JSON.stringify({
        pid: 999999,
        startedAt: "never",
        execPath: "/nope",
        sessionId: "stale",
        token: "stale-token",
      }),
    );
    const worker = String.raw`
      const { existsSync, writeFileSync } = require("node:fs");
      const { join } = require("node:path");
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const root = process.env.INCODEX_TEST_ROOT;
      const barrier = process.env.INCODEX_TEST_BARRIER;
      const readyRoot = process.env.INCODEX_TEST_READY_ROOT;
      const doneRoot = process.env.INCODEX_TEST_DONE_ROOT;
      const releaseFile = process.env.INCODEX_TEST_RELEASE_FILE;
      const index = process.env.INCODEX_TEST_INDEX;
      writeFileSync(join(readyRoot, index), "ready\n");
      while (!existsSync(barrier)) {
        const until = Date.now() + 5;
        while (Date.now() < until) {}
      }
      const owner = instance.currentOwner("stale-claim-replacement", process.execPath);
      let exitCode = 0;
      try {
        instance.acquireOwnerLease(root, owner);
        process.stdout.write("WINNER " + owner.token + "\n");
      } catch (error) {
        process.stdout.write(String(error.code || "ERROR") + "\n");
        exitCode = error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 2 : 3;
      }
      writeFileSync(join(doneRoot, index), "done\n");
      const waiter = new Int32Array(new SharedArrayBuffer(4));
      while (!existsSync(releaseFile)) {
        Atomics.wait(waiter, 0, 0, 5);
      }
      process.exit(exitCode);
    `;
    const children = Array.from({ length: 8 }, (_, index) =>
      spawn(process.execPath, ["-e", worker], {
        env: {
          ...process.env,
          INCODEX_TEST_MODULE: modulePath,
          INCODEX_TEST_ROOT: root,
          INCODEX_TEST_BARRIER: barrier,
          INCODEX_TEST_READY_ROOT: readyRoot,
          INCODEX_TEST_DONE_ROOT: doneRoot,
          INCODEX_TEST_RELEASE_FILE: releaseFile,
          INCODEX_TEST_INDEX: String(index),
        },
        stdio: ["ignore", "pipe", "pipe"],
      }),
    );
    const results = children.map(
      (child) =>
        new Promise<{ code: number | null; stdout: string; stderr: string }>((resolve) => {
          let stdout = "";
          let stderr = "";
          child.stdout?.on("data", (chunk) => (stdout += String(chunk)));
          child.stderr?.on("data", (chunk) => (stderr += String(chunk)));
          child.once("close", (code) => resolve({ code, stdout, stderr }));
          child.once("error", (error) => resolve({ code: 3, stdout, stderr: String(error) }));
        }),
    );
    const waitForCount = async (directory: string, expected: number) => {
      const deadline = Date.now() + 5000;
      while (readdirSync(directory).length < expected && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
      return readdirSync(directory).length === expected;
    };
    const allReady = await waitForCount(readyRoot, children.length);
    writeFileSync(barrier, "go\n");
    const allDone = await waitForCount(doneRoot, children.length);
    writeFileSync(releaseFile, "release\n");
    const completed = await Promise.all(results);
    const winners = completed.filter((result) => result.stdout.trim().startsWith("WINNER "));
    const rejected = completed.filter((result) => ["OWNER_BUSY", "OWNER_RACE"].includes(result.stdout.trim()));
    const winnerToken = winners[0]?.stdout.trim().slice("WINNER ".length);

    expect(winners).toHaveLength(1);
    expect(rejected).toHaveLength(7);
    expect(allReady).toBe(true);
    expect(allDone).toBe(true);
    expect(winnerToken).toBeString();
    expect(readOwnerLock(root)?.token).toBe(winnerToken);
    expect(completed.every((result) => result.code === 0 || result.code === 2)).toBe(true);
    expect(completed.every((result) => result.stderr === "")).toBe(true);
  }, 30_000);

  test("a crashed reclaim marker is recoverable without poisoning takeover", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-crashed-reclaim-"));
    const claimRoot = join(root, ".incognito.lock.takeover");
    const reclaimRoot = join(claimRoot, ".reclaim");
    mkdirSync(reclaimRoot, { recursive: true });
    writeFileSync(
      join(claimRoot, "owner"),
      JSON.stringify({
        pid: 999999,
        startedAt: "never",
        processStartIdentity: "never",
        execIdentity: "/nope",
        token: "stale-claim-token",
      }),
    );
    writeFileSync(
      join(reclaimRoot, "marker.00000000000001"),
      JSON.stringify({
        pid: 999999,
        startedAt: "never",
        processStartIdentity: "never",
        execIdentity: "/nope",
        token: "crashed-reclaimer-token",
      }),
    );
    writeFileSync(
      join(root, LOCK_NAME),
      JSON.stringify({
        pid: 999999,
        startedAt: "never",
        execPath: "/nope",
        sessionId: "stale",
        token: "stale-token",
      }),
    );

    const replacement = currentOwner("crashed-reclaim-replacement", process.execPath);
    acquireOwnerLease(root, replacement);

    expect(readOwnerLock(root)?.token).toBe(replacement.token);
    expect(!existsSync(claimRoot) || !readdirSync(join(claimRoot, ".reclaim")).some((name) => name.startsWith("marker."))).toBe(
      true,
    );
  });

  test("a reclaimer cannot hand off a marker that another process replaced", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-reclaim-marker-window-"));
    const claimRoot = join(root, ".incognito.lock.takeover");
    const reclaimRoot = join(claimRoot, ".reclaim");
    const pauseFile = join(root, "reclaim-paused");
    const releaseFile = join(root, "reclaim-release");
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    mkdirSync(reclaimRoot, { recursive: true });
    writeFileSync(
      join(claimRoot, "owner"),
      JSON.stringify({
        pid: 999999,
        startedAt: "never",
        processStartIdentity: "never",
        execIdentity: "/nope",
        token: "stale-claim-token",
      }),
    );
    writeFileSync(
      join(root, LOCK_NAME),
      JSON.stringify({
        pid: 999999,
        startedAt: "never",
        execPath: "/nope",
        sessionId: "stale",
        token: "stale-token",
      }),
    );
    writeFileSync(
      join(reclaimRoot, "marker.0000000000000001"),
      JSON.stringify({
        pid: 999999,
        startedAt: "never",
        processStartIdentity: "never",
        execIdentity: "/nope",
        token: "stale-reclaimer-token",
      }),
    );

    const worker = String.raw`
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const root = process.env.INCODEX_TEST_ROOT;
      const owner = instance.currentOwner("reclaim-marker-cleaner", process.execPath);
      try {
        instance.acquireOwnerLease(root, owner);
        process.stdout.write("UNEXPECTED_WINNER\n");
        process.exit(3);
      } catch (error) {
        process.stdout.write(String(error.code || "ERROR") + "\n");
        process.exit(error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 0 : 2);
      }
    `;
    const child = spawn(process.execPath, ["-e", worker], {
      env: {
        ...process.env,
        INCODEX_TEST_MODULE: modulePath,
        INCODEX_TEST_ROOT: root,
        INCODEX_TEST_RECLAIM_HANDOFF_PAUSE_FILE: pauseFile,
        INCODEX_TEST_RECLAIM_HANDOFF_RELEASE_FILE: releaseFile,
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let closed = false;
    child.stdout?.on("data", (chunk) => (stdout += String(chunk)));
    child.stderr?.on("data", (chunk) => (stderr += String(chunk)));
    const closePromise = new Promise<{ code: number | null }>((resolve) => {
      child.once("close", (code) => {
        closed = true;
        resolve({ code });
      });
      child.once("error", () => {
        closed = true;
        resolve({ code: 2 });
      });
    });

    try {
      const deadline = Date.now() + 3000;
      while (!existsSync(pauseFile) && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
      expect(existsSync(pauseFile)).toBe(true);
      if (!existsSync(pauseFile)) return;

      const oldReclaim = join(claimRoot, ".reclaim");
      const quarantined = join(root, ".reclaim-before-replacement");
      renameSync(oldReclaim, quarantined);
      mkdirSync(oldReclaim);
      const replacement = currentOwner("replacement-reclaimer", process.execPath);
      writeFileSync(join(oldReclaim, "marker.0000000000000002"), JSON.stringify(replacement));
      writeFileSync(releaseFile, "release\n");

      const result = await closePromise;
      expect(result.code).toBe(0);
      expect(["OWNER_BUSY", "OWNER_RACE"]).toContain(stdout.trim());
      expect(stderr).toBe("");
      expect(readFileSync(join(oldReclaim, "marker.0000000000000002"), "utf8")).toContain(replacement.token);
    } finally {
      writeFileSync(releaseFile, "release\n");
      if (!closed) child.kill();
      await closePromise;
    }
  });

  test("a foreign regular takeover residue fails closed without deletion", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-foreign-claim-"));
    const claimPath = join(root, ".incognito.lock.takeover");
    const foreignRecord = JSON.stringify({ pid: 999999, token: "foreign-record" });
    writeFileSync(claimPath, foreignRecord);
    writeFileSync(
      join(root, LOCK_NAME),
      JSON.stringify({
        pid: 999999,
        startedAt: "never",
        execPath: "/nope",
        sessionId: "stale",
        token: "stale-owner-token",
      }),
    );

    let caught: unknown;
    try {
      acquireOwnerLease(root, currentOwner("foreign-claim", process.execPath));
    } catch (error) {
      caught = error;
    }

    expect((caught as { code?: string })?.code).toBe("OWNER_FOREIGN_CLAIM");
    expect(String((caught as { message?: string })?.message)).toMatch(/foreign regular file/);
    expect(readFileSync(claimPath, "utf8")).toBe(foreignRecord);
  });

  test("malformed reclaim generations fail closed without cleanup", () => {
    for (const generation of ["9007199254740992", "00000000000000000001", "0"]) {
      const root = mkdtempSync(join(tmpdir(), "incodex-malformed-generation-"));
      const claimRoot = join(root, ".incognito.lock.takeover");
      const reclaimRoot = join(claimRoot, ".reclaim");
      mkdirSync(reclaimRoot, { recursive: true });
      writeFileSync(
        join(claimRoot, "owner"),
        JSON.stringify({
          pid: 999999,
          startedAt: "never",
          processStartIdentity: "never",
          execIdentity: "/nope",
          token: "stale-claim-token",
        }),
      );
      const marker = join(reclaimRoot, `marker.${generation}`);
      writeFileSync(
        marker,
        JSON.stringify({
          pid: 999999,
          startedAt: "never",
          processStartIdentity: "never",
          execIdentity: "/nope",
          token: `malformed-${generation}`,
        }),
      );
      writeFileSync(
        join(root, LOCK_NAME),
        JSON.stringify({
          pid: 999999,
          startedAt: "never",
          execPath: "/nope",
          sessionId: "stale",
          token: "stale-owner-token",
        }),
      );

      let caught: unknown;
      try {
        acquireOwnerLease(root, currentOwner(`malformed-${generation}`, process.execPath));
      } catch (error) {
        caught = error;
      }

      expect((caught as { code?: string })?.code).toBe("OWNER_RECLAIM_UNREADABLE");
      expect(String((caught as { message?: string })?.message)).toMatch(/generation/);
      expect(readFileSync(marker, "utf8")).toContain(`malformed-${generation}`);
      expect(readFileSync(join(claimRoot, "owner"), "utf8")).toContain("stale-claim-token");
    }
  });

  test("a stale claim cleaner cannot remove a replacement claim in its unlink window", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-takeover-claim-window-"));
    const claimRoot = join(root, ".incognito.lock.takeover");
    const pauseFile = join(root, "claim-paused");
    const releaseFile = join(root, "claim-release");
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    mkdirSync(claimRoot);
    writeFileSync(
      join(claimRoot, "owner"),
      JSON.stringify({
        pid: 999999,
        startedAt: "never",
        processStartIdentity: "never",
        execIdentity: "/nope",
        token: "stale-claim-token",
      }),
    );
    writeFileSync(
      join(root, LOCK_NAME),
      JSON.stringify({
        pid: 999999,
        startedAt: "never",
        execPath: "/nope",
        sessionId: "stale",
        token: "stale-token",
      }),
    );

    const worker = String.raw`
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const root = process.env.INCODEX_TEST_ROOT;
      const owner = instance.currentOwner("claim-cleaner", process.execPath);
      try {
        instance.acquireOwnerLease(root, owner);
        process.stdout.write("UNEXPECTED_WINNER\n");
        process.exit(3);
      } catch (error) {
        process.stdout.write(String(error.code || "ERROR") + "\n");
        process.exit(error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 0 : 2);
      }
    `;
    const child = spawn(process.execPath, ["-e", worker], {
      env: {
        ...process.env,
        INCODEX_TEST_MODULE: modulePath,
        INCODEX_TEST_ROOT: root,
        INCODEX_TEST_TAKEOVER_PAUSE_FILE: pauseFile,
        INCODEX_TEST_TAKEOVER_RELEASE_FILE: releaseFile,
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let closed = false;
    child.stdout?.on("data", (chunk) => (stdout += String(chunk)));
    child.stderr?.on("data", (chunk) => (stderr += String(chunk)));
    const closePromise = new Promise<{ code: number | null }>((resolve) => {
      child.once("close", (code) => {
        closed = true;
        resolve({ code });
      });
      child.once("error", () => {
        closed = true;
        resolve({ code: 2 });
      });
    });

    try {
      const deadline = Date.now() + 3000;
      while (!existsSync(pauseFile) && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
      expect(existsSync(pauseFile)).toBe(true);
      if (!existsSync(pauseFile)) return;

      const oldClaim = join(root, ".incognito.lock.takeover");
      const quarantine = join(root, ".stale-claim-test");
      renameSync(oldClaim, quarantine);
      const replacement = currentOwner("claim-replacement", process.execPath);
      mkdirSync(oldClaim);
      writeFileSync(join(oldClaim, "owner"), JSON.stringify(replacement));
      writeFileSync(releaseFile, "release\n");

      const result = await closePromise;
      expect(result.code).toBe(0);
      expect(["OWNER_BUSY", "OWNER_RACE"]).toContain(stdout.trim());
      expect(stderr).toBe("");
      expect(readFileSync(join(oldClaim, "owner"), "utf8")).toContain(replacement.token);
    } finally {
      writeFileSync(releaseFile, "release\n");
      if (!closed) child.kill();
      await closePromise;
    }
  });

  test("twenty OS processes recover one truncated lock without deleting the winner", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-os-invalid-contenders-"));
    const barrier = join(root, "start");
    const readyRoot = join(root, "ready");
    const doneRoot = join(root, "done");
    const releaseFile = join(root, "release");
    mkdirSync(readyRoot);
    mkdirSync(doneRoot);
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    const truncated = "{\"pid\":";
    writeFileSync(join(root, LOCK_NAME), truncated);
    const worker = String.raw`
      const { existsSync, writeFileSync } = require("node:fs");
      const { join } = require("node:path");
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const root = process.env.INCODEX_TEST_ROOT;
      const barrier = process.env.INCODEX_TEST_BARRIER;
      const readyRoot = process.env.INCODEX_TEST_READY_ROOT;
      const doneRoot = process.env.INCODEX_TEST_DONE_ROOT;
      const releaseFile = process.env.INCODEX_TEST_RELEASE_FILE;
      const index = process.env.INCODEX_TEST_INDEX;
      writeFileSync(join(readyRoot, index), "ready\n");
      while (!existsSync(barrier)) {
        const until = Date.now() + 5;
        while (Date.now() < until) {}
      }
      const owner = instance.currentOwner("invalid-recovery", process.execPath);
      let exitCode = 0;
      try {
        instance.acquireOwnerLease(root, owner);
        process.stdout.write("WINNER " + owner.token + "\n");
      } catch (error) {
        process.stdout.write(String(error.code || "ERROR") + "\n");
        exitCode = error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 2 : 3;
      }
      writeFileSync(join(doneRoot, index), "done\n");
      const waiter = new Int32Array(new SharedArrayBuffer(4));
      while (!existsSync(releaseFile)) {
        Atomics.wait(waiter, 0, 0, 5);
      }
      process.exit(exitCode);
    `;
    const children = Array.from({ length: 20 }, (_, index) =>
      spawn(process.execPath, ["-e", worker], {
        env: {
          ...process.env,
          INCODEX_TEST_MODULE: modulePath,
          INCODEX_TEST_ROOT: root,
          INCODEX_TEST_BARRIER: barrier,
          INCODEX_TEST_READY_ROOT: readyRoot,
          INCODEX_TEST_DONE_ROOT: doneRoot,
          INCODEX_TEST_RELEASE_FILE: releaseFile,
          INCODEX_TEST_INDEX: String(index),
        },
        stdio: ["ignore", "pipe", "pipe"],
      }),
    );
    const results = children.map(
      (child) =>
        new Promise<{ code: number | null; stdout: string; stderr: string }>((resolve) => {
          let stdout = "";
          let stderr = "";
          child.stdout?.on("data", (chunk) => (stdout += String(chunk)));
          child.stderr?.on("data", (chunk) => (stderr += String(chunk)));
          child.once("close", (code) => resolve({ code, stdout, stderr }));
          child.once("error", (error) => resolve({ code: 3, stdout, stderr: String(error) }));
        }),
    );
    const waitForCount = async (directory: string, expected: number) => {
      const deadline = Date.now() + 5000;
      while (readdirSync(directory).length < expected && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
      return readdirSync(directory).length === expected;
    };
    const allReady = await waitForCount(readyRoot, children.length);
    writeFileSync(barrier, "go\n");
    const allDone = await waitForCount(doneRoot, children.length);
    writeFileSync(releaseFile, "release\n");
    const completed = await Promise.all(results);
    const winners = completed.filter((result) => result.stdout.trim().startsWith("WINNER "));
    const rejected = completed.filter((result) => ["OWNER_BUSY", "OWNER_RACE"].includes(result.stdout.trim()));
    const winnerToken = winners[0]?.stdout.trim().slice("WINNER ".length);
    const quarantine = readdirSync(root).filter((name) => name.startsWith(`.${LOCK_NAME}.invalid.`));

    expect(winners).toHaveLength(1);
    expect(rejected).toHaveLength(19);
    expect(allReady).toBe(true);
    expect(allDone).toBe(true);
    expect(winnerToken).toBeString();
    expect(quarantine.length).toBeGreaterThan(0);
    expect(readOwnerLock(root)?.token).toBe(winnerToken);
    expect(completed.every((result) => result.code === 0 || result.code === 2)).toBe(true);
    expect(completed.every((result) => result.stderr === "")).toBe(true);
  }, 30_000);

  test("a replacement published during takeover unlink survives the old cleaner", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-takeover-window-"));
    const pauseFile = join(root, "takeover-paused");
    const releaseFile = join(root, "takeover-release");
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    writeFileSync(
      join(root, LOCK_NAME),
      JSON.stringify({
        pid: 999999,
        startedAt: "never",
        execPath: "/nope",
        sessionId: "stale",
        token: "stale-token",
      }),
    );

    const worker = String.raw`
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const root = process.env.INCODEX_TEST_ROOT;
      const owner = instance.currentOwner("old-cleaner", process.execPath);
      try {
        instance.acquireOwnerLease(root, owner);
        process.stdout.write("UNEXPECTED_WINNER\n");
        process.exit(3);
      } catch (error) {
        process.stdout.write(String(error.code || "ERROR") + "\n");
        process.exit(error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 0 : 2);
      }
    `;
    const child = spawn(process.execPath, ["-e", worker], {
      env: {
        ...process.env,
        INCODEX_TEST_MODULE: modulePath,
        INCODEX_TEST_ROOT: root,
        INCODEX_TEST_TAKEOVER_PAUSE_FILE: pauseFile,
        INCODEX_TEST_TAKEOVER_RELEASE_FILE: releaseFile,
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let closed = false;
    child.stdout?.on("data", (chunk) => (stdout += String(chunk)));
    child.stderr?.on("data", (chunk) => (stderr += String(chunk)));
    const closePromise = new Promise<{ code: number | null }>((resolve) => {
      child.once("close", (code) => {
        closed = true;
        resolve({ code });
      });
      child.once("error", () => {
        closed = true;
        resolve({ code: 2 });
      });
    });

    try {
      const deadline = Date.now() + 3000;
      while (!existsSync(pauseFile) && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
      expect(existsSync(pauseFile)).toBe(true);
      if (!existsSync(pauseFile)) return;

      unlinkSync(join(root, LOCK_NAME));
      const replacement = currentOwner("replacement", process.execPath);
      writeOwnerLock(root, replacement);
      writeFileSync(releaseFile, "release\n");

      const result = await closePromise;
      expect(result.code).toBe(0);
      expect(stdout.trim()).toBe("OWNER_BUSY");
      expect(stderr).toBe("");
      expect(readOwnerLock(root)?.token).toBe(replacement.token);
    } finally {
      writeFileSync(releaseFile, "release\n");
      if (!closed) child.kill();
      await closePromise;
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
