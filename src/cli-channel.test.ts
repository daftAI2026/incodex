import { describe, expect, test } from "bun:test";
import { detectInstallChannel, selfUninstallPaths, updateAction } from "./cli-channel";

describe("detectInstallChannel", () => {
  test("Cellar or Homebrew prefix is homebrew", () => {
    expect(detectInstallChannel({ execPath: "/opt/homebrew/bin/incodex", argv1: "/opt/homebrew/bin/incodex" })).toBe(
      "homebrew",
    );
    expect(
      detectInstallChannel({
        execPath: "/usr/local/Cellar/incodex/0.1.0/bin/incodex",
        argv1: "/usr/local/bin/incodex",
      }),
    ).toBe("homebrew");
  });

  test("TypeScript entry is source", () => {
    expect(detectInstallChannel({ execPath: "/usr/local/bin/bun", argv1: "/Users/me/incodex/src/cli.ts" })).toBe(
      "source",
    );
  });

  test("Homebrew bun running src/cli.ts is still source", () => {
    expect(
      detectInstallChannel({ execPath: "/opt/homebrew/bin/bun", argv1: "/Users/me/incodex/src/cli.ts" }),
    ).toBe("source");
  });

  test("a compiled binary under ~/.local/bin is script", () => {
    expect(detectInstallChannel({ execPath: "/Users/me/.local/bin/incodex", argv1: "/Users/me/.local/bin/incodex" })).toBe(
      "script",
    );
  });
});

describe("updateAction", () => {
  test("homebrew refuses and tells the user to brew upgrade", () => {
    const action = updateAction("homebrew");
    expect(action.kind).toBe("refuse");
    if (action.kind === "refuse") expect(action.message).toContain("brew upgrade incodex");
  });

  test("source refuses and tells the user to pull the repo", () => {
    const action = updateAction("source");
    expect(action.kind).toBe("refuse");
    if (action.kind === "refuse") expect(action.message).toMatch(/git pull|bun link/);
  });

  test("legacy compiled TypeScript copy refuses instead of becoming a second updater", () => {
    const action = updateAction("script");
    expect(action.kind).toBe("refuse");
    if (action.kind === "refuse") expect(action.message).toContain("legacy TypeScript CLI updater has been retired");
  });
});

describe("selfUninstallPaths", () => {
  test("removes incodex and inc next to the script binary", () => {
    expect(selfUninstallPaths("/Users/me/.local/bin/incodex")).toEqual([
      "/Users/me/.local/bin/incodex",
      "/Users/me/.local/bin/inc",
    ]);
  });
});
