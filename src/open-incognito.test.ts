import { describe, expect, test } from "bun:test";
import { EventEmitter } from "node:events";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  chatGptBinary,
  describeIncognitoOpen,
  formatSessionCleanup,
  prepareIncognitoOpen,
  waitAndBurn,
} from "./open-incognito";

function tempRoot(): string {
  return mkdtempSync(join(tmpdir(), "incodex-open-"));
}

function fakeApp(root: string): string {
  const app = join(root, "ChatGPT.app");
  const mac = join(app, "Contents", "MacOS");
  mkdirSync(mac, { recursive: true });
  writeFileSync(join(mac, "ChatGPT"), "#!/bin/sh\nexit 0\n");
  return app;
}

describe("open incognito without patching", () => {
  test("resolves the official executable inside the .app", () => {
    expect(chatGptBinary("/Applications/ChatGPT.app")).toBe(
      "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
    );
  });

  test("dry-run description does not create a session", () => {
    const root = tempRoot();
    const app = fakeApp(root);
    const described = describeIncognitoOpen({ appPath: app, userRoot: join(root, "home") });
    expect(described.bin).toBe(chatGptBinary(app));
    expect(described.args[0]).toMatch(/^--user-data-dir=/);
    expect(existsSync(join(root, "home"))).toBe(false);
  });

  test("prepare uses isolated home and Chromium user-data-dir", () => {
    const root = tempRoot();
    const app = fakeApp(root);
    const sourceHome = join(root, "codex");
    mkdirSync(sourceHome);
    writeFileSync(join(sourceHome, "auth.json"), "{}\n");
    writeFileSync(join(sourceHome, "config.toml"), "model = \"test\"\n");
    const plan = prepareIncognitoOpen({
      appPath: app,
      userRoot: join(root, "home"),
      sourceHome,
      pid: process.pid,
    });
    expect(plan.args).toEqual([`--user-data-dir=${plan.chromium}`]);
    expect(plan.env.CODEX_HOME).toBe(plan.home);
    expect(plan.env.CODEX_ELECTRON_USER_DATA_PATH).toBe(plan.chromium);
    expect(plan.env.INCODEX_INCOGNITO).toBe("1");
    expect(readFileSync(join(plan.home, "auth.json"), "utf8")).toBe("{}\n");
    expect(existsSync(join(app, "Contents/Resources/app.asar"))).toBe(false);
  });

  test("copySettings failure burns the session it just created", () => {
    const root = tempRoot();
    const app = fakeApp(root);
    const sourceHome = join(root, "codex");
    mkdirSync(sourceHome);
    expect(() =>
      prepareIncognitoOpen({
        appPath: app,
        userRoot: join(root, "home"),
        sourceHome,
        pid: process.pid,
        copySettings: () => {
          throw new Error("copy failed");
        },
      }),
    ).toThrow("copy failed");
    const sessions = join(root, "home", "sessions");
    const remaining = existsSync(sessions)
      ? readdirSync(sessions, { recursive: true }).map(String)
      : [];
    expect(remaining.some((name) => name.includes("codex-home") || name.includes("chromium"))).toBe(
      false,
    );
  });

  test("spawn error still burns the session", async () => {
    const root = tempRoot();
    const app = fakeApp(root);
    const sourceHome = join(root, "codex");
    mkdirSync(sourceHome);
    writeFileSync(join(sourceHome, "auth.json"), "{}\n");
    const userRoot = join(root, "home");
    const plan = prepareIncognitoOpen({
      appPath: app,
      userRoot,
      sourceHome,
      pid: process.pid,
    });
    const spawnImpl = (() => {
      const child = new EventEmitter() as EventEmitter & { killed?: boolean };
      queueMicrotask(() => child.emit("error", new Error("ENOENT")));
      return child;
    }) as unknown as typeof import("node:child_process").spawn;
    const result = await waitAndBurn(plan, userRoot, spawnImpl, { retryDelayMs: 0 });
    expect(existsSync(plan.sessionRoot)).toBe(false);
    expect(result.cleanup.removed).toBe(true);
  });

  test("a burn that cannot delete does not claim the session was removed", async () => {
    const root = tempRoot();
    const app = fakeApp(root);
    const sourceHome = join(root, "codex");
    mkdirSync(sourceHome);
    const userRoot = join(root, "home");
    const plan = prepareIncognitoOpen({
      appPath: app,
      userRoot,
      sourceHome,
      pid: process.pid,
    });
    const spawnImpl = (() => {
      const child = new EventEmitter() as EventEmitter & { killed?: boolean };
      queueMicrotask(() => child.emit("exit", 0));
      return child;
    }) as unknown as typeof import("node:child_process").spawn;
    const result = await waitAndBurn(plan, userRoot, spawnImpl, {
      retryDelayMs: 0,
      burn: () => {
        throw new Error("EPERM");
      },
    });
    expect(existsSync(plan.sessionRoot)).toBe(true);
    expect(result.cleanup).toEqual({
      removed: false,
      attempts: 5,
      retainedPath: plan.sessionRoot,
      reason: "EPERM",
    });
    expect(formatSessionCleanup(result.cleanup).ok).toBe(false);
    expect(formatSessionCleanup(result.cleanup).message).not.toMatch(/removed/i);
  });
});

