import { describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as safeHome from "./runtime/incodex-safe-home.cts";

const runtimeMain = await (async () => {
  if (process.platform !== "win32") return import("../dist/incodex-main.cjs");
  const descriptor = Object.getOwnPropertyDescriptor(process, "platform");
  Object.defineProperty(process, "platform", { value: "linux" });
  try {
    return await import("../dist/incodex-main.cjs");
  } finally {
    if (descriptor) Object.defineProperty(process, "platform", descriptor);
  }
})();

function tempRoot(): string {
  return mkdtempSync(join(tmpdir(), "incodex-runtime-main-"));
}

function loadInstalledRuntimeLocaleReader(sourceHome: string): () => string {
  const main = readFileSync(join(import.meta.dir, "../dist/incodex-main.cjs"), "utf8");
  const start = main.indexOf("function readLocaleOverride() {");
  const end = main.indexOf("\nfunction sessionBurnExpectation", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  return new Function(
    "fs",
    "path",
    "sourceHome",
    `${main.slice(start, end)}; return readLocaleOverride;`,
  )({ existsSync, readFileSync }, { join }, () => sourceHome) as () => string;
}

function loadInstalledRuntimeHookWindow(readLocaleOverride: () => string) {
  const main = readFileSync(join(import.meta.dir, "../dist/incodex-main.cjs"), "utf8");
  const start = main.indexOf("function hookWindow(");
  const end = main.indexOf("\nasync function attachElectron", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  return new Function(
    "isAuxiliaryWindow",
    "rememberWindow",
    "hookPreload",
    "ipcGuard",
    "allowedWindows",
    "trustedOrigins",
    "readLocaleOverride",
    "isIncognito",
    "process",
    "windowsPlatform",
    "reportInjectionError",
    "reportInjectionProbe",
    "markAcceptedWindowReady",
    `${main.slice(start, end)}; return hookWindow;`,
  )(
    () => false,
    () => {},
    () => {},
    { bindWindowIdentity: () => true },
    new Map(),
    new Set(),
    readLocaleOverride,
    () => true,
    { platform: "win32" },
    null,
    () => {},
    () => Promise.resolve(undefined),
    () => {},
  ) as (win: unknown, source: string) => void;
}

describe("Electron session preparation", () => {
  test("installed Windows Runtime injects a literal TOML locale into the renderer", async () => {
    const root = tempRoot();
    const sourceHome = join(root, ".codex");
    mkdirSync(sourceHome);
    writeFileSync(join(sourceHome, "config.toml"), "localeOverride = 'zh-CN'\n");

    const scripts: string[] = [];
    const hookWindow = loadInstalledRuntimeHookWindow(loadInstalledRuntimeLocaleReader(sourceHome));
    hookWindow(
      {
        webContents: {
          session: {},
          isDestroyed: () => false,
          on: () => {},
          executeJavaScript: (script: string) => {
            scripts.push(script);
            return Promise.resolve(undefined);
          },
        },
      },
      "window.__incodexInjected = true;",
    );
    await Promise.resolve();

    expect(scripts[0]).toContain('window.__incodexLocale="zh-CN";');
  });

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
