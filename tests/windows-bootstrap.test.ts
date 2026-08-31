import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { join, win32 } from "node:path";
import { readFileSync } from "node:fs";

const bootstrapPath = join(import.meta.dir, "../crates/incodex-cli/assets/incodex-windows-bootstrap.cjs");

describe("Windows Runtime bootstrap", () => {
  test("reads one bounded message without waiting for a named-pipe EOF", () => {
    const bootstrap = require(bootstrapPath) as {
      readActivationEnvironment(
        pipeName: string,
        io: {
          openSync: (path: string, flags: string) => number;
          writeSync: (descriptor: number, value: string) => number;
          readSync: (
            descriptor: number,
            buffer: Buffer,
            offset: number,
            length: number,
            position: null,
          ) => number;
          closeSync: (descriptor: number) => void;
        },
      ): unknown;
    };
    const response = Buffer.from(
      JSON.stringify({ mode: "runtime", environment: { CODEX_HOME: "C:\\isolated" } }),
    );
    const writes: string[] = [];
    const closes: number[] = [];
    let reads = 0;

    expect(
      bootstrap.readActivationEnvironment("\\\\.\\pipe\\Incodex-test", {
        openSync: () => 42,
        writeSync: (_descriptor, value) => {
          writes.push(value);
          return Buffer.byteLength(value);
        },
        readSync: (_descriptor, buffer, offset, length) => {
          reads++;
          response.copy(buffer, offset, 0, Math.min(response.length, length));
          return response.length;
        },
        closeSync: (descriptor) => closes.push(descriptor),
      }),
    ).toEqual({ mode: "runtime", environment: { CODEX_HOME: "C:\\isolated" } });
    expect(writes).toEqual(["environment\n"]);
    expect(reads).toBe(1);
    expect(closes).toEqual([42]);
  });

  test("claims one isolated profile environment before loading its selected Runtime release", () => {
    const bootstrap = require(bootstrapPath) as {
      attachWindowsRuntime(options: {
        argv: string[];
        env: Record<string, string | undefined>;
        load: (path: string) => void;
        onElectronLoaded: (callback: () => void) => void;
        processType: string;
        readActivationEnvironment: (pipeName: string) => unknown;
        readState: (path: string) => unknown;
        runtimeDir: string;
      }): boolean;
    };
    const userDataDir = "C:\\Users\\test\\.incodex\\sessions\\s-one\\chromium";
    const token = createHash("sha256").update(userDataDir, "utf8").digest("hex").slice(0, 32);
    const registrationId = "0123456789abcdef0123456789abcdef";
    const packageFullName = "OpenAI.Codex_1.2.3.4_x64__publisher";
    const statePath = "C:\\Users\\test\\.incodex\\windows-install.json";
    const runtimeDir = "C:\\Users\\test\\.incodex\\runtime";
    const env: Record<string, string | undefined> = {};
    const loaded: string[] = [];
    const pipes: string[] = [];
    let electronLoaded: (() => void) | undefined;

    expect(
      bootstrap.attachWindowsRuntime({
        argv: [`--user-data-dir=${userDataDir}`],
        env,
        load: (path: string) => loaded.push(path),
        onElectronLoaded: (callback) => {
          electronLoaded = callback;
        },
        processType: "browser",
        readActivationEnvironment(pipeName: string) {
          pipes.push(pipeName);
          return {
            mode: "runtime",
            environment: {
              CODEX_HOME: "C:\\Users\\test\\.incodex\\sessions\\s-one\\codex-home",
              INCODEX_INCOGNITO: "1",
              INCODEX_WINDOWS_REGISTRATION_ID: registrationId,
              INCODEX_WINDOWS_PACKAGE_FULL_NAME: packageFullName,
              INCODEX_WINDOWS_STATE_PATH: statePath,
            },
          };
        },
        readState: () => ({
          schemaVersion: 1,
          desired: "enabled",
          phase: "enabled-observed",
          registrationId,
          packageFullName,
          runtimeRelease: "0.5.0-releasehash",
        }),
        runtimeDir,
      }),
    ).toBe(true);
    expect(pipes).toEqual([
      `\\\\.\\pipe\\Incodex-Activation-Environment-${token}`,
    ]);
    expect(env.CODEX_HOME).toEndWith("sessions\\s-one\\codex-home");
    expect(env.INCODEX_INCOGNITO).toBe("1");
    expect(loaded).toEqual([]);
    expect(electronLoaded).toBeFunction();
    electronLoaded?.();
    expect(loaded).toEqual([
      win32.join(runtimeDir, "releases", "0.5.0-releasehash", "incodex-main.cjs"),
    ]);
  });

  test("an isolated CDP profile receives its environment without loading a second Runtime", () => {
    const bootstrap = require(bootstrapPath) as {
      attachWindowsRuntime(options: {
        argv: string[];
        env: Record<string, string | undefined>;
        load: (path: string) => void;
        onElectronLoaded: (callback: () => void) => void;
        processType: string;
        readActivationEnvironment: () => unknown;
      }): boolean;
    };
    const loaded: string[] = [];
    const env: Record<string, string | undefined> = {};

    expect(
      bootstrap.attachWindowsRuntime({
        argv: ["--user-data-dir=C:\\Users\\test\\.incodex\\sessions\\s-cdp\\chromium"],
        env,
        load: (path: string) => loaded.push(path),
        onElectronLoaded: (callback: () => void) => callback(),
        processType: "browser",
        readActivationEnvironment: () => ({
          mode: "cdp",
          environment: { CODEX_HOME: "C:\\isolated" },
        }),
      }),
    ).toBe(true);
    expect(env.CODEX_HOME).toBe("C:\\isolated");
    expect(loaded).toEqual([]);
  });

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
    const runtimeRoot = "C:\\Users\\test\\.incodex\\runtime";
    const env: Record<string, string | undefined> = {
      INCODEX_WINDOWS_REGISTRATION_ID: registrationId,
      INCODEX_WINDOWS_PACKAGE_FULL_NAME: packageFullName,
      INCODEX_WINDOWS_STATE_PATH: "C:\\Users\\test\\.incodex\\windows-install.json",
    };
    const loaded: string[] = [];
    let electronLoaded: (() => void) | undefined;
    const options = {
      env,
      load: (path: string) => loaded.push(path),
      onElectronLoaded: (callback: () => void) => {
        electronLoaded = callback;
      },
      processType: "browser",
      readState: () => ({
        schemaVersion: 1,
        desired: "enabled",
        phase: "enabled-unobserved",
        registrationId,
        packageFullName,
        runtimeRelease: "0.6.0-newreleasehash",
      }),
      runtimeDir: runtimeRoot,
    };

    expect(bootstrap.attachWindowsRuntime(options)).toBe(true);
    expect(bootstrap.attachWindowsRuntime(options)).toBe(false);
    expect(loaded).toEqual([]);
    expect(electronLoaded).toBeFunction();
    electronLoaded?.();
    expect(loaded).toHaveLength(1);
    expect(loaded).toEqual([
      win32.join(runtimeRoot, "releases", "0.6.0-newreleasehash", "incodex-main.cjs"),
    ]);
    expect(readFileSync(bootstrapPath, "utf8")).toContain(
      'load(path.win32.join(runtimeDir, "incodex-main.cjs"));',
    );
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
      { ...valid, runtimeRelease: ".." },
      { ...valid, runtimeRelease: "0.5.0\\otherhash" },
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
