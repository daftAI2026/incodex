import { describe, expect, test } from "bun:test";
import { createConnection, createServer } from "node:net";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  SOCK_NAME,
  acquireOwnerLease,
  clearOwnerLock,
  connectExisting,
  currentOwner,
  ownerPortFromExec,
  readOwnerLock,
  releaseOwnerLease,
  writeOwnerLock,
} from "./runtime/incodex-instance.cts";

const target = (root: string) => join(root, "target-executable");

const waitForCount = async (directory: string, expected: number, timeoutMs = 30_000) => {
  const deadline = Date.now() + timeoutMs;
  while (readdirSync(directory).length < expected && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  return readdirSync(directory).length === expected;
};

describe("kernel-held TCP owner lease", () => {
  test("includes the macOS uid in the fixed port derivation", () => {
    const execPath = "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT";
    expect(ownerPortFromExec(execPath, 501)).not.toBe(ownerPortFromExec(execPath, 502));
    expect(ownerPortFromExec(execPath, 501)).toBe(ownerPortFromExec(execPath, 501));
  });

  test("does not overwrite a live legacy owner record before publication", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tcp-legacy-record-"));
    const legacy = currentOwner("legacy-runtime", process.execPath);
    delete (legacy as Record<string, unknown>).execIdentity;
    writeOwnerLock(root, legacy);
    const candidate = currentOwner("new-runtime", process.execPath);
    await expect(acquireOwnerLease(root, candidate)).rejects.toMatchObject({ code: "OWNER_LEGACY_OWNER" });
    expect(readOwnerLock(root)?.token).toBe(legacy.token);
  });

  test("probe returns only a non-secret marker while token raise still works", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tcp-probe-"));
    const owner = currentOwner("probe", target(root));
    await acquireOwnerLease(root, owner);
    const response = await new Promise<string>((resolve, reject) => {
      const socket = createConnection({ host: "127.0.0.1", port: ownerPortFromExec(owner.execPath) });
      let output = "";
      socket.setEncoding("utf8");
      socket.once("error", reject);
      socket.on("data", (chunk) => {
        output += chunk;
        if (output.includes("\n")) {
          socket.destroy();
          resolve(output.trim());
        }
      });
      socket.once("connect", () => socket.write("probe\n"));
    });
    expect(response).toBe("owner-ready");
    expect(response).not.toContain(owner.token);
    expect(await connectExisting(root, 500, owner.token)).toBe(true);
    clearOwnerLock(root, owner);
  });

  test("deletes only its diagnostic record before asynchronously releasing the port", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tcp-release-"));
    const owner = currentOwner("release", target(root));
    await acquireOwnerLease(root, owner);
    const wrongOwner = { ...owner, token: "wrong-token", nonce: "wrong-token" };
    expect(await releaseOwnerLease(root, wrongOwner)).toBe(false);
    expect(await connectExisting(root, 500, owner.token)).toBe(true);
    expect(await releaseOwnerLease(root, owner)).toBe(true);
    expect(readOwnerLock(root)).toBeNull();
    const replacement = currentOwner("reopen", target(root));
    await expect(acquireOwnerLease(root, replacement)).resolves.toEqual(replacement);
    expect(await releaseOwnerLease(root, replacement)).toBe(true);
  });

  test("bounds an unterminated protocol request", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tcp-protocol-limit-"));
    const owner = currentOwner("protocol", target(root));
    await acquireOwnerLease(root, owner);
    const closed = new Promise<boolean>((resolve) => {
      const socket = createConnection({ host: "127.0.0.1", port: ownerPortFromExec(owner.execPath) });
      socket.once("close", () => resolve(true));
      socket.once("connect", () => socket.write("x".repeat(300)));
      socket.once("error", () => resolve(true));
    });
    expect(await Promise.race([closed, new Promise<boolean>((resolve) => setTimeout(() => resolve(false), 2_000))])).toBe(true);
    await releaseOwnerLease(root, owner);
  });

  test("twenty OS contenders have one listener owner and keep it until completion", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tcp-contenders-"));
    const readyRoot = join(root, "ready");
    const doneRoot = join(root, "done");
    const resultRoot = join(root, "result");
    const releaseFile = join(root, "release");
    const targetExec = join(root, "target-executable");
    mkdirSync(readyRoot);
    mkdirSync(doneRoot);
    mkdirSync(resultRoot);
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    const worker = String.raw`
      (async () => {
      const { existsSync, writeFileSync } = require("node:fs");
      const { randomBytes } = require("node:crypto");
      const { join } = require("node:path");
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const root = process.env.INCODEX_TEST_ROOT;
      const index = process.env.INCODEX_TEST_INDEX;
      const releaseFile = process.env.INCODEX_TEST_RELEASE_FILE;
      writeFileSync(join(process.env.INCODEX_TEST_READY_ROOT, index), "ready\n");
      while (!existsSync(join(root, "start"))) await new Promise((resolve) => setTimeout(resolve, 5));
      const token = randomBytes(16).toString("hex");
      const owner = {
        pid: process.pid,
        startedAt: "fixture",
        processStartIdentity: "fixture",
        execPath: process.env.INCODEX_TEST_TARGET_EXEC,
        execIdentity: "target-executable",
        sessionId: "tcp-contender-" + index,
        token,
        nonce: token,
      };
      let exitCode = 0;
      let outcome = "";
      try {
        await instance.acquireOwnerLease(root, owner);
        outcome = "WINNER " + owner.token;
      } catch (error) {
        outcome = String(error.code || "ERROR");
        exitCode = error.code === "OWNER_BUSY" ? 2 : 3;
      }
      writeFileSync(join(process.env.INCODEX_TEST_RESULT_ROOT, index), outcome + "\n");
      writeFileSync(join(process.env.INCODEX_TEST_DONE_ROOT, index), "done\n");
      while (!existsSync(releaseFile)) await new Promise((resolve) => setTimeout(resolve, 5));
      if (outcome.startsWith("WINNER ")) instance.clearOwnerLock(root, owner);
      process.exit(exitCode);
      })().catch((error) => {
        const { writeFileSync } = require("node:fs");
        const { join } = require("node:path");
        const index = process.env.INCODEX_TEST_INDEX;
        const message = "ERROR " + String(error?.stack || error);
        writeFileSync(join(process.env.INCODEX_TEST_RESULT_ROOT, index), message + "\n");
        writeFileSync(join(process.env.INCODEX_TEST_DONE_ROOT, index), "done\n");
        console.error(error);
        process.exit(3);
      });
    `;
    const children = Array.from({ length: 20 }, (_, index) =>
      spawn(process.execPath, ["-e", worker], {
        env: {
          ...process.env,
          INCODEX_TEST_MODULE: modulePath,
          INCODEX_TEST_ROOT: root,
          INCODEX_TEST_READY_ROOT: readyRoot,
          INCODEX_TEST_DONE_ROOT: doneRoot,
          INCODEX_TEST_RESULT_ROOT: resultRoot,
          INCODEX_TEST_RELEASE_FILE: releaseFile,
          INCODEX_TEST_TARGET_EXEC: targetExec,
          INCODEX_TEST_INDEX: String(index),
        },
        stdio: ["ignore", "ignore", "pipe"],
      }),
    );
    const completions = children.map(
      (child) =>
        new Promise<{ code: number | null; stderr: string }>((resolve) => {
          let stderr = "";
          child.stderr?.on("data", (chunk) => (stderr += String(chunk)));
          child.once("close", (code) => resolve({ code, stderr }));
          child.once("error", (error) => resolve({ code: 3, stderr: String(error) }));
        }),
    );
    let outcomes: string[] = [];
    try {
      expect(await waitForCount(readyRoot, children.length)).toBe(true);
      writeFileSync(join(root, "start"), "go\n");
      expect(await waitForCount(doneRoot, children.length)).toBe(true);
      expect(await waitForCount(resultRoot, children.length)).toBe(true);
      outcomes = readdirSync(resultRoot).map((index) => readFileSync(join(resultRoot, index), "utf8").trim());
    } finally {
      writeFileSync(releaseFile, "release\n");
    }
    const finished = await Promise.all(completions);
    expect(outcomes.filter((value) => value.startsWith("WINNER "))).toHaveLength(1);
    expect(outcomes.filter((value) => value === "OWNER_BUSY")).toHaveLength(19);
    expect(finished.every(({ code, stderr }) => (code === 0 || code === 2) && stderr === "")).toBe(true);
  }, 60_000);

  test("a SIGKILL releases the listener so the next process can acquire", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tcp-sigkill-"));
    const ready = join(root, "ready");
    const targetExec = join(root, "target-executable");
    const modulePath = join(import.meta.dir, "runtime/incodex-instance.cts");
    const worker = `
      (async () => {
      const { writeFileSync } = require("node:fs");
      const instance = require(process.env.INCODEX_TEST_MODULE);
      const owner = instance.currentOwner("sigkill", process.env.INCODEX_TEST_TARGET_EXEC);
      await instance.acquireOwnerLease(process.env.INCODEX_TEST_ROOT, owner);
      writeFileSync(process.env.INCODEX_TEST_READY, owner.token);
      await new Promise(() => {});
      })().catch((error) => { console.error(error); process.exit(3); });
    `;
    const child = spawn(process.execPath, ["-e", worker], {
      env: { ...process.env, INCODEX_TEST_MODULE: modulePath, INCODEX_TEST_ROOT: root, INCODEX_TEST_READY: ready, INCODEX_TEST_TARGET_EXEC: targetExec },
      stdio: "ignore",
    });
    const deadline = Date.now() + 10_000;
    while (!existsSync(ready) && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 10));
    expect(existsSync(ready)).toBe(true);
    child.kill("SIGKILL");
    await new Promise((resolve) => child.once("close", resolve));
    const replacement = currentOwner("after-sigkill", targetExec);
    await expect(acquireOwnerLease(root, replacement)).resolves.toEqual(replacement);
    clearOwnerLock(root, replacement);
  }, 30_000);

  test("a foreign listener fails closed without trying another port", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tcp-foreign-"));
    const targetExec = join(root, "target-executable");
    const port = ownerPortFromExec(targetExec);
    const foreign = createServer((socket) => socket.end("not-incodex\n"));
    await new Promise<void>((resolve) => foreign.listen(port, "127.0.0.1", resolve));
    try {
      await expect(acquireOwnerLease(root, currentOwner("foreign-port", targetExec))).rejects.toMatchObject({ code: "OWNER_FOREIGN_PORT" });
    } finally {
      foreign.close();
    }
  });

  test("a delayed listener handshake fails closed without a fallback port", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tcp-delayed-"));
    const targetExec = join(root, "target-executable");
    const foreign = createServer(() => {});
    await new Promise<void>((resolve) => foreign.listen(ownerPortFromExec(targetExec), "127.0.0.1", resolve));
    try {
      await expect(acquireOwnerLease(root, currentOwner("delayed-port", targetExec))).rejects.toMatchObject({ code: "OWNER_PORT_UNAVAILABLE" });
    } finally {
      foreign.close();
    }
  });

  test("an old Unix socket is foreign and is never removed", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tcp-legacy-"));
    const targetExec = join(root, "target-executable");
    const socketPath = join(root, SOCK_NAME);
    const legacy = createServer(() => {});
    await new Promise<void>((resolve) => legacy.listen(socketPath, resolve));
    try {
      await expect(acquireOwnerLease(root, currentOwner("legacy-socket", targetExec))).rejects.toMatchObject({ code: "OWNER_LEGACY_SOCKET" });
      expect(existsSync(socketPath)).toBe(true);
    } finally {
      legacy.close();
    }
  });

  test("any legacy takeover residue is foreign and is never removed", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tcp-legacy-claim-"));
    const targetExec = join(root, "target-executable");
    const claim = join(root, ".incognito.lock.takeover");
    mkdirSync(claim);
    await expect(acquireOwnerLease(root, currentOwner("legacy-claim", targetExec))).rejects.toMatchObject({ code: "OWNER_FOREIGN_CLAIM" });
    expect(existsSync(claim)).toBe(true);
  });

  test("old metadata cleanup cannot break a replacement listener", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-tcp-replacement-"));
    const targetExec = join(root, "target-executable");
    const first = currentOwner("first", targetExec);
    await acquireOwnerLease(root, first);
    clearOwnerLock(root, first);
    const replacement = currentOwner("replacement", targetExec);
    await acquireOwnerLease(root, replacement);
    expect(await connectExisting(root, 500, replacement.token)).toBe(true);
    expect(clearOwnerLock(root, first)).toBe(false);
    expect(await connectExisting(root, 500, replacement.token)).toBe(true);
    clearOwnerLock(root, replacement);
  });
});
