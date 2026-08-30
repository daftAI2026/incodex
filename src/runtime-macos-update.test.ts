import { afterEach, describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  chmodSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

const update = require("./runtime/incodex-macos-update.cts");

const roots: string[] = [];

function scratch(): string {
  const root = join(
    import.meta.dir,
    `.tmp-macos-update-${process.pid}-${Date.now()}-${roots.length}`,
  );
  roots.push(root);
  mkdirSync(root, { recursive: true });
  return root;
}

function fixture() {
  const home = scratch();
  const userRoot = join(home, ".incodex");
  const appPath = join(home, "Applications", "ChatGPT.app");
  const execPath = join(appPath, "Contents", "MacOS", "ChatGPT");
  const helperPath = join(userRoot, "helpers", "macos-update", "abc", "incodex");
  const registrationPath = join(userRoot, "macos-update", "registration.json");
  mkdirSync(join(appPath, "Contents", "MacOS"), { recursive: true });
  mkdirSync(join(userRoot, "helpers", "macos-update", "abc"), { recursive: true });
  mkdirSync(join(userRoot, "macos-update"), { recursive: true });
  writeFileSync(execPath, "app");
  writeFileSync(helperPath, "helper");
  chmodSync(helperPath, 0o700);
  const helperSha256 = createHash("sha256").update("helper").digest("hex");
  writeFileSync(
    registrationPath,
    `${JSON.stringify({
      schemaVersion: 1,
      installId: "install-epoch-a",
      appPath,
      helperPath,
      helperSha256,
    })}\n`,
    { mode: 0o600 },
  );
  return { appPath, execPath, helperPath, home, registrationPath, userRoot };
}

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

describe("macOS update coordinator launch", () => {
  test("starts the verified helper with one install epoch and parent identity", () => {
    const f = fixture();
    const calls: any[] = [];
    const child = {
      unrefCalled: false,
      unref() {
        this.unrefCalled = true;
      },
    };

    const launched = update.spawnCoordinator({
      platform: "darwin",
      userRoot: f.userRoot,
      execPath: f.execPath,
      pid: 42,
      spawnProcess(command: string, args: string[], options: unknown) {
        calls.push({ command, args, options });
        return child;
      },
    });

    expect(launched).toBe(true);
    expect(calls).toHaveLength(1);
    expect(calls[0].command).toBe(f.helperPath);
    expect(calls[0].args).toEqual([]);
    expect(calls[0].options.env.INCODEX_MACOS_UPDATE_WORKER).toBe("1");
    expect(calls[0].options.env.INCODEX_MACOS_UPDATE_INSTALL_ID).toBe("install-epoch-a");
    expect(calls[0].options.env.INCODEX_MACOS_UPDATE_PARENT_PID).toBe("42");
    expect(calls[0].options.detached).toBe(true);
    expect(calls[0].options.stdio).toBe("ignore");
    expect(child.unrefCalled).toBe(true);
  });

  test("rejects helper hash mismatch, symlinks, and a foreign app path", () => {
    const badHash = fixture();
    writeFileSync(badHash.helperPath, "changed");
    expect(
      update.spawnCoordinator({
        platform: "darwin",
        userRoot: badHash.userRoot,
        execPath: badHash.execPath,
        pid: 1,
        spawnProcess() {
          throw new Error("must not spawn");
        },
      }),
    ).toBe(false);

    const linked = fixture();
    const real = `${linked.helperPath}.real`;
    writeFileSync(real, "helper");
    rmSync(linked.helperPath);
    symlinkSync(real, linked.helperPath);
    expect(
      update.spawnCoordinator({
        platform: "darwin",
        userRoot: linked.userRoot,
        execPath: linked.execPath,
        pid: 1,
        spawnProcess() {
          throw new Error("must not spawn");
        },
      }),
    ).toBe(false);

    const foreign = fixture();
    const body = JSON.parse(readFileSync(foreign.registrationPath, "utf8"));
    body.appPath = join(foreign.home, "Other.app");
    writeFileSync(foreign.registrationPath, `${JSON.stringify(body)}\n`);
    expect(
      update.spawnCoordinator({
        platform: "darwin",
        userRoot: foreign.userRoot,
        execPath: foreign.execPath,
        pid: 1,
        spawnProcess() {
          throw new Error("must not spawn");
        },
      }),
    ).toBe(false);
  });

  test("is inert outside macOS", () => {
    const f = fixture();
    expect(
      update.spawnCoordinator({
        platform: "win32",
        userRoot: f.userRoot,
        execPath: f.execPath,
        pid: 1,
        spawnProcess() {
          throw new Error("must not spawn");
        },
      }),
    ).toBe(false);
  });
});
