import { describe, expect, test } from "bun:test";
import { EventEmitter } from "node:events";
import { join } from "node:path";

const windowsPlatform = require("../../crates/incodex-cli/assets/incodex-windows-platform.cjs");

describe("Windows Runtime lifecycle adapter", () => {
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

  test("raises the existing shared window through the native owner pipe", () => {
    let connectionHandler: ((socket: EventEmitter & { end: (value: string) => void }) => void) | null =
      null;
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
      (handler: typeof connectionHandler) => {
        connectionHandler = handler;
        return server;
      },
    );
    const socket = new EventEmitter() as EventEmitter & { end: (value: string) => void };
    socket.end = (value) => replies.push(value);
    connectionHandler?.(socket);
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
