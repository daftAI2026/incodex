import { describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as runtimeMain from "../dist/incodex-main.cjs";
import * as safeHome from "./runtime/incodex-safe-home.cts";

function tempRoot(): string {
  return mkdtempSync(join(tmpdir(), "incodex-runtime-main-"));
}

describe("Electron session preparation", () => {
  test("partial settings copy burns the identity-bound session root", () => {
    const prepareIncognitoSession = (runtimeMain as any).prepareIncognitoSession;
    expect(typeof prepareIncognitoSession).toBe("function");

    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const sourceHome = join(root, ".codex");
    mkdirSync(sourceHome);
    writeFileSync(join(sourceHome, "auth.json"), '{"token":"partial"}\n');
    const configTarget = join(root, "config-target.toml");
    writeFileSync(configTarget, 'localeOverride = "zh-CN"\n');
    symlinkSync(configTarget, join(sourceHome, "config.toml"));

    let sessionRoot = "";
    let authWasCopied = false;
    const result = prepareIncognitoSession({
      userRoot,
      sourceHomePath: sourceHome,
      appTarget: "test-target",
      pid: process.pid,
      createSessionHome: safeHome.createSessionHome,
      copySettings: (home: string, source: string) => {
        sessionRoot = safeHome.sessionRootFromHome(home);
        try {
          return safeHome.copySettings(home, source);
        } finally {
          authWasCopied = existsSync(join(home, "auth.json"));
        }
      },
      burnSessionHome: safeHome.burnSessionHome,
      log: () => {},
    });

    expect(authWasCopied).toBe(true);
    expect(result).toEqual({ ok: false, reason: "prepare-failed" });
    expect(sessionRoot).not.toBe("");
    expect(existsSync(sessionRoot)).toBe(false);
  });

  test("child burn refuses a replaced owner manifest after handoff", () => {
    const burnIncognitoSession = (runtimeMain as any).burnIncognitoSession;
    expect(typeof burnIncognitoSession).toBe("function");

    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = safeHome.createSessionHome(userRoot, { pid: process.pid });
    const ownerPath = join(session.root, "owner.json");
    const owner = JSON.parse(readFileSync(ownerPath, "utf8"));
    const snapshot = {
      pid: owner.pid,
      processStartIdentity: owner.processStartIdentity,
    };
    owner.processStartIdentity = "tampered-after-handoff";
    writeFileSync(ownerPath, `${JSON.stringify(owner)}\n`);

    expect(() => burnIncognitoSession(session, snapshot, userRoot)).toThrow(/owner/);
    expect(existsSync(session.root)).toBe(true);
    expect(safeHome.readBurnProof(session.root, userRoot, session.sessionId)).toBeNull();
  });

  test("child burn removes the session and writes proof with its handoff snapshot", () => {
    const burnIncognitoSession = (runtimeMain as any).burnIncognitoSession;
    expect(typeof burnIncognitoSession).toBe("function");

    const root = tempRoot();
    const userRoot = join(root, ".incodex");
    const session = safeHome.createSessionHome(userRoot, { pid: process.pid });
    const owner = JSON.parse(readFileSync(join(session.root, "owner.json"), "utf8"));
    const snapshot = {
      pid: owner.pid,
      processStartIdentity: owner.processStartIdentity,
    };

    expect(burnIncognitoSession(session, snapshot, userRoot)).toBe(true);
    expect(existsSync(session.root)).toBe(false);
    expect(safeHome.readBurnProof(session.root, userRoot, session.sessionId)).not.toBeNull();
  });
});
