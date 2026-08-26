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
        readState: (path: string) => unknown;
        runtimeDir: string;
      }): boolean;
    };
    const registrationId = "0123456789abcdef0123456789abcdef";
    const packageFullName = "OpenAI.Codex_1.2.3.4_x64__publisher";
    const runtimeDir = "C:\\Users\\test\\.incodex\\runtime\\releases\\0.5.0-releasehash";
    const env: Record<string, string | undefined> = {
      INCODEX_WINDOWS_REGISTRATION_ID: registrationId,
      INCODEX_WINDOWS_PACKAGE_FULL_NAME: packageFullName,
      INCODEX_WINDOWS_STATE_PATH: "C:\\Users\\test\\.incodex\\windows-install.json",
    };
    const loaded: string[] = [];
    const options = {
      env,
      load: (path: string) => loaded.push(path),
      processType: "browser",
      readState: () => ({
        schemaVersion: 1,
        desired: "enabled",
        phase: "enabled-unobserved",
        registrationId,
        packageFullName,
        runtimeRelease: "0.5.0-releasehash",
      }),
      runtimeDir,
    };

    expect(bootstrap.attachWindowsRuntime(options)).toBe(true);
    expect(bootstrap.attachWindowsRuntime(options)).toBe(false);
    expect(loaded).toHaveLength(1);
    expect(loaded[0]).toEndWith("incodex-main.cjs");
  });

  test("fails closed when durable install ownership is absent, disabled, or mismatched", () => {
    const bootstrap = require(bootstrapPath) as {
      attachWindowsRuntime(options: {
        env: Record<string, string | undefined>;
        load: (path: string) => void;
        processType: string;
        readState: (path: string) => unknown;
        runtimeDir: string;
      }): boolean;
    };
    const registrationId = "0123456789abcdef0123456789abcdef";
    const packageFullName = "OpenAI.Codex_1.2.3.4_x64__publisher";
    const runtimeDir = "C:\\Users\\test\\.incodex\\runtime\\releases\\0.5.0-releasehash";
    const env = {
      INCODEX_WINDOWS_REGISTRATION_ID: registrationId,
      INCODEX_WINDOWS_PACKAGE_FULL_NAME: packageFullName,
      INCODEX_WINDOWS_STATE_PATH: "C:\\Users\\test\\.incodex\\windows-install.json",
    };
    const valid = {
      schemaVersion: 1,
      desired: "enabled",
      phase: "enabled-observed",
      registrationId,
      packageFullName,
      runtimeRelease: "0.5.0-releasehash",
    };
    const invalidStates = [
      null,
      { ...valid, desired: "disabled" },
      { ...valid, phase: "disable-requested" },
      { ...valid, registrationId: "fedcba9876543210fedcba9876543210" },
      { ...valid, packageFullName: "Other.Package_1.2.3.4_x64__publisher" },
      { ...valid, runtimeRelease: "0.5.0-otherhash" },
    ];

    for (const state of invalidStates) {
      const loaded: string[] = [];
      expect(
        bootstrap.attachWindowsRuntime({
          env: { ...env },
          load: (path: string) => loaded.push(path),
          processType: "browser",
          readState: () => state,
          runtimeDir,
        }),
      ).toBe(false);
      expect(loaded).toEqual([]);
    }
  });

  test("does nothing outside an owned Owl browser process", () => {
    const bootstrap = require(bootstrapPath) as {
      attachWindowsRuntime(options: {
        env: Record<string, string | undefined>;
        load: (path: string) => void;
        processType: string;
        readState?: (path: string) => unknown;
        runtimeDir?: string;
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
