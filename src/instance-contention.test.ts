import { describe, expect, test } from "bun:test";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from "node:fs";
import { spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  acquireOwnerLease,
  clearOwnerLock,
  currentOwner,
  LOCK_NAME,
  readOwnerLock,
  writeOwnerLock,
} from "./runtime/incodex-instance.cts";

describe("cross-process owner contention", () => {
  test("twenty OS processes produce one winner and never steal its lease", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-os-contenders-"));
    const barrier = join(root, "start");
    const readyRoot = join(root, "ready");
    const doneRoot = join(root, "done");
    const resultRoot = join(root, "result");
    const releaseFile = join(root, "release");
    mkdirSync(readyRoot);
    mkdirSync(doneRoot);
    mkdirSync(resultRoot);
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    const worker = String.raw`
      const { existsSync, writeFileSync } = require("node:fs");
      const { join } = require("node:path");
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const root = process.env.INCODEX_TEST_ROOT;
      const barrier = process.env.INCODEX_TEST_BARRIER;
      const readyRoot = process.env.INCODEX_TEST_READY_ROOT;
      const doneRoot = process.env.INCODEX_TEST_DONE_ROOT;
      const resultRoot = process.env.INCODEX_TEST_RESULT_ROOT;
      const releaseFile = process.env.INCODEX_TEST_RELEASE_FILE;
      const index = process.env.INCODEX_TEST_INDEX;
      writeFileSync(join(readyRoot, index), "ready\n");
      while (!existsSync(barrier)) {
        const until = Date.now() + 5;
        while (Date.now() < until) {}
      }
      const owner = instance.currentOwner("worker", process.execPath);
      let exitCode = 0;
      let outcome = "";
      try {
        instance.acquireOwnerLease(root, owner);
        outcome = "WINNER";
      } catch (error) {
        outcome = String(error.code || "ERROR");
        exitCode = error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 2 : 3;
      }
      writeFileSync(join(resultRoot, index), outcome + "\n");
      process.stdout.write(outcome + "\n");
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
          INCODEX_TEST_RESULT_ROOT: resultRoot,
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
    const allResults = await waitForCount(resultRoot, children.length);
    writeFileSync(releaseFile, "release\n");
    const completed = await Promise.all(results);
    const outcomes = readdirSync(resultRoot).map((index) => readFileSync(join(resultRoot, index), "utf8").trim());
    const winners = outcomes.filter((result) => result === "WINNER");
    const rejected = outcomes.filter((result) => ["OWNER_BUSY", "OWNER_RACE"].includes(result));

    expect(winners).toHaveLength(1);
    expect(rejected).toHaveLength(19);
    expect(allReady).toBe(true);
    expect(allDone).toBe(true);
    expect(allResults).toBe(true);
    expect(completed.every((result) => result.code === 0 || result.code === 2)).toBe(true);
    expect(completed.every((result) => result.stderr === "")).toBe(true);
  }, 30_000);

  test("twenty OS processes replacing a stale owner preserve the winner lease", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-os-stale-contenders-"));
    const barrier = join(root, "start");
    const readyRoot = join(root, "ready");
    const doneRoot = join(root, "done");
    const resultRoot = join(root, "result");
    const releaseFile = join(root, "release");
    mkdirSync(readyRoot);
    mkdirSync(doneRoot);
    mkdirSync(resultRoot);
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
      const resultRoot = process.env.INCODEX_TEST_RESULT_ROOT;
      const releaseFile = process.env.INCODEX_TEST_RELEASE_FILE;
      const index = process.env.INCODEX_TEST_INDEX;
      writeFileSync(join(readyRoot, index), "ready\n");
      while (!existsSync(barrier)) {
        const until = Date.now() + 5;
        while (Date.now() < until) {}
      }
      const owner = instance.currentOwner("stale-replacement", process.execPath);
      let exitCode = 0;
      let outcome = "";
      try {
        instance.acquireOwnerLease(root, owner);
        outcome = "WINNER " + owner.token;
      } catch (error) {
        outcome = String(error.code || "ERROR");
        exitCode = error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 2 : 3;
      }
      writeFileSync(join(resultRoot, index), outcome + "\n");
      process.stdout.write(outcome + "\n");
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
          INCODEX_TEST_RESULT_ROOT: resultRoot,
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
    const allResults = await waitForCount(resultRoot, children.length);
    writeFileSync(releaseFile, "release\n");
    const completed = await Promise.all(results);
    const outcomes = readdirSync(resultRoot).map((index) => readFileSync(join(resultRoot, index), "utf8").trim());
    const winners = outcomes.filter((result) => result.startsWith("WINNER "));
    const rejected = outcomes.filter((result) => ["OWNER_BUSY", "OWNER_RACE"].includes(result));
    const winnerToken = winners[0]?.slice("WINNER ".length);

    expect(winners).toHaveLength(1);
    expect(rejected).toHaveLength(19);
    expect(allReady).toBe(true);
    expect(allDone).toBe(true);
    expect(allResults).toBe(true);
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
    const resultRoot = join(root, "result");
    const releaseFile = join(root, "release");
    mkdirSync(readyRoot);
    mkdirSync(doneRoot);
    mkdirSync(resultRoot);
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
      const resultRoot = process.env.INCODEX_TEST_RESULT_ROOT;
      const releaseFile = process.env.INCODEX_TEST_RELEASE_FILE;
      const index = process.env.INCODEX_TEST_INDEX;
      writeFileSync(join(readyRoot, index), "ready\n");
      while (!existsSync(barrier)) {
        const until = Date.now() + 5;
        while (Date.now() < until) {}
      }
      const owner = instance.currentOwner("stale-claim-replacement", process.execPath);
      let exitCode = 0;
      let outcome = "";
      try {
        instance.acquireOwnerLease(root, owner);
        outcome = "WINNER " + owner.token;
      } catch (error) {
        outcome = String(error.code || "ERROR");
        exitCode = error.code === "OWNER_BUSY" || error.code === "OWNER_RACE" ? 2 : 3;
      }
      writeFileSync(join(resultRoot, index), outcome + "\n");
      process.stdout.write(outcome + "\n");
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
          INCODEX_TEST_RESULT_ROOT: resultRoot,
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
    const allResults = await waitForCount(resultRoot, children.length);
    writeFileSync(releaseFile, "release\n");
    const completed = await Promise.all(results);
    const outcomes = readdirSync(resultRoot).map((index) => readFileSync(join(resultRoot, index), "utf8").trim());
    const winners = outcomes.filter((result) => result.startsWith("WINNER "));
    const rejected = outcomes.filter((result) => ["OWNER_BUSY", "OWNER_RACE"].includes(result));
    const winnerToken = winners[0]?.slice("WINNER ".length);

    expect(winners).toHaveLength(1);
    expect(rejected).toHaveLength(7);
    expect(allReady).toBe(true);
    expect(allDone).toBe(true);
    expect(allResults).toBe(true);
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

  test("a foreign claim blocks the no-owner fast path without publishing a lease", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-foreign-fast-path-"));
    const claimPath = join(root, ".incognito.lock.takeover");
    const foreignRecord = JSON.stringify({ pid: 999999, token: "foreign-fast-path" });
    writeFileSync(claimPath, foreignRecord);

    let caught: unknown;
    try {
      acquireOwnerLease(root, currentOwner("foreign-fast-path", process.execPath));
    } catch (error) {
      caught = error;
    }

    expect((caught as { code?: string })?.code).toBe("OWNER_FOREIGN_CLAIM");
    expect(existsSync(join(root, LOCK_NAME))).toBe(false);
    expect(readFileSync(claimPath, "utf8")).toBe(foreignRecord);
  });

  test("owner cleanup treats a foreign claim as non-releasable without throwing", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-foreign-cleanup-"));
    const claimPath = join(root, ".incognito.lock.takeover");
    const foreignRecord = JSON.stringify({ pid: 999999, token: "foreign-cleanup" });
    const owner = currentOwner("foreign-cleanup", process.execPath);
    writeOwnerLock(root, owner);
    writeFileSync(claimPath, foreignRecord);

    expect(() => clearOwnerLock(root, owner)).not.toThrow();
    expect(clearOwnerLock(root, owner)).toBe(false);
    expect(readOwnerLock(root)?.token).toBe(owner.token);
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
});
