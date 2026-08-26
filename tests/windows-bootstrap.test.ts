import { describe, expect, test } from "bun:test";
import { join } from "node:path";

const bootstrapPath = join(import.meta.dir, "../crates/incodex-cli/assets/incodex-windows-bootstrap.cjs");

describe("Windows Runtime bootstrap", () => {
  test("attaches the shared main once in the Owl browser process", () => {
    const bootstrap = require(bootstrapPath) as {
      attachWindowsRuntime(options: {
        env: Record<string, string | undefined>;
        load: (path: string) => void;
        processType: string;
      }): boolean;
    };
    const env: Record<string, string | undefined> = {
      INCODEX_WINDOWS_REGISTRATION_ID: "0123456789abcdef0123456789abcdef",
    };
    const loaded: string[] = [];
    const options = {
      env,
      load: (path: string) => loaded.push(path),
      processType: "browser",
    };

    expect(bootstrap.attachWindowsRuntime(options)).toBe(true);
    expect(bootstrap.attachWindowsRuntime(options)).toBe(false);
    expect(loaded).toHaveLength(1);
    expect(loaded[0]).toEndWith("incodex-main.cjs");
  });

  test("does nothing outside an owned Owl browser process", () => {
    const bootstrap = require(bootstrapPath) as {
      attachWindowsRuntime(options: {
        env: Record<string, string | undefined>;
        load: (path: string) => void;
        processType: string;
      }): boolean;
    };
    for (const [processType, registrationId] of [
      ["renderer", "0123456789abcdef0123456789abcdef"],
      ["utility", "0123456789abcdef0123456789abcdef"],
      ["browser", ""],
      ["browser", "not-an-install-id"],
    ]) {
      const loaded: string[] = [];
      expect(
        bootstrap.attachWindowsRuntime({
          env: { INCODEX_WINDOWS_REGISTRATION_ID: registrationId },
          load: (path: string) => loaded.push(path),
          processType,
        }),
      ).toBe(false);
      expect(loaded).toEqual([]);
    }
  });
});
