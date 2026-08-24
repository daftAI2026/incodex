import { describe, expect, test } from "bun:test";
import { spawn } from "node:child_process";
import { once } from "node:events";
import * as runtimeMain from "../dist/incodex-main.cjs";
import * as runtimeInstance from "../dist/incodex-instance.cjs";

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
