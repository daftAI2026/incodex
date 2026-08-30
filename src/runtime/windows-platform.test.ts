import { describe, expect, test } from "bun:test";
import { EventEmitter } from "node:events";
import { join } from "node:path";

const windowsPlatform = require("../../crates/incodex-cli/assets/incodex-windows-platform.cjs");

describe("Windows Runtime lifecycle adapter", () => {
  test("does not expose an automatic official-app exit capability", () => {
    expect(windowsPlatform.listenForNormalExit).toBeUndefined();
  });

  test("writes Runtime acceptance only to the guardian pipe", () => {
    const writes: unknown[][] = [];
    const pipe = "\\\\.\\pipe\\Incodex-Runtime-Ready-0123456789abcdef0123456789abcdef";
    expect(
      windowsPlatform.markReady(pipe, (...args: unknown[]) => writes.push(args)),
    ).toBe(true);
    expect(writes).toEqual([[pipe, "accepted\n"]]);
    expect(windowsPlatform.markReady("C:\\Temp\\ready", () => {})).toBe(false);
  });

  test("writes exact main-window closure only to the guardian close pipe", () => {
    const writes: unknown[][] = [];
    const pipe = "\\\\.\\pipe\\Incodex-Runtime-Closed-0123456789abcdef0123456789abcdef";
    expect(
      windowsPlatform.markClosed(pipe, (...args: unknown[]) => writes.push(args)),
    ).toBe(true);
    expect(writes).toEqual([[pipe, "closed\n"]]);
    expect(windowsPlatform.markClosed("C:\\Temp\\closed", () => {})).toBe(false);
  });

  test("rechecks readiness after the shared UI mounts asynchronously", async () => {
    const scheduled: Array<() => void> = [];
    let inspections = 0;
    let accepted = 0;

    windowsPlatform.observeRuntimeUiReadiness(
      { isDestroyed: () => false },
      async () => ++inspections >= 2,
      () => accepted++,
      (callback: () => void, delay: number) => {
        expect(delay).toBe(250);
        scheduled.push(callback);
      },
    );

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(inspections).toBe(1);
    expect(accepted).toBe(0);
    expect(scheduled).toHaveLength(1);

    scheduled.shift()?.();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(inspections).toBe(2);
    expect(accepted).toBe(1);
    expect(scheduled).toHaveLength(0);
  });

  test("raises the existing shared window through the native owner pipe", () => {
    type ConnectionHandler = (socket: EventEmitter & { end: (value: string) => void }) => void;
    const connection: { handler?: ConnectionHandler } = {};
    let listened = "";
    let raised = 0;
    const replies: string[] = [];
    const server = {
      listen(value: string) {
        listened = value;
      },
    };
    const pipe = "\\\\.\\pipe\\Incodex-Runtime-Raise";
    const result = windowsPlatform.listenForRaise(
      pipe,
      () => raised++,
      (handler: ConnectionHandler) => {
        connection.handler = handler;
        return server;
      },
    );
    const socket = new EventEmitter() as EventEmitter & { end: (value: string) => void };
    socket.end = (value) => replies.push(value);
    connection.handler?.(socket);
    socket.emit("data", Buffer.from("raise\n"));

    expect(result).toBe(server);
    expect(listened).toBe(pipe);
    expect(raised).toBe(1);
    expect(replies).toEqual(["raised\n"]);
  });

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

  test("accepts an authenticated fast close before Runtime readiness", async () => {
    const stdout = new EventEmitter() as EventEmitter & { destroy: () => void };
    stdout.destroy = () => {};
    const child = new EventEmitter() as EventEmitter & {
      pid: number;
      stdout: EventEmitter & { destroy: () => void };
      unref: () => void;
    };
    child.pid = 45;
    child.stdout = stdout;
    child.unref = () => {};

    const result = await windowsPlatform.launchIncognito({
      helperPath: "C:\\Incodex\\incodex.exe",
      sourceHome: "C:\\Users\\me\\.codex",
      spawnProcess: () => {
        queueMicrotask(() => {
          stdout.emit("data", Buffer.from("closed\n"));
          child.emit("close", 0);
        });
        return child;
      },
    });

    expect(result).toEqual({ ok: true, reason: "closed-before-ready" });
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
          child.emit("close", 0);
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

  test("never force-kills a guardian that may still be restoring package state", async () => {
    const stdout = new EventEmitter() as EventEmitter & { destroy: () => void };
    stdout.destroy = () => {};
    let killed = false;
    const child = new EventEmitter() as EventEmitter & {
      pid: number;
      stdout: EventEmitter & { destroy: () => void };
      stdin: { end: () => void; destroy: () => void };
      kill: () => boolean;
      unref: () => void;
    };
    child.pid = 44;
    child.stdout = stdout;
    child.stdin = { end() {}, destroy() {} };
    child.kill = () => {
      killed = true;
      return true;
    };
    child.unref = () => {};

    const result = await windowsPlatform.launchIncognito({
      helperPath: "C:\\Incodex\\incodex.exe",
      sourceHome: "C:\\Users\\me\\.codex",
      readyTimeoutMs: 1,
      cancelExitTimeoutMs: 1,
      spawnProcess: () => child,
    });

    expect(result).toEqual({ ok: false, reason: "cleanup-pending" });
    expect(killed).toBe(false);
  });
});
