import { describe, expect, test } from "bun:test";
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  burnSessionHome,
  burnSessionHomeWithOwner,
  copySettings,
  createSessionHome,
  exclusiveCopyFile,
  FILE_MODE,
  LOG_LIMIT,
  rotateAndAppendLog,
  resolveSourceHome,
  seedWindowState,
  sweepOrphanSessions,
  handoffSessionOwner,
} from "./runtime/incodex-safe-home.cts";
import { processIdentity } from "./runtime/incodex-instance.cts";

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

  test("seedWindowState projects stable geometry and official fresh-home markers", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const source = join(root, "codex");
    mkdirSync(source);
    writeFileSync(
      join(source, ".codex-global-state.json"),
      JSON.stringify({
        "electron-main-window-bounds": {
          x: -40,
          y: 38,
          width: 1710,
          height: 1073,
          isMaximized: true,
        },
        "thread-titles": { secret: "must-not-cross" },
        "selected-project": "/private/project",
        "electron-persisted-atom-state": { secret: true },
      }),
    );
    const session = createSessionHome(userRoot);

    const before = Date.now();
    expect(seedWindowState(session.home, source)).toBe(true);
    const after = Date.now();

    const destination = join(session.home, ".codex-global-state.json");
    const state = JSON.parse(readFileSync(destination, "utf8"));
    expect(state["desktop-first-seen-at-ms"]).toBeGreaterThanOrEqual(before);
    expect(state["desktop-first-seen-at-ms"]).toBeLessThanOrEqual(after);
    delete state["desktop-first-seen-at-ms"];
    expect(state).toEqual({
      "electron-main-window-bounds": {
        x: -40,
        y: 38,
        width: 1710,
        height: 1073,
      },
      "electron-persisted-atom-state": {
        "chatgpt-migration-announcement-completed-v1": true,
        "chatgpt-update-downloaded-announcement-seen-v1": true,
      },
    });
    expect(lstatSync(destination).mode & 0o777).toBe(FILE_MODE);
  });

  test("seedWindowState prefers live geometry over stale persisted bounds", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const source = join(root, "codex");
    mkdirSync(source);
    writeFileSync(
      join(source, ".codex-global-state.json"),
      JSON.stringify({
        "electron-main-window-bounds": {
          x: 0,
          y: 38,
          width: 1710,
          height: 1073,
          isMaximized: true,
        },
      }),
    );
    const session = createSessionHome(userRoot);

    expect(
      seedWindowState(session.home, source, {
        x: 597,
        y: 34,
        width: 869,
        height: 1073,
      }),
    ).toBe(true);

    const state = JSON.parse(readFileSync(join(session.home, ".codex-global-state.json"), "utf8"));
    expect(state["electron-main-window-bounds"]).toEqual({
      x: 597,
      y: 34,
      width: 869,
      height: 1073,
    });
  });

  test("seedWindowState skips malformed bounds and preserves copied language settings", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const source = join(root, "codex");
    mkdirSync(source);
    writeFileSync(join(source, "auth.json"), '{"token":"source"}\n');
    writeFileSync(join(source, "config.toml"), 'localeOverride = "zh-CN"\n');
    writeFileSync(
      join(source, ".codex-global-state.json"),
      JSON.stringify({
        "electron-main-window-bounds": {
          x: 0,
          y: 0,
          width: "wide",
          height: 820,
          isMaximized: false,
        },
      }),
    );
    const session = createSessionHome(userRoot);

    expect(copySettings(session.home, source)).toBe(2);
    expect(seedWindowState(session.home, source)).toBe(false);

    expect(existsSync(join(session.home, ".codex-global-state.json"))).toBe(false);
    expect(readFileSync(join(session.home, "config.toml"), "utf8")).toBe(
      'localeOverride = "zh-CN"\n',
    );
    expect(readFileSync(join(session.home, "auth.json"), "utf8")).toBe(
      '{"token":"source"}\n',
    );
  });

  test("seedWindowState refuses a source global-state symlink", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const source = join(root, "codex");
    mkdirSync(source);
    const outside = join(root, "outside-global-state.json");
    writeFileSync(
      outside,
      JSON.stringify({
        "electron-main-window-bounds": {
          x: 0,
          y: 0,
          width: 900,
          height: 700,
          isMaximized: false,
        },
      }),
    );
    symlinkSync(outside, join(source, ".codex-global-state.json"));
    const session = createSessionHome(userRoot);

    expect(() => seedWindowState(session.home, source)).toThrow(/symlink/);
    expect(existsSync(join(session.home, ".codex-global-state.json"))).toBe(false);
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
    expect(existsSync(join(userRoot, "identity"))).toBe(false);
  });

  test("session owner records the same process start identity as Runtime owner metadata", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot, { pid: process.pid });
    const owner = JSON.parse(readFileSync(join(session.root, "owner.json"), "utf8"));
    expect(owner.processStartIdentity).toBe(processIdentity(process.pid).processStartIdentity);
  });

  test("janitor treats a reused PID as an orphan and burns its session", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot, { pid: process.pid });
    const ownerPath = join(session.root, "owner.json");
    const owner = JSON.parse(readFileSync(ownerPath, "utf8"));
    owner.processStartIdentity = "Fri Aug 22 10:37:03 2025";
    writeFileSync(ownerPath, `${JSON.stringify(owner)}\n`);

    expect(sweepOrphanSessions(userRoot)).toBe(1);
    expect(existsSync(session.root)).toBe(false);
  });

  test("janitor retains a live session when process identity is unknown", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot, { pid: process.pid });
    expect(
      sweepOrphanSessions(userRoot, {
        pidAlive: () => true,
        processIdentity: () => null,
      }),
    ).toBe(0);
    expect(existsSync(session.root)).toBe(true);
  });

  test("janitor retains a live session with an unparseable legacy process identity", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot, { pid: process.pid });
    const ownerPath = join(session.root, "owner.json");
    const owner = JSON.parse(readFileSync(ownerPath, "utf8"));
    owner.processStartIdentity = "六  8月/22 10:37:03 2026";
    writeFileSync(ownerPath, `${JSON.stringify(owner)}\n`);

    expect(
      sweepOrphanSessions(userRoot, {
        pidAlive: () => true,
        processIdentity: () => ({ processStartIdentity: "Sat Aug 22 10:37:03 2026" }),
      }),
    ).toBe(0);
    expect(existsSync(session.root)).toBe(true);
  });

  test("janitor retains a live session with non-C locale identity words", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot, { pid: process.pid });
    const ownerPath = join(session.root, "owner.json");
    const owner = JSON.parse(readFileSync(ownerPath, "utf8"));
    owner.processStartIdentity = "Sab Ago 22 10:37:03 2025";
    writeFileSync(ownerPath, `${JSON.stringify(owner)}\n`);

    expect(
      sweepOrphanSessions(userRoot, {
        pidAlive: () => true,
        processIdentity: () => ({ processStartIdentity: "Sat Aug 22 10:37:03 2025" }),
      }),
    ).toBe(0);
    expect(existsSync(session.root)).toBe(true);
  });

  test("process identity probe pins the C locale for ps output", () => {
    const source = readFileSync(join(import.meta.dir, "runtime/incodex-owner-core.cts"), "utf8");
    expect(source).toContain("LC_ALL: \"C\"");
  });

  test("burn revalidates the owner snapshot before deleting the session", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot, { pid: process.pid });
    const ownerPath = join(session.root, "owner.json");
    const owner = JSON.parse(readFileSync(ownerPath, "utf8"));
    const snapshot = {
      pid: owner.pid,
      processStartIdentity: owner.processStartIdentity,
    };
    owner.pid = 999999;
    writeFileSync(ownerPath, `${JSON.stringify(owner)}\n`);

    expect(() =>
      burnSessionHomeWithOwner(session.root, {
        userRoot,
        sessionId: session.sessionId,
        ino: session.ino,
        dev: session.dev,
      }, snapshot),
    ).toThrow(/owner/);
    expect(existsSync(session.root)).toBe(true);
  });

  test("session owner handoff updates PID and process start identity atomically", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot, { pid: 999999 });
    handoffSessionOwner(session.root, process.pid);
    const owner = JSON.parse(readFileSync(join(session.root, "owner.json"), "utf8"));
    expect(owner.pid).toBe(process.pid);
    expect(owner.processStartIdentity).toBe(processIdentity(process.pid).processStartIdentity);
  });

  test("an open-created session stays pending until handoff and janitor retains it", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot, { pid: 999999, handoffPending: true });
    const owner = JSON.parse(readFileSync(join(session.root, "owner.json"), "utf8"));
    expect(owner.handoffPending).toBe(true);
    expect(sweepOrphanSessions(userRoot)).toBe(0);
    expect(existsSync(session.root)).toBe(true);
  });

  test("handoff clears the open pending marker in the same owner publication", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot, { pid: 999999, handoffPending: true });
    handoffSessionOwner(session.root, process.pid);
    const owner = JSON.parse(readFileSync(join(session.root, "owner.json"), "utf8"));
    expect(owner.handoffPending).toBe(false);
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

  test("burn still removes a leftover session after owner.json is already gone", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot);
    writeFileSync(join(session.chromium, "Cache"), "cookie");
    rmSync(join(session.root, "owner.json"));
    burnSessionHome(session.root, { userRoot, sessionId: session.sessionId, ino: session.ino, dev: session.dev });
    expect(() => lstatSync(session.root)).toThrow();
  });

  test("burn without owner.json still refuses a folder whose name is not the session id", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = createSessionHome(userRoot);
    rmSync(join(session.root, "owner.json"));
    expect(() => burnSessionHome(session.root, { userRoot, sessionId: "s-other" })).toThrow(/missing session owner/);
    expect(lstatSync(session.root).isDirectory()).toBe(true);
  });
});

describe("session lifecycle", () => {
  test("copies source settings directly and leaves an existing identity cache untouched", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const source = join(root, "codex");
    mkdirSync(source);
    writeFileSync(join(source, "auth.json"), '{"token":"source"}');
    writeFileSync(join(source, "config.toml"), 'localeOverride = "zh-CN"\n');
    mkdirSync(join(userRoot, "identity"), { recursive: true });
    writeFileSync(join(userRoot, "identity", "auth.json"), "legacy-cache\n");
    const first = createSessionHome(userRoot);
    copySettings(first.home, source);
    expect(readFileSync(join(userRoot, "identity", "auth.json"), "utf8")).toBe("legacy-cache\n");
    expect(readFileSync(join(first.home, "auth.json"), "utf8")).toBe('{"token":"source"}');
    expect(readFileSync(join(first.home, "config.toml"), "utf8")).toBe('localeOverride = "zh-CN"\n');
    expect(readFileSync(join(source, "auth.json"), "utf8")).toBe('{"token":"source"}');
    expect(readFileSync(join(source, "config.toml"), "utf8")).toBe('localeOverride = "zh-CN"\n');
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

  test("janitor refuses a replaced session root without recorded identity", () => {
    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const dead = createSessionHome(userRoot, { pid: 999999 });
    rmSync(dead.root, { recursive: true, force: true });
    mkdirSync(dead.root);
    writeFileSync(join(dead.root, "replacement.txt"), "keep-me");

    expect(sweepOrphanSessions(userRoot)).toBe(0);
    expect(readFileSync(join(dead.root, "replacement.txt"), "utf8")).toBe("keep-me");
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
