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
  const start = main.indexOf("function readLocaleOverride(");
  const end = main.indexOf("function sessionBurnExpectation", start);
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
  const end = main.indexOf("async function attachElectron", start);
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
  test("uses the same 22-point source-window cascade as Mac without fixing window dimensions", () => {
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    const start = main.indexOf("const CHROME_WINDOW_TILE_PIXELS");
    const end = main.indexOf("function applyChromeWindowTile", start);
    expect(start).toBeGreaterThanOrEqual(0);
    expect(end).toBeGreaterThan(start);
    const source = { x: 250, y: 136, width: 1399, height: 820 };
    const screen = { getDisplayMatching: () => ({ workArea: { x: 0, y: 0, width: 4000, height: 2000 } }) };
    const expected = { x: 272, y: 158, width: source.width, height: source.height };
    for (const platform of ["darwin", "win32"]) {
      const tile = new Function("process", `${main.slice(start, end)}; return chromeTileBounds;`)({ platform });
      expect(tile(source, screen)).toEqual(expected);
    }
  });
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
