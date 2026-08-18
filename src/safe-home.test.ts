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

    expect(() => copySettings(session.home, source)).toThrow(/symlink|overwrite/);
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

    rmSync(session.home, { recursive: true, force: true });
    symlinkSync(victimDir, session.home);

    expect(() =>
      burnSessionHome(session.home, {
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
    expect(first.home.includes(`${join(userRoot, "sessions")}`)).toBe(true);
    expect(lstatSync(userRoot).mode & 0o777).toBe(0o700);
    expect(lstatSync(first.home).mode & 0o777).toBe(0o700);
    expect(lstatSync(join(first.home, "owner.json")).mode & 0o777).toBe(FILE_MODE);
    expect(lstatSync(first.home).isSymbolicLink()).toBe(false);
  });

  test("copySettings writes private files and burn removes the whole session", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const source = join(root, "codex");
    mkdirSync(source);
    writeFileSync(join(source, "auth.json"), '{"token":"x"}');
    const session = createSessionHome(userRoot);
    expect(copySettings(session.home, source)).toBe(1);
    expect(readFileSync(join(session.home, "auth.json"), "utf8")).toBe('{"token":"x"}');
    expect(lstatSync(join(session.home, "auth.json")).mode & 0o777).toBe(FILE_MODE);
    burnSessionHome(session.home, {
      userRoot,
      sessionId: session.sessionId,
      ino: session.ino,
      dev: session.dev,
    });
    expect(() => lstatSync(session.home)).toThrow();
    expect(readFileSync(join(source, "auth.json"), "utf8")).toBe('{"token":"x"}');
  });

  test("burn refuses a session id mismatch", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot);
    expect(() =>
      burnSessionHome(session.home, { userRoot, sessionId: "other-session", ino: session.ino, dev: session.dev }),
    ).toThrow(/session id/);
    expect(lstatSync(session.home).isDirectory()).toBe(true);
  });
});
