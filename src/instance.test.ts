import { describe, expect, test } from "bun:test";
import { mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
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
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    const worker = String.raw`
      const { existsSync } = require("node:fs");
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const root = process.env.INCODEX_TEST_ROOT;
      const barrier = process.env.INCODEX_TEST_BARRIER;
      while (!existsSync(barrier)) {
        const until = Date.now() + 5;
        while (Date.now() < until) {}
      }
      const owner = instance.currentOwner("worker", process.execPath);
      try {
        instance.acquireOwnerLease(root, owner);
        process.stdout.write("WINNER\n");
        setTimeout(() => process.exit(0), 1500);
      } catch (error) {
        process.stdout.write(String(error.code || "ERROR") + "\n");
        process.exit(error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 2 : 3);
      }
    `;
    const children = Array.from({ length: 20 }, () =>
      spawn(process.execPath, ["-e", worker], {
        env: {
          ...process.env,
          INCODEX_TEST_MODULE: modulePath,
          INCODEX_TEST_ROOT: root,
          INCODEX_TEST_BARRIER: barrier,
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
    writeFileSync(barrier, "go\n");
    const completed = await Promise.all(results);
    const winners = completed.filter((result) => result.stdout.trim() === "WINNER");
    const rejected = completed.filter((result) => ["OWNER_BUSY", "OWNER_RACE"].includes(result.stdout.trim()));

    expect(winners).toHaveLength(1);
    expect(rejected).toHaveLength(19);
    expect(completed.every((result) => result.code === 0 || result.code === 2)).toBe(true);
    expect(completed.every((result) => result.stderr === "")).toBe(true);
  });

  test("twenty OS processes replacing a stale owner preserve the winner lease", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-os-stale-contenders-"));
    const barrier = join(root, "start");
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    writeFileSync(join(root, LOCK_NAME), JSON.stringify({
      pid: 999999,
      startedAt: "never",
      execPath: "/nope",
      sessionId: "stale",
      token: "stale-token",
    }));
    const worker = String.raw`
      const { existsSync } = require("node:fs");
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const root = process.env.INCODEX_TEST_ROOT;
      const barrier = process.env.INCODEX_TEST_BARRIER;
      while (!existsSync(barrier)) {
        const until = Date.now() + 5;
        while (Date.now() < until) {}
      }
      const owner = instance.currentOwner("stale-replacement", process.execPath);
      try {
        instance.acquireOwnerLease(root, owner);
        process.stdout.write("WINNER " + owner.token + "\n");
        setTimeout(() => process.exit(0), 1500);
      } catch (error) {
        process.stdout.write(String(error.code || "ERROR") + "\n");
        process.exit(error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 2 : 3);
      }
    `;
    const children = Array.from({ length: 20 }, () =>
      spawn(process.execPath, ["-e", worker], {
        env: {
          ...process.env,
          INCODEX_TEST_MODULE: modulePath,
          INCODEX_TEST_ROOT: root,
          INCODEX_TEST_BARRIER: barrier,
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
    writeFileSync(barrier, "go\n");
    const completed = await Promise.all(results);
    const winners = completed.filter((result) => result.stdout.trim().startsWith("WINNER "));
    const rejected = completed.filter((result) => ["OWNER_BUSY", "OWNER_RACE"].includes(result.stdout.trim()));
    const winnerToken = winners[0]?.stdout.trim().slice("WINNER ".length);

    expect(winners).toHaveLength(1);
    expect(rejected).toHaveLength(19);
    expect(winnerToken).toBeString();
    expect(readOwnerLock(root)?.token).toBe(winnerToken);
    expect(completed.every((result) => result.code === 0 || result.code === 2)).toBe(true);
    expect(completed.every((result) => result.stderr === "")).toBe(true);
  });

  test("twenty OS processes recover one truncated lock without deleting the winner", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-os-invalid-contenders-"));
    const barrier = join(root, "start");
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    const truncated = "{\"pid\":";
    writeFileSync(join(root, LOCK_NAME), truncated);
    const worker = String.raw`
      const { existsSync } = require("node:fs");
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const root = process.env.INCODEX_TEST_ROOT;
      const barrier = process.env.INCODEX_TEST_BARRIER;
      while (!existsSync(barrier)) {
        const until = Date.now() + 5;
        while (Date.now() < until) {}
      }
      const owner = instance.currentOwner("invalid-recovery", process.execPath);
      try {
        instance.acquireOwnerLease(root, owner);
        process.stdout.write("WINNER " + owner.token + "\n");
        setTimeout(() => process.exit(0), 1500);
      } catch (error) {
        process.stdout.write(String(error.code || "ERROR") + "\n");
        process.exit(error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 2 : 3);
      }
    `;
    const children = Array.from({ length: 20 }, () =>
      spawn(process.execPath, ["-e", worker], {
        env: {
          ...process.env,
          INCODEX_TEST_MODULE: modulePath,
          INCODEX_TEST_ROOT: root,
          INCODEX_TEST_BARRIER: barrier,
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
    writeFileSync(barrier, "go\n");
    const completed = await Promise.all(results);
    const winners = completed.filter((result) => result.stdout.trim().startsWith("WINNER "));
    const rejected = completed.filter((result) => ["OWNER_BUSY", "OWNER_RACE"].includes(result.stdout.trim()));
    const winnerToken = winners[0]?.stdout.trim().slice("WINNER ".length);
    const quarantine = readdirSync(root).filter((name) => name.startsWith(`.${LOCK_NAME}.invalid.`));

    expect(winners).toHaveLength(1);
    expect(rejected).toHaveLength(19);
    expect(winnerToken).toBeString();
    expect(quarantine.length).toBeGreaterThan(0);
    expect(readOwnerLock(root)?.token).toBe(winnerToken);
    expect(completed.every((result) => result.code === 0 || result.code === 2)).toBe(true);
    expect(completed.every((result) => result.stderr === "")).toBe(true);
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
