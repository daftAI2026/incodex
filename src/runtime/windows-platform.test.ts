import { describe, expect, test } from "bun:test";
import { EventEmitter } from "node:events";
import { join } from "node:path";

const windowsPlatform = require("../../crates/incodex-cli/assets/incodex-windows-platform.cjs");

class FakeSocket extends EventEmitter {
  constructor(private readonly onEnd: (message: string, done?: () => void) => void) {
    super();
  }

  setEncoding(_encoding: string): void {}

  end(message: string, done?: () => void): void {
    this.onEnd(message, done);
  }
}

describe("Windows Runtime lifecycle adapter", () => {
  test("accepts one authenticated normal-exit request before invoking Electron quit", () => {
    let connection: ((socket: FakeSocket) => void) | undefined;
    let listening = "";
    const events: string[] = [];
    const server = {
      listen(name: string) {
        listening = name;
      },
    };
    const socket = new FakeSocket((message, done) => {
      events.push(`response:${message}`);
      done?.();
    });

    const result = windowsPlatform.listenForNormalExit(
      "\\\\.\\pipe\\Incodex-Runtime-Control-0123456789abcdef0123456789abcdef",
      () => events.push("quit"),
      (handler: (client: FakeSocket) => void) => {
        connection = handler;
        return server;
      },
    );
    connection?.(socket);
    socket.emit("data", "quit\n");

    expect(result).toBe(server);
    expect(listening).toBe(
      "\\\\.\\pipe\\Incodex-Runtime-Control-0123456789abcdef0123456789abcdef",
    );
    expect(events).toEqual(["response:accepted\n", "quit"]);
  });

  test("rejects malformed normal-exit capabilities and commands", () => {
    expect(() =>
      windowsPlatform.listenForNormalExit("\\\\.\\pipe\\Incodex-Runtime-Control-public", () => {}),
    ).toThrow("invalid Windows Runtime control endpoint");

    let connection: ((socket: FakeSocket) => void) | undefined;
    const responses: string[] = [];
    windowsPlatform.listenForNormalExit(
      "\\\\.\\pipe\\Incodex-Runtime-Control-fedcba9876543210fedcba9876543210",
      () => {
        throw new Error("must not quit");
      },
      (handler: (client: FakeSocket) => void) => {
        connection = handler;
        return { listen() {} };
      },
    );
    const socket = new FakeSocket((message) => responses.push(message));
    connection?.(socket);
    socket.emit("data", "close\n");
    expect(responses).toEqual(["refused\n"]);
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

  test("exits only after the last primary Windows window closes", () => {
    let closed: (() => void) | undefined;
    let anotherPrimaryWindow = true;
    const exits: number[] = [];
    const win = {
      on(event: string, callback: () => void) {
        if (event === "closed") closed = callback;
      },
    };

    windowsPlatform.exitAfterLastMainWindowCloses(
      win,
      () => anotherPrimaryWindow,
      (code: number) => exits.push(code),
    );

    closed?.();
    expect(exits).toEqual([]);
    anotherPrimaryWindow = false;
    closed?.();
    expect(exits).toEqual([0]);
  });

  test("exits when the official Windows host hides its last primary window", () => {
    let close: (() => void) | undefined;
    let visible = true;
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];
    const win = {
      isDestroyed: () => false,
      isVisible: () => visible,
      on(event: string, callback: () => void) {
        if (event === "close") close = callback;
      },
    };

    windowsPlatform.exitAfterLastMainWindowCloses(
      win,
      () => false,
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );

    close?.();
    visible = false;
    scheduled.shift()?.();
    expect(exits).toEqual([0]);
  });

  test("keeps observing a closing Windows host until it actually becomes hidden", () => {
    let close: (() => void) | undefined;
    let visible = true;
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];
    const win = {
      isDestroyed: () => false,
      isVisible: () => visible,
      on(event: string, callback: () => void) {
        if (event === "close") close = callback;
      },
    };

    windowsPlatform.exitAfterLastMainWindowCloses(
      win,
      () => false,
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );

    close?.();
    scheduled.shift()?.();
    expect(exits).toEqual([]);
    expect(scheduled).toHaveLength(1);
    visible = false;
    scheduled.shift()?.();
    expect(exits).toEqual([0]);
  });

  test("does not abandon a host that hides after the old polling deadline", () => {
    let close: (() => void) | undefined;
    let visible = true;
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];
    const win = {
      isDestroyed: () => false,
      isVisible: () => visible,
      on(event: string, callback: () => void) {
        if (event === "close") close = callback;
      },
    };

    windowsPlatform.exitAfterLastMainWindowCloses(
      win,
      () => false,
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );

    close?.();
    for (let attempt = 0; attempt < 50; attempt++) {
      expect(scheduled).toHaveLength(1);
      scheduled.shift()?.();
    }
    expect(exits).toEqual([]);
    expect(scheduled).toHaveLength(1);
    visible = false;
    scheduled.shift()?.();
    expect(exits).toEqual([0]);
  });

  test("keeps observing while another primary Windows window is still visible", () => {
    let close: (() => void) | undefined;
    let visible = true;
    let anotherPrimaryWindow = true;
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];
    const win = {
      isDestroyed: () => false,
      isVisible: () => visible,
      on(event: string, callback: () => void) {
        if (event === "close") close = callback;
      },
    };

    windowsPlatform.exitAfterLastMainWindowCloses(
      win,
      () => anotherPrimaryWindow,
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );

    close?.();
    scheduled.shift()?.();
    expect(exits).toEqual([]);
    expect(scheduled).toHaveLength(1);
    anotherPrimaryWindow = false;
    visible = false;
    scheduled.shift()?.();
    expect(exits).toEqual([0]);
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
