/**
 * [INPUT]: 依赖 macOS 更新交接模块及临时文件系统夹具，模拟 Coordinator 与原生加载边界
 * [OUTPUT]: 提供交接顺序、资产校验和同步启动契约的回归测试
 * [POS]: src 测试层的 Sparkle 安全网，确保更新恢复钩子先布防且不让官方 main 让出事件循环
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
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
  const coordinatorAppPath = join(
    userRoot,
    "helpers",
    "macos-update",
    "abc",
    "Incodex Update Coordinator.app",
  );
  const coordinatorPath = join(
    coordinatorAppPath,
    "Contents",
    "MacOS",
    "incodex-update-coordinator",
  );
  const interposerPath = join(
    userRoot,
    "helpers",
    "macos-update",
    "abc",
    "libincodex-sparkle-interpose.dylib",
  );
  const registrationPath = join(userRoot, "macos-update", "registration.json");
  mkdirSync(join(appPath, "Contents", "MacOS"), { recursive: true });
  mkdirSync(join(userRoot, "helpers", "macos-update", "abc"), { recursive: true });
  mkdirSync(join(coordinatorAppPath, "Contents", "MacOS"), { recursive: true });
  mkdirSync(join(userRoot, "macos-update"), { recursive: true });
  writeFileSync(execPath, "app");
  writeFileSync(helperPath, "helper");
  writeFileSync(coordinatorPath, "coordinator");
  writeFileSync(join(coordinatorAppPath, "Contents", "Info.plist"), "plist");
  writeFileSync(interposerPath, "interposer");
  chmodSync(helperPath, 0o700);
  chmodSync(coordinatorPath, 0o700);
  chmodSync(interposerPath, 0o600);
  const helperSha256 = createHash("sha256").update("helper").digest("hex");
  const coordinatorSha256 = createHash("sha256").update("coordinator").digest("hex");
  const interposerSha256 = createHash("sha256").update("interposer").digest("hex");
  writeFileSync(
    registrationPath,
    `${JSON.stringify({
      schemaVersion: 2,
      installId: "install-epoch-a",
      appPath,
      helperPath,
      helperSha256,
      coordinatorAppPath,
      coordinatorPath,
      coordinatorSha256,
      interposerPath,
      interposerSha256,
    })}\n`,
    { mode: 0o600 },
  );
  return {
    appPath,
    coordinatorAppPath,
    coordinatorPath,
    execPath,
    helperPath,
    home,
    interposerPath,
    registrationPath,
    userRoot,
  };
}

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

describe("macOS seamless update handoff", () => {
  test("arms the interposer synchronously after the verified coordinator is ready", () => {
    const f = fixture();
    const calls: any[] = [];
    const order: string[] = [];
    const child = {
      unrefCalled: false,
      unref() {
        this.unrefCalled = true;
      },
    };

    const armed = update.prepareUpdateHandoff({
      platform: "darwin",
      userRoot: f.userRoot,
      execPath: f.execPath,
      pid: 42,
      spawnProcess(command: string, args: string[], options: unknown) {
        order.push("spawn");
        calls.push({ command, args, options });
        return child;
      },
      waitForCoordinator() {
        order.push("ready");
        return true;
      },
      requireSparkle(file: string) {
        order.push("sparkle");
        expect(file).toEndWith("/Contents/Resources/native/sparkle.node");
      },
      loadInterposer(file: string) {
        order.push("interposer");
        expect(file).toBe(f.interposerPath);
      },
    });

    expect(armed).toBe(true);
    expect(order).toEqual(["spawn", "ready", "sparkle", "interposer"]);
    expect(calls).toHaveLength(1);
    expect(calls[0].command).toBe(f.coordinatorPath);
    expect(calls[0].args).toEqual([]);
    expect(calls[0].options.env.INCODEX_MACOS_UPDATE_COORDINATOR).toBe("1");
    expect(calls[0].options.env.INCODEX_MACOS_UPDATE_INSTALL_ID).toBe("install-epoch-a");
    expect(calls[0].options.env.INCODEX_MACOS_UPDATE_HOST_PID).toBe("42");
    expect(calls[0].options.env.INCODEX_MACOS_UPDATE_HELPER_PATH).toBe(f.helperPath);
    expect(calls[0].options.env.INCODEX_MACOS_UPDATE_HELPER_SHA256).toMatch(/^[0-9a-f]{64}$/);
    expect(calls[0].options.env.INCODEX_MACOS_UPDATE_HANDOFF_ID).toMatch(/^[0-9a-f]{32}$/);
    expect(calls[0].options.env.INCODEX_MACOS_UPDATE_PENDING_PATH).toBe(
      join(f.userRoot, "macos-update", "pending.json"),
    );
    expect(calls[0].options.detached).toBe(true);
    expect(calls[0].options.stdio).toBe("ignore");
    expect(child.unrefCalled).toBe(true);
  });

  test("rejects helper hash mismatch, symlinks, and a foreign app path", () => {
    const badHash = fixture();
    writeFileSync(badHash.helperPath, "changed");
    expect(
      update.prepareUpdateHandoff({
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
      update.prepareUpdateHandoff({
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
      update.prepareUpdateHandoff({
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
      update.prepareUpdateHandoff({
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
