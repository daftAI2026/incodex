import { describe, expect, test } from "bun:test";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

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
    "process",
    `${main.slice(start, end)}; return readLocaleOverride;`,
  )(
    { existsSync, readFileSync },
    { join },
    () => sourceHome,
    { platform: "win32" },
  ) as () => string;
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

describe("installed Windows Runtime session preparation", () => {
  test("injects a literal TOML locale into the renderer", async () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-runtime-windows-main-"));
    try {
      const sourceHome = join(root, ".codex");
      mkdirSync(sourceHome);
      writeFileSync(join(sourceHome, "config.toml"), "localeOverride = 'zh-CN'\n");

      const scripts: string[] = [];
      const hookWindow = loadInstalledRuntimeHookWindow(
        loadInstalledRuntimeLocaleReader(sourceHome),
      );
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
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});
