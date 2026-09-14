import { describe, expect, test } from "bun:test";
import { spawn } from "node:child_process";
import { once } from "node:events";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as runtimeMain from "../dist/incodex-main.cjs";
import * as runtimeInstance from "../dist/incodex-instance.cjs";
import * as runtimeSafeHome from "./runtime/incodex-safe-home.cts";

function tempRoot(): string {
  return mkdtempSync(join(tmpdir(), "incodex-runtime-process-"));
}

describe("Runtime isolated helper cleanup", () => {
  test("matches only the exact inherited session root marker", () => {
    const sessionProcessIdsFromPs = (runtimeMain as any).sessionProcessIdsFromPs;
    expect(typeof sessionProcessIdsFromPs).toBe("function");
    const root = "/Users/test/My Data/.incodex/sessions/target/s-9";
    const snapshot = [
      `410 /helper INCODEX_SESSION_ROOT=${root} CODEX_HOME=/tmp/home`,
      `411 /helper INCODEX_SESSION_ROOT=${root}0 CODEX_HOME=/tmp/home`,
      `412 /helper --database=${root}/chromium/Crashpad`,
      "413 /Applications/ChatGPT.app/Contents/MacOS/ChatGPT CODEX_HOME=/Users/test/.codex",
    ].join("\n");

    expect(sessionProcessIdsFromPs(snapshot, root)).toEqual([410]);
  });

  test("quiesces helpers before the first burn attempt", async () => {
    const cleanupExitedSession = (runtimeMain as any).cleanupExitedSession;
    expect(typeof cleanupExitedSession).toBe("function");
    const events: string[] = [];
    const session = { root: "/tmp/session/s-1", sessionId: "s-1", ino: 10, dev: 20 };

    const removed = await cleanupExitedSession(session, null, {
      userRoot: "/tmp/session-home",
      quiesceSessionHelpers: async () => events.push("quiesce"),
      readBurnProof: () => null,
      assertOwnerPresentForUnsnapshottedBurn: () => {},
      burnSessionHome: () => {
        events.push("burn");
        return true;
      },
      burnSessionHomeWithOwner: () => {
        throw new Error("owner burn is not expected");
      },
      exists: () => false,
      clearBurnProof: () => {},
      wait: async () => {},
      log: () => {},
    });

    expect(removed).toBe(true);
    expect(events[0]).toBe("quiesce");
    expect(events.slice(1).every((event) => event === "burn")).toBe(true);
  });

  test("retains the session when helper quiescence cannot be proven", async () => {
    const cleanupExitedSession = (runtimeMain as any).cleanupExitedSession;
    expect(typeof cleanupExitedSession).toBe("function");
    let burned = false;

    const removed = await cleanupExitedSession(
      { root: "/tmp/session/s-2", sessionId: "s-2", ino: 30, dev: 40 },
      null,
      {
        userRoot: "/tmp/session-home",
        quiesceSessionHelpers: async () => {
          throw new Error("helpers survived");
        },
        readBurnProof: () => null,
        burnSessionHome: () => {
          burned = true;
          return true;
        },
        exists: () => true,
        clearBurnProof: () => {},
        wait: async () => {},
        log: () => {},
      },
    );

    expect(removed).toBe(false);
    expect(burned).toBe(false);
  });

  test("an exact child exit can finish a partial burn after owner removal", async () => {
    const cleanupExitedSession = (runtimeMain as any).cleanupExitedSession;
    expect(typeof cleanupExitedSession).toBe("function");
    const userRoot = join(tempRoot(), ".incodex");
    const session = runtimeSafeHome.createSessionHome(userRoot, { pid: process.pid });
    const owner = JSON.parse(readFileSync(join(session.root, "owner.json"), "utf8"));
    const ownerSnapshot = {
      pid: owner.pid,
      processStartIdentity: owner.processStartIdentity,
    };
    rmSync(join(session.root, "owner.json"));
    writeFileSync(join(session.root, "late-plugin-cache"), "late\n");
    const events: string[] = [];

    const removed = await cleanupExitedSession(session, ownerSnapshot, {
      userRoot,
      quiesceSessionHelpers: async () => events.push("quiesce"),
      wait: async () => {},
      log: () => {},
    });

    expect(events).toEqual(["quiesce"]);
    expect(removed).toBe(true);
    expect(existsSync(session.root)).toBe(false);
  });

  test("a missing owner without the parent snapshot stays retained", async () => {
    const cleanupExitedSession = (runtimeMain as any).cleanupExitedSession;
    expect(typeof cleanupExitedSession).toBe("function");
    const userRoot = join(tempRoot(), ".incodex");
    const session = runtimeSafeHome.createSessionHome(userRoot, { pid: process.pid });
    rmSync(join(session.root, "owner.json"));
    writeFileSync(join(session.root, "late-plugin-cache"), "late\n");

    const removed = await cleanupExitedSession(session, null, {
      userRoot,
      quiesceSessionHelpers: async () => {},
      wait: async () => {},
      log: () => {},
    });

    expect(removed).toBe(false);
    expect(existsSync(join(session.root, "late-plugin-cache"))).toBe(true);
  });

  test("partial-burn recovery refuses a replaced session inode", async () => {
    const cleanupExitedSession = (runtimeMain as any).cleanupExitedSession;
    expect(typeof cleanupExitedSession).toBe("function");
    const userRoot = join(tempRoot(), ".incodex");
    const session = runtimeSafeHome.createSessionHome(userRoot, { pid: process.pid });
    const owner = JSON.parse(readFileSync(join(session.root, "owner.json"), "utf8"));
    const ownerSnapshot = {
      pid: owner.pid,
      processStartIdentity: owner.processStartIdentity,
    };
    const replacement = `${session.root}-replacement`;
    mkdirSync(replacement);
    writeFileSync(join(replacement, "keep"), "keep\n");
    rmSync(session.root, { recursive: true });
    renameSync(replacement, session.root);

    const removed = await cleanupExitedSession(session, ownerSnapshot, {
      userRoot,
      quiesceSessionHelpers: async () => {},
      wait: async () => {},
      log: () => {},
    });

    expect(removed).toBe(false);
    expect(readFileSync(join(session.root, "keep"), "utf8")).toBe("keep\n");
  });

  test("partial-burn recovery still rejects a present owner mismatch", async () => {
    const cleanupExitedSession = (runtimeMain as any).cleanupExitedSession;
    expect(typeof cleanupExitedSession).toBe("function");
    const userRoot = join(tempRoot(), ".incodex");
    const session = runtimeSafeHome.createSessionHome(userRoot, { pid: process.pid });
    const ownerPath = join(session.root, "owner.json");
    const owner = JSON.parse(readFileSync(ownerPath, "utf8"));
    const ownerSnapshot = {
      pid: owner.pid,
      processStartIdentity: owner.processStartIdentity,
    };
    owner.processStartIdentity = "tampered-after-exit";
    writeFileSync(ownerPath, `${JSON.stringify(owner)}\n`);

    const removed = await cleanupExitedSession(session, ownerSnapshot, {
      userRoot,
      quiesceSessionHelpers: async () => {},
      wait: async () => {},
      log: () => {},
    });

    expect(removed).toBe(false);
    expect(existsSync(ownerPath)).toBe(true);
  });

  test.skipIf(process.platform !== "darwin")(
    "finds the inherited marker in a real macOS process snapshot",
    async () => {
      const quiesceSessionHelpers = (runtimeInstance as any).quiesceSessionHelpers;
      expect(typeof quiesceSessionHelpers).toBe("function");
      const root = `/tmp/incodex runtime marker ${process.pid}-${Date.now()}`;
      const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
        env: { ...process.env, INCODEX_SESSION_ROOT: root },
        stdio: "ignore",
      });
      await once(child, "spawn");
      await new Promise((resolve) => setTimeout(resolve, 100));

      try {
        await quiesceSessionHelpers(root);
        const exited = child.exitCode != null || child.signalCode != null
          ? true
          : await Promise.race([
            once(child, "exit").then(() => true),
            new Promise<boolean>((resolve) => setTimeout(() => resolve(false), 300)),
          ]);
        expect(exited).toBe(true);
      } finally {
        if (child.exitCode == null && child.signalCode == null) child.kill("SIGKILL");
      }
    },
  );
});
