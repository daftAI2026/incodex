import { describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, symlinkSync, writeFileSync } from "node:fs";
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
});
