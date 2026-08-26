import { describe, expect, test } from "bun:test";
import { EventEmitter } from "node:events";
import { join } from "node:path";

const windowsPlatform = require("../../crates/incodex-cli/assets/incodex-windows-platform.cjs");

describe("Windows Runtime lifecycle adapter", () => {
  test("delegates only native lifecycle data to the installed helper", async () => {
    const helperPath = "C:\\Users\\me\\.incodex\\windows\\helpers\\abc\\incodex-helper.exe";
    const sourceHome = "C:\\Users\\me\\.codex";
    const calls: unknown[][] = [];
    const stdout = new EventEmitter() as EventEmitter & { destroy: () => void };
    stdout.destroy = () => {};
    const child = new EventEmitter() as EventEmitter & {
      pid: number;
      stdout: EventEmitter & { destroy: () => void };
      unref: () => void;
    };
    child.pid = 42;
    child.stdout = stdout;
    child.unref = () => {};
    const result = await windowsPlatform.launchIncognito({
      helperPath,
      sourceHome,
      sourceBounds: "10,20,1200,800",
      spawnProcess(...args: unknown[]) {
        calls.push(args);
        queueMicrotask(() => stdout.emit("data", Buffer.from("ready\n")));
        return child;
      },
    });

    expect(result).toEqual({ ok: true });
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[0]).toBe(helperPath);
    expect(calls[0]?.[1]).toEqual([
      "__incodex_windows_runtime_open",
      "--source-home",
      sourceHome,
      "--source-bounds",
      "10,20,1200,800",
    ]);
    expect(calls[0]?.[2]).toMatchObject({
      windowsHide: true,
      stdio: ["pipe", "pipe", "ignore"],
    });
  });

  test("fails closed before spawning on relative or malformed lifecycle input", async () => {
    let spawned = false;
    const spawnProcess = () => {
      spawned = true;
      throw new Error("must not spawn");
    };

    expect(
      await windowsPlatform.launchIncognito({
        helperPath: join("relative", "incodex.exe"),
        sourceHome: "C:\\Users\\me\\.codex",
        spawnProcess,
      }),
    ).toEqual({ ok: false, reason: "invalid-helper" });
    expect(
      await windowsPlatform.launchIncognito({
        helperPath: "C:\\Incodex\\incodex.exe",
        sourceHome: "C:\\Users\\me\\.codex",
        sourceBounds: "10,20,1200,800;calc",
        spawnProcess,
      }),
    ).toEqual({ ok: false, reason: "invalid-source-bounds" });
    expect(spawned).toBe(false);
  });

  test("cancels the guardian and waits for its exit before reporting a ready timeout", async () => {
    const stdout = new EventEmitter() as EventEmitter & { destroy: () => void };
    stdout.destroy = () => {};
    const writes: string[] = [];
    let exited = false;
    const child = new EventEmitter() as EventEmitter & {
      pid: number;
      stdout: EventEmitter & { destroy: () => void };
      stdin: { end: (value: string) => void; destroy: () => void };
      kill: () => boolean;
      unref: () => void;
    };
    child.pid = 43;
    child.stdout = stdout;
    child.stdin = {
      end(value) {
        writes.push(value);
        queueMicrotask(() => {
          exited = true;
          child.emit("exit", 0);
        });
      },
      destroy() {},
    };
    child.kill = () => true;
    child.unref = () => {};

    const result = await windowsPlatform.launchIncognito({
      helperPath: "C:\\Incodex\\incodex.exe",
      sourceHome: "C:\\Users\\me\\.codex",
      readyTimeoutMs: 1,
      spawnProcess: () => child,
    });

    expect(result).toEqual({ ok: false, reason: "ready-timeout" });
    expect(writes).toEqual(["cancel\n"]);
    expect(exited).toBe(true);
  });
});
