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
import { spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  currentOwner,
  LOCK_NAME,
  readOwnerLock,
  writeOwnerLock,
} from "./runtime/incodex-instance.cts";

describe("cross-process owner recovery", () => {
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
    const resultRoot = join(root, "result");
    const releaseFile = join(root, "release");
    mkdirSync(readyRoot);
    mkdirSync(doneRoot);
    mkdirSync(resultRoot);
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
      const resultRoot = process.env.INCODEX_TEST_RESULT_ROOT;
      const releaseFile = process.env.INCODEX_TEST_RELEASE_FILE;
      const index = process.env.INCODEX_TEST_INDEX;
      writeFileSync(join(readyRoot, index), "ready\n");
      while (!existsSync(barrier)) {
        const until = Date.now() + 5;
        while (Date.now() < until) {}
      }
      const owner = instance.currentOwner("invalid-recovery", process.execPath);
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
    const quarantine = readdirSync(root).filter((name) => name.startsWith(`.${LOCK_NAME}.invalid.`));

    expect(winners).toHaveLength(1);
    expect(rejected).toHaveLength(19);
    expect(allReady).toBe(true);
    expect(allDone).toBe(true);
    expect(allResults).toBe(true);
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
