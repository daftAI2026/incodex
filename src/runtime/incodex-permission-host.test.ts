import { describe, expect, test } from "bun:test";
import { PassThrough, Readable } from "node:stream";

import {
  APP_PATH,
  createPermissionHost,
  encodeHostMessage,
  parseHostArgs,
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

function makeHost(options: {
  lines: unknown[];
  stdin?: Readable;
  guide?: FakeGuide;
  createGuide?: (options: Record<string, unknown>) => Promise<FakeGuide>;
  native?: Record<string, unknown>;
  appPath?: string;
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
    canPresent: () => true,
  });
  return { host, guide, stdout, nativeCalls };
}

describe("one-shot native permission host protocol", () => {
  test("accepts only a hexadecimal nonce and pins the official app path", () => {
    expect(parseHostArgs(["node", "host.cjs", "--nonce", NONCE])).toEqual({ nonce: NONCE });
    expect(APP_PATH).toBe("/Applications/ChatGPT.app");
    expect(() => parseHostArgs(["node", "host.cjs"])).toThrow();
    expect(() => parseHostArgs(["node", "host.cjs", "--nonce", "../../tmp"])).toThrow();
    expect(() => parseHostArgs(["node", "host.cjs", "--nonce", NONCE, "--app", "/tmp/other.app"])).toThrow();
  });

  test("encodes exactly one nonce-bearing JSON line without log text", () => {
    expect(encodeHostMessage(NONCE, "ready")).toBe(`${JSON.stringify({ nonce: NONCE, type: "ready" })}\n`);
    expect(encodeHostMessage(NONCE, "error", "native host failed")).toBe(
      `${JSON.stringify({ nonce: NONCE, type: "error", message: "native host failed" })}\n`,
    );
  });

  test("emits ready, forwards CLI state, and closes on granted", async () => {
    const { host, guide, stdout } = makeHost({
      lines: [
        { nonce: NONCE, type: "state", state: "awaiting-user" },
        { nonce: NONCE, type: "state", state: "granted" },
      ],
    });

    await host.run();

    expect(stdout.lines).toEqual([JSON.stringify({ nonce: NONCE, type: "ready" })]);
    expect(guide.states).toEqual([
      { state: "awaiting-user" },
      { state: "granted" },
    ]);
    expect(guide.closeCalls).toBe(1);
  });

  test("sends allow only for the initial repair choice", async () => {
    const guide = fakeGuide();
    guide.resolveChoice("repair");
    const { host, stdout } = makeHost({
      lines: [{ nonce: NONCE, type: "close" }],
      guide,
    });

    await host.run();

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

  test("rejects wrong nonce, unknown state, malformed JSON, and oversized lines", async () => {
    const guide = fakeGuide();
    const { host, stdout } = makeHost({
      lines: [
        { nonce: "ffffffff", type: "state", state: "granted" },
        { nonce: NONCE, type: "state", state: "unknown" },
        "not-json",
        "x".repeat(70_000),
      ],
      guide,
    });

    await host.run();

    expect(stdout.lines[0]).toBe(JSON.stringify({ nonce: NONCE, type: "ready" }));
    expect(stdout.lines.at(-1)).toMatch(/"type":"error"/);
    expect(guide.closeCalls).toBe(1);
  });

  test("EOF closes the native guide and does not claim success", async () => {
    const guide = fakeGuide();
    const { host, stdout } = makeHost({ lines: [], guide });

    await host.run();

    expect(guide.closeCalls).toBe(1);
    expect(stdout.lines).not.toContain(JSON.stringify({ nonce: NONCE, type: "granted" }));
  });

  test("does not allow a custom app path to become the drag/TCC target", () => {
    expect(() => makeHost({ lines: [], appPath: "/tmp/ChatGPT.app" })).toThrow();
  });
});
