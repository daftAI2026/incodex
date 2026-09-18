import { describe, expect, test } from "bun:test";
import { PassThrough, Readable } from "node:stream";

import {
  APP_PATH,
  createPermissionHost,
  encodeHostMessage,
  makeAppKitRuntime,
  parseHostArgs,
  startAppKitEventPump,
} from "./incodex-permission-host.cts";

const NONCE = "0123456789abcdef0123456789abcdef";

type GuideChoice = "repair" | "later";
type GuideState = "repairing" | "awaiting-user" | "granted" | "error";

type FakeGuide = {
  choice: Promise<GuideChoice>;
  closeCalls: number;
  states: Array<{ state: GuideState; message?: string }>;
  retryHandlers: Array<() => void>;
  closeHandlers: Array<() => void>;
  onRetry: (callback: () => void) => () => void;
  onClose: (callback: () => void) => () => void;
  setState: (state: GuideState, message?: string) => void;
  emitRetry: () => void;
  close: () => void;
  resolveChoice: (choice: GuideChoice) => void;
};

function fakeGuide(): FakeGuide {
  let resolveChoice!: (choice: GuideChoice) => void;
  const guide: FakeGuide = {
    choice: new Promise<GuideChoice>((resolve) => { resolveChoice = resolve; }),
    closeCalls: 0,
    states: [],
    retryHandlers: [],
    closeHandlers: [],
    setState: (state, message) => { guide.states.push({ state, ...(message ? { message } : {}) }); },
    onRetry: (callback) => { guide.retryHandlers.push(callback); return () => {}; },
    onClose: (callback) => { guide.closeHandlers.push(callback); return () => {}; },
    emitRetry: () => { for (const callback of guide.retryHandlers) callback(); },
    close: () => {
      guide.closeCalls += 1;
      for (const callback of guide.closeHandlers) callback();
    },
    resolveChoice,
  };
  return guide;
}

function input(lines: unknown[]): Readable {
  return Readable.from(lines.map((line) => `${typeof line === "string" ? line : JSON.stringify(line)}\n`));
}

function output(): { lines: string[]; write: (chunk: string) => boolean } {
  const lines: string[] = [];
  return { lines, write: (chunk: string) => { lines.push(...chunk.trimEnd().split("\n")); return true; } };
}

async function waitForReady(stdout: { lines: string[] }): Promise<void> {
  const ready = JSON.stringify({ nonce: NONCE, type: "ready" });
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (stdout.lines.includes(ready)) return;
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  throw new Error("native permission host did not become ready");
}

function makeHost(options: {
  lines: unknown[];
  stdin?: Readable;
  guide?: FakeGuide;
  createGuide?: (options: Record<string, unknown>) => Promise<FakeGuide | null>;
  native?: Record<string, unknown>;
  appPath?: string;
  runtime?: Record<string, unknown>;
  canPresent?: () => boolean;
  presentationTimeoutMs?: number;
}) {
  const guide = options.guide ?? fakeGuide();
  const stdout = output();
  const nativeCalls: Array<{ name: string; value: unknown }> = [];
  const createGuide = options.createGuide ?? (async (value: Record<string, unknown>) => {
    nativeCalls.push({ name: "createNativeAccessibilitySetupWindow", value });
    return guide;
  });
  const host = createPermissionHost({
    argv: ["node", "incodex-permission-host.cjs", "--nonce", NONCE],
    stdin: options.stdin ?? input(options.lines),
    stdout,
    appPath: options.appPath ?? APP_PATH,
    native: options.native ?? {
      createNativeAccessibilitySetupWindow: createGuide,
      runNativePermissionHandoff: (...args: unknown[]) => {
        nativeCalls.push({ name: "runNativePermissionHandoff", value: args });
      },
    },
    createGuide,
    copy: { title: "title", body: "body" },
    layoutDirection: "leftToRight",
    canPresent: options.canPresent ?? (() => true),
    presentationTimeoutMs: options.presentationTimeoutMs,
    runtime: options.runtime,
  });
  return { host, guide, stdout, nativeCalls };
}

async function waitForReadyOrError(stdout: { lines: string[] }): Promise<"ready" | "error"> {
  const ready = JSON.stringify({ nonce: NONCE, type: "ready" });
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (stdout.lines.includes(ready)) return "ready";
    if (stdout.lines.some((line) => line.includes('"type":"error"'))) return "error";
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  return "error";
}

function appKitRuntimeFixture(runningApplications: Array<Record<string, unknown>>, frontmostApplication: Record<string, unknown>) {
  const officialExecutable = `${APP_PATH}/Contents/MacOS/ChatGPT`;
  const windows = [{
    objectForKey$: (key: string) => ({
      kCGWindowOwnerPID: 700,
      kCGWindowLayer: 0,
      kCGWindowBounds: {
        objectForKey$: (boundsKey: string) => ({ X: 10, Y: 20, Width: 800, Height: 600 }[boundsKey]),
      },
    }[key]),
  }];
  const application = {
    nextEventMatchingMask$untilDate$inMode$dequeue$: () => null,
    sendEvent$: () => {},
    setActivationPolicy$: () => {},
    finishLaunching: () => {},
  };
  const workspace = {
    frontmostApplication: () => frontmostApplication,
    runningApplications: () => runningApplications,
  };
  const foundation = {
    NSDate: { dateWithTimeIntervalSinceNow$: () => ({}) },
    NSString: { stringWithUTF8String$: (value: string) => value },
    NSLocale: { preferredLanguages: () => [] },
  };
  class NobjcLibrary {
    NSApplication = { sharedApplication: () => application };
    NSWorkspace = { sharedWorkspace: () => workspace };
    NSDate = foundation.NSDate;
    NSString = foundation.NSString;
    NSLocale = foundation.NSLocale;
    constructor(_path: string) {}
  }
  return {
    officialExecutable,
    objc: {
      NobjcLibrary,
      RunLoop: { pump: () => false, stop: () => {} },
      callFunction: (name: string) => name === "CGWindowListCopyWindowInfo" ? windows : undefined,
    },
  };
}

function runningApp(pid: number, executablePath = `${APP_PATH}/Contents/MacOS/ChatGPT`) {
  return {
    bundleIdentifier: () => "com.openai.codex",
    executableURL: () => ({ path: () => executablePath }),
    isActive: () => true,
    processIdentifier: () => pid,
  };
}

describe("one-shot native permission host protocol", () => {
  test("fails closed when AppKit event dispatch selectors are unavailable", () => {
    expect(() => startAppKitEventPump({
      application: {},
      foundation: {},
      runLoop: { run: () => () => {} },
    })).toThrow(/nextEvent\/sendEvent/);
  });

  test("drains AppKit events before stopping the objc-js run loop", () => {
    const events = ["mouse", "display-link"];
    const delivered: string[] = [];
    const application = {
      nextEventMatchingMask$untilDate$inMode$dequeue$: () => events.shift(),
      sendEvent$: (event: string) => { delivered.push(event); },
    };
    const foundation = {
      NSDate: { dateWithTimeIntervalSinceNow$: () => ({}) },
      NSString: { stringWithUTF8String$: (value: string) => value },
    };
    let stopped = 0;
    const runLoop = { run: () => () => { stopped += 1; } };
    const timers: Array<() => void> = [];
    const pump = startAppKitEventPump({
      application,
      foundation,
      runLoop,
      setIntervalFn: ((callback: () => void) => { timers.push(callback); return {} as any; }) as any,
      clearIntervalFn: () => {},
    });

    pump.pumpOnce();
    expect(delivered).toEqual(["mouse", "display-link"]);
    expect(timers).toHaveLength(1);
    pump.stop();
    expect(stopped).toBe(1);
  });

  test("uses objc-js compatible unsigned NSEventMask bits", () => {
    let seenMask: unknown;
    const pump = startAppKitEventPump({
      application: {
        nextEventMatchingMask$untilDate$inMode$dequeue$: (mask: unknown) => {
          seenMask = mask;
          return null;
        },
        sendEvent$: () => {},
      },
      foundation: {
        NSDate: { dateWithTimeIntervalSinceNow$: () => ({}) },
        NSString: { stringWithUTF8String$: (value: string) => value },
      },
      runLoop: { run: () => () => {} },
      setIntervalFn: ((callback: () => void) => ({ callback })) as any,
      clearIntervalFn: () => {},
    });

    pump.pumpOnce();

    expect(seenMask).toBe(0xffffffffffffffffn);
    pump.stop();
  });

  test("reports an event-dispatch failure and stops the pump", () => {
    const errors: unknown[] = [];
    const timers: Array<() => void> = [];
    const pump = startAppKitEventPump({
      application: {
        nextEventMatchingMask$untilDate$inMode$dequeue$: () => "bad-event",
        sendEvent$: () => { throw new Error("event dispatch failed"); },
      },
      foundation: {
        NSDate: { dateWithTimeIntervalSinceNow$: () => ({}) },
        NSString: { stringWithUTF8String$: (value: string) => value },
      },
      runLoop: { run: () => () => {} },
      setIntervalFn: ((callback: () => void) => { timers.push(callback); return {} as any; }) as any,
      clearIntervalFn: () => {},
      onError: ((error: unknown) => { errors.push(error); }) as any,
    });

    timers[0]();

    expect(errors).toHaveLength(1);
    pump.stop();
  });

  test("canPresent requires one official executable process and the frontmost PID", () => {
    const front = runningApp(700);
    const duplicateFixture = appKitRuntimeFixture([front, runningApp(701)], front);
    const duplicateRuntime = makeAppKitRuntime(duplicateFixture.objc as any);
    try {
      expect(duplicateRuntime.canPresent()).toBe(false);
    } finally {
      duplicateRuntime.stopPump();
    }

    const wrongPidFixture = appKitRuntimeFixture([runningApp(701)], front);
    const wrongPidRuntime = makeAppKitRuntime(wrongPidFixture.objc as any);
    try {
      expect(wrongPidRuntime.canPresent()).toBe(false);
    } finally {
      wrongPidRuntime.stopPump();
    }

    const uniqueFixture = appKitRuntimeFixture([front], front);
    const uniqueRuntime = makeAppKitRuntime(uniqueFixture.objc as any);
    try {
      expect(uniqueRuntime.canPresent()).toBe(true);
    } finally {
      uniqueRuntime.stopPump();
    }
  });

  test("accepts only a hexadecimal nonce and pins the official app path", () => {
    expect(parseHostArgs(["node", "host.cjs", "--nonce", NONCE])).toEqual({ nonce: NONCE });
    expect(APP_PATH).toBe("/Applications/ChatGPT.app");
    expect(() => parseHostArgs(["node", "host.cjs"])).toThrow();
    expect(() => parseHostArgs(["node", "host.cjs", "--nonce", "a"])).toThrow(/32/);
    expect(() => parseHostArgs(["node", "host.cjs", "--nonce", `${NONCE}00`])).toThrow(/32/);
    expect(() => parseHostArgs(["node", "host.cjs", "--nonce", "../../tmp"])).toThrow();
    expect(() => parseHostArgs(["node", "host.cjs", "--nonce", NONCE, "--app", "/tmp/other.app"])).toThrow();
  });

  test("encodes exactly one nonce-bearing JSON line without log text", () => {
    expect(encodeHostMessage(NONCE, "ready", undefined as any)).toBe(`${JSON.stringify({ nonce: NONCE, type: "ready" })}\n`);
    expect(encodeHostMessage(NONCE, "error", "native host failed" as any)).toBe(
      `${JSON.stringify({ nonce: NONCE, type: "error", message: "native host failed" })}\n`,
    );
  });

  test("emits ready, forwards CLI state, and closes on granted", async () => {
    const stdin = new PassThrough();
    const { host, guide, stdout } = makeHost({
      lines: [],
      stdin,
    });
    const running = host.run();
    await waitForReady(stdout);
    stdin.write(`${JSON.stringify({ nonce: NONCE, type: "state", state: "awaiting-user" })}\n`);
    stdin.write(`${JSON.stringify({ nonce: NONCE, type: "state", state: "granted" })}\n`);
    stdin.end();

    await running;

    expect(stdout.lines).toEqual([JSON.stringify({ nonce: NONCE, type: "ready" })]);
    expect(guide.states).toEqual([
      { state: "awaiting-user" },
      { state: "granted" },
    ]);
    expect(guide.closeCalls).toBe(1);
  });

  test("sends allow only for the initial repair choice", async () => {
    const guide = fakeGuide();
    const stdin = new PassThrough();
    guide.resolveChoice("repair");
    const { host, stdout } = makeHost({
      lines: [],
      stdin,
      guide,
    });
    const running = host.run();
    await waitForReady(stdout);
    stdin.write(`${JSON.stringify({ nonce: NONCE, type: "close" })}\n`);
    stdin.end();

    await running;

    expect(stdout.lines).toContain(JSON.stringify({ nonce: NONCE, type: "ready" }));
    expect(stdout.lines).toContain(JSON.stringify({ nonce: NONCE, type: "allow" }));
    expect(stdout.lines.filter((line) => line.includes('"type":"allow"'))).toHaveLength(1);
  });

  test("sends retry for native Back/Allow without owning reset or Settings", async () => {
    const guide = fakeGuide();
    const stdin = new PassThrough();
    const { host, stdout } = makeHost({
      lines: [],
      stdin,
      guide,
    });
    const running = host.run();
    await Promise.resolve();
    guide.resolveChoice("repair");
    await new Promise((resolve) => setTimeout(resolve, 0));
    guide.emitRetry();
    stdin.write(`${JSON.stringify({ nonce: NONCE, type: "close" })}\n`);
    stdin.end();

    await running;

    expect(stdout.lines).toContain(JSON.stringify({ nonce: NONCE, type: "retry" }));
    expect(stdout.lines).not.toContain(JSON.stringify({ nonce: NONCE, type: "reset" }));
  });

  test("sends later and closes when the guide chooses Skip/Later", async () => {
    const guide = fakeGuide();
    const stdin = new PassThrough();
    const { host, stdout } = makeHost({ lines: [], stdin, guide });
    const running = host.run();
    await Promise.resolve();
    guide.resolveChoice("later");

    await running;

    expect(stdout.lines).toEqual([
      JSON.stringify({ nonce: NONCE, type: "ready" }),
      JSON.stringify({ nonce: NONCE, type: "later" }),
    ]);
    expect(guide.closeCalls).toBe(1);
  });

  test("rejects a wrong nonce and closes without forwarding the state", async () => {
    const guide = fakeGuide();
    const { host, stdout } = makeHost({
      lines: [{ nonce: "ffffffff", type: "state", state: "granted" }],
      guide,
    });

    await host.run();

    expect(stdout.lines.at(-1)).toMatch(/"type":"error"/);
    expect(guide.states).toEqual([]);
    expect(guide.closeCalls).toBe(0);
  });

  test("rejects an unknown state as a protocol error", async () => {
    const guide = fakeGuide();
    const { host, stdout } = makeHost({
      lines: [{ nonce: NONCE, type: "state", state: "unknown" }],
      guide,
    });

    await host.run();

    expect(stdout.lines.at(-1)).toMatch(/"type":"error"/);
    expect(guide.states).toEqual([]);
    expect(guide.closeCalls).toBe(0);
  });

  test("rejects malformed JSON as a protocol error", async () => {
    const guide = fakeGuide();
    const { host, stdout } = makeHost({ lines: ["not-json"], guide });

    await host.run();

    expect(stdout.lines.at(-1)).toMatch(/"type":"error"/);
    expect(guide.states).toEqual([]);
    expect(guide.closeCalls).toBe(0);
  });

  test("rejects an oversized input line before JSON parsing", async () => {
    const guide = fakeGuide();
    const { host, stdout } = makeHost({ lines: ["x".repeat(70_000)], guide });

    await host.run();

    expect(stdout.lines.at(-1)).toMatch(/"type":"error"/);
    expect(guide.states).toEqual([]);
    expect(guide.closeCalls).toBe(0);
  });

  test("EOF closes the native guide and does not claim success", async () => {
    const guide = fakeGuide();
    const stdin = new PassThrough();
    const { host, stdout } = makeHost({ lines: [], stdin, guide });
    const running = host.run();
    await waitForReady(stdout);
    stdin.end();

    await running;

    expect(guide.closeCalls).toBe(1);
    expect(stdout.lines).not.toContain(JSON.stringify({ nonce: NONCE, type: "granted" }));
  });

  test("EOF while waiting for the official window prevents late native guide creation", async () => {
    const guide = fakeGuide();
    const stdin = new PassThrough();
    let createCalls = 0;
    const { host, stdout } = makeHost({
      lines: [],
      stdin,
      guide,
      canPresent: () => false,
      presentationTimeoutMs: 5_000,
      createGuide: async () => {
        createCalls += 1;
        return guide;
      },
    });
    const running = host.run();
    await new Promise((resolve) => setTimeout(resolve, 0));
    stdin.end();

    await running;

    expect(createCalls).toBe(0);
    expect(stdout.lines).toEqual([]);
    expect(guide.closeCalls).toBe(0);
  });

  test("pre-ready close and granted state cannot create a late native guide", async () => {
    for (const state of ["close", "granted"] as const) {
      const guide = fakeGuide();
      const stdin = new PassThrough();
      let createCalls = 0;
      const { host, stdout } = makeHost({
        lines: [],
        stdin,
        guide,
        canPresent: () => false,
        presentationTimeoutMs: 5_000,
        createGuide: async () => {
          createCalls += 1;
          return guide;
        },
      });
      const running = host.run();
      await Promise.resolve();
      stdin.write(`${JSON.stringify(
        state === "close"
          ? { nonce: NONCE, type: "close" }
          : { nonce: NONCE, type: "state", state: "granted" },
      )}\n`);
      stdin.end();
      await running;
      expect(createCalls).toBe(0);
      expect(stdout.lines).toEqual([]);
      expect(guide.closeCalls).toBe(0);
    }
  });

  test("retries a transient null native guide within one presentation deadline", async () => {
    const stdin = new PassThrough();
    const guide = fakeGuide();
    let createCalls = 0;
    const { host, stdout } = makeHost({
      lines: [],
      stdin,
      guide,
      presentationTimeoutMs: 100,
      createGuide: async () => {
        createCalls += 1;
        return createCalls === 1 ? null : guide;
      },
    });
    const running = host.run();
    const outcome = await waitForReadyOrError(stdout);
    if (outcome === "ready") {
      guide.resolveChoice("later");
      stdin.end();
    } else {
      host.close();
      stdin.end();
    }
    await running;

    expect(outcome).toBe("ready");
    expect(createCalls).toBe(2);
    expect(stdout.lines).toContain(JSON.stringify({ nonce: NONCE, type: "ready" }));
    expect(stdout.lines).not.toContain(expect.stringContaining('"type":"error"'));
  });

  test("bounds a permanently unavailable native guide instead of spinning forever", async () => {
    const stdin = new PassThrough();
    let createCalls = 0;
    const { host, stdout } = makeHost({
      lines: [],
      stdin,
      presentationTimeoutMs: 40,
      createGuide: async () => {
        createCalls += 1;
        return null;
      },
    });
    await expect(host.run()).rejects.toThrow();
    stdin.end();

    expect(createCalls).toBeGreaterThan(1);
    expect(stdout.lines.at(-1)).toMatch(/"type":"error"/);
  });

  test("EOF during a null-guide retry prevents a later guide from being created", async () => {
    const stdin = new PassThrough();
    const guide = fakeGuide();
    let createCalls = 0;
    const { host, stdout } = makeHost({
      lines: [],
      stdin,
      guide,
      presentationTimeoutMs: 100,
      createGuide: async () => {
        createCalls += 1;
        setTimeout(() => stdin.end(), 0);
        return null;
      },
    });

    await host.run();

    expect(createCalls).toBe(1);
    expect(stdout.lines).toEqual([]);
    expect(guide.closeCalls).toBe(0);
  });

  test("does not allow a custom app path to become the drag/TCC target", () => {
    expect(() => makeHost({ lines: [], appPath: "/tmp/ChatGPT.app" })).toThrow();
  });
});
