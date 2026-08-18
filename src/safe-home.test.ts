import { describe, expect, test } from "bun:test";
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  burnSessionHome,
  copySettings,
  createSessionHome,
  exclusiveCopyFile,
  FILE_MODE,
  LOG_LIMIT,
  rotateAndAppendLog,
  resolveSourceHome,
  sweepOrphanSessions,
} from "./runtime/incodex-safe-home.cjs";

function tempRoot(): string {
  return mkdtempSync(join(tmpdir(), "incodex-safe-"));
}

describe("symlink burn and copy", () => {
  test("refuses a symlink session root and does not delete the target", () => {
    const root = tempRoot();
    const victimDir = join(root, "victim-dir");
    const victim = join(victimDir, "victim.txt");
    mkdirSync(victimDir);
    writeFileSync(victim, "keep-me");
    const fakeHome = join(root, "incognito-home");
    symlinkSync(victimDir, fakeHome);

    expect(() =>
      burnSessionHome(fakeHome, { userRoot: join(root, ".incodex"), sessionId: "nope" }),
    ).toThrow(/symlink/);
    expect(readFileSync(victim, "utf8")).toBe("keep-me");
  });

  test("refuses when the session parent is a symlink", () => {
    const root = tempRoot();
    const outside = join(root, "outside");
    mkdirSync(outside);
    const userRoot = join(root, ".incodex");
    mkdirSync(userRoot);
    symlinkSync(outside, join(userRoot, "sessions"));
    expect(() => createSessionHome(userRoot)).toThrow(/symlink/);
    expect(lstatSync(join(userRoot, "sessions")).isSymbolicLink()).toBe(true);
  });

  test("copySettings refuses to follow a destination auth.json symlink", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const source = join(root, "codex");
    mkdirSync(source);
    writeFileSync(join(source, "auth.json"), '{"ok":true}');
    writeFileSync(join(source, "config.toml"), "localeOverride = \"zh-CN\"\n");
    const session = createSessionHome(userRoot);
    const outside = join(root, "outside-auth.json");
    writeFileSync(outside, "secret");
    symlinkSync(outside, join(session.home, "auth.json"));

    expect(() => copySettings(session.home, source, userRoot)).toThrow(/symlink|overwrite/);
    expect(readFileSync(outside, "utf8")).toBe("secret");
  });

  test("exclusiveCopyFile does not overwrite an existing unknown file", () => {
    const root = tempRoot();
    const src = join(root, "src.json");
    const dest = join(root, "dest.json");
    writeFileSync(src, "new");
    writeFileSync(dest, "old");
    expect(() => exclusiveCopyFile(src, dest)).toThrow(/overwrite/);
    expect(readFileSync(dest, "utf8")).toBe("old");
  });

  test("directory replacement race: inode change refuses burn", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot);
    const victimDir = join(root, "victim-dir");
    mkdirSync(victimDir);
    const victim = join(victimDir, "victim.txt");
    writeFileSync(victim, "keep-me");

    rmSync(session.root, { recursive: true, force: true });
    symlinkSync(victimDir, session.root);

    expect(() =>
      burnSessionHome(session.root, {
        userRoot,
        sessionId: session.sessionId,
        ino: session.ino,
        dev: session.dev,
      }),
    ).toThrow();
    expect(readFileSync(victim, "utf8")).toBe("keep-me");
  });

  test("createSessionHome uses a random directory under sessions with 0700/0600", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const first = createSessionHome(userRoot);
    const second = createSessionHome(userRoot);
    expect(first.home).not.toBe(second.home);
    expect(first.sessionId).not.toBe(second.sessionId);
    expect(first.root.includes(`${join(userRoot, "sessions")}`)).toBe(true);
    expect(first.home.endsWith(`${join("codex-home")}`) || first.home.endsWith("/codex-home")).toBe(true);
    expect(first.chromium.endsWith("/chromium")).toBe(true);
    expect(lstatSync(userRoot).mode & 0o777).toBe(0o700);
    expect(lstatSync(first.root).mode & 0o777).toBe(0o700);
    expect(lstatSync(join(first.root, "owner.json")).mode & 0o777).toBe(FILE_MODE);
    expect(lstatSync(join(first.root, "lock")).mode & 0o777).toBe(FILE_MODE);
    expect(lstatSync(first.root).isSymbolicLink()).toBe(false);
  });

  test("copySettings writes private files and burn removes the whole session", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const source = join(root, "codex");
    mkdirSync(source);
    writeFileSync(join(source, "auth.json"), '{"token":"x"}');
    const session = createSessionHome(userRoot);
    expect(copySettings(session.home, source, userRoot)).toBe(1);
    expect(readFileSync(join(session.home, "auth.json"), "utf8")).toBe('{"token":"x"}');
    expect(lstatSync(join(session.home, "auth.json")).mode & 0o777).toBe(FILE_MODE);
    writeFileSync(join(session.chromium, "Cache"), "cookie");
    burnSessionHome(session.root, {
      userRoot,
      sessionId: session.sessionId,
      ino: session.ino,
      dev: session.dev,
    });
    expect(() => lstatSync(session.root)).toThrow();
    expect(() => lstatSync(session.chromium)).toThrow();
    expect(readFileSync(join(source, "auth.json"), "utf8")).toBe('{"token":"x"}');
  });

  test("burn refuses a session id mismatch", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot);
    expect(() =>
      burnSessionHome(session.root, { userRoot, sessionId: "other-session", ino: session.ino, dev: session.dev }),
    ).toThrow(/session id/);
    expect(lstatSync(session.root).isDirectory()).toBe(true);
  });
});

describe("session lifecycle", () => {
  test("drops stale identity auth when the source auth.json is gone", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const source = join(root, "codex");
    mkdirSync(source);
    writeFileSync(join(source, "auth.json"), '{"token":"old"}');
    const first = createSessionHome(userRoot);
    copySettings(first.home, source, userRoot);
    expect(readFileSync(join(userRoot, "identity", "auth.json"), "utf8")).toBe('{"token":"old"}');
    rmSync(join(source, "auth.json"));
    const second = createSessionHome(userRoot);
    expect(copySettings(second.home, source, userRoot)).toBe(0);
    expect(() => lstatSync(join(userRoot, "identity", "auth.json"))).toThrow();
    expect(() => lstatSync(join(second.home, "auth.json"))).toThrow();
  });

  test("janitor burns sessions whose owner pid is dead and leaves a live one", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const dead = createSessionHome(userRoot, { pid: 999999 });
    writeFileSync(join(dead.chromium, "x"), "left");
    const live = createSessionHome(userRoot, { pid: process.pid });
    const swept = sweepOrphanSessions(userRoot, { keepSessionId: live.sessionId });
    expect(swept).toBeGreaterThanOrEqual(1);
    expect(() => lstatSync(dead.root)).toThrow();
    expect(lstatSync(live.root).isDirectory()).toBe(true);
  });

  test("clone and live targets do not share a session or chromium directory", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const live = createSessionHome(userRoot, { targetId: "official-aaa" });
    const clone = createSessionHome(userRoot, { targetId: "app-bbb" });
    expect(live.root).not.toBe(clone.root);
    expect(live.chromium).not.toBe(clone.chromium);
    expect(live.root.includes("official-aaa")).toBe(true);
    expect(clone.root.includes("app-bbb")).toBe(true);
  });

  test("incognito.log rotates instead of growing forever", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    mkdirSync(userRoot);
    const log = join(userRoot, "logs", "incognito.log");
    mkdirSync(join(userRoot, "logs"));
    writeFileSync(log, "x".repeat(LOG_LIMIT));
    rotateAndAppendLog(userRoot, "new-line\n");
    expect(readFileSync(join(userRoot, "logs", "incognito.log"), "utf8")).toBe("new-line\n");
    expect(lstatSync(`${log}.1`).isFile()).toBe(true);
  });

  test("custom CODEX_HOME is resolved instead of the default ~/.codex", () => {
    const custom = join(tempRoot(), "my-codex");
    expect(resolveSourceHome(custom, "/Users/me/.codex")).toBe(custom);
    expect(resolveSourceHome("  ", "/Users/me/.codex")).toBe("/Users/me/.codex");
  });
});
