import { describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import * as runtimeSafeHome from "./runtime/incodex-safe-home.cts";

function tempRoot(): string {
  return mkdtempSync(join("/tmp", "incodex-late-recreation-"));
}

describe("Runtime late session recreation", () => {
  test("only a proven deletion permits path-only cleanup of a late replacement", () => {
    const cleanupExpectedForAttempt = (runtimeSafeHome as any).cleanupExpectedForAttempt;
    expect(typeof cleanupExpectedForAttempt).toBe("function");

    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = runtimeSafeHome.createSessionHome(userRoot, { pid: 999999 });
    const expected = {
      userRoot,
      sessionId: session.sessionId,
      ino: session.ino,
      dev: session.dev,
    };

    const firstExpected = cleanupExpectedForAttempt(expected, false);
    expect(firstExpected.ino).toBe(session.ino);
    expect(firstExpected.dev).toBe(session.dev);
    expect(runtimeSafeHome.burnSessionHome(session.root, firstExpected)).toBe(true);

    mkdirSync(session.root);
    writeFileSync(join(session.root, "late-plugin-cache"), "late\n");
    const lateExpected = cleanupExpectedForAttempt(expected, true);
    expect(lateExpected.ino).toBeUndefined();
    expect(lateExpected.dev).toBeUndefined();
    expect(runtimeSafeHome.burnSessionHome(session.root, lateExpected)).toBe(true);
    expect(existsSync(session.root)).toBe(false);
  });

  test("replacement before the first proven deletion keeps inode/device binding", () => {
    const cleanupExpectedForAttempt = (runtimeSafeHome as any).cleanupExpectedForAttempt;
    expect(typeof cleanupExpectedForAttempt).toBe("function");

    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = runtimeSafeHome.createSessionHome(userRoot, { pid: 999999 });
    const expected = {
      userRoot,
      sessionId: session.sessionId,
      ino: session.ino,
      dev: session.dev,
    };
    rmSync(session.root, { recursive: true, force: true });
    mkdirSync(session.root);
    writeFileSync(join(session.root, "replacement"), "keep\n");

    const conservative = cleanupExpectedForAttempt(expected, false);
    expect(() => runtimeSafeHome.burnSessionHome(session.root, conservative)).toThrow(/inode|device/);
    expect(existsSync(join(session.root, "replacement"))).toBe(true);
  });
});
