/**
 * [INPUT]: 依赖权限 Swift 源码、隔离夹具和原生构建工具。
 * [OUTPUT]: 验证权限协议、安装上下文与默认窗口呈现门。
 * [POS]: 原生卡协议回归；无窗夹具保留真实 gate，可见测试须显式 opt-in。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { afterAll, beforeAll, expect, setDefaultTimeout, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from "node:child_process";
import { join } from "node:path";
import { tmpdir } from "node:os";

const repositoryRoot = join(import.meta.dir, "..");
const nativeRoot = join(repositoryRoot, "native", "macos");
const viewSource = join(nativeRoot, "permission-views.swift");
const settingsSource = join(nativeRoot, "permission-host-settings.swift");
const presenterSource = join(nativeRoot, "permission-host-presenter.swift");
const flightSource = join(nativeRoot, "permission-host-flight.swift");
const hostSource = join(nativeRoot, "permission-host.swift");

setDefaultTimeout(120_000);

function nonce(): string {
  return "0123456789abcdef0123456789abcdef";
}

function line(message: Record<string, unknown>): string {
  return `${JSON.stringify(message)}\n`;
}

function buildHost(): { executable: string; directory: string } {
  const directory = mkdtempSync(join(tmpdir(), "incodex-native-host-test-"));
  const executable = join(directory, "permission-host");
  const result = spawnSync("xcrun", [
    "swiftc",
    "-target",
    `${process.arch === "arm64" ? "arm64" : "x86_64"}-apple-macos12.0`,
    "-D",
    "INCODEX_PERMISSION_HOST_TESTING",
    "-module-name",
    "IncodexPermissionHostTest",
    viewSource, join(nativeRoot, "permission-initial.swift"),
    settingsSource,
    flightSource,
    join(nativeRoot, "permission-host-presenter-geometry.swift"),
    join(nativeRoot, "permission-host-presenter-views.swift"),
    presenterSource,
    hostSource,
    "-o",
    executable,
  ], {
    cwd: repositoryRoot,
    encoding: "utf8",
    timeout: 90_000,
  });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
  expect(result.status, output || "native host executable failed to build").toBe(0);
  return { executable, directory };
}

function buildNoWindowPresenterHost(directory: string): string {
  const fixture = join(directory, "no-window-presenter.swift");
  const executable = join(directory, "no-window-presenter-host");
  writeFileSync(fixture, `import AppKit
@MainActor final class PermissionHostPresenter {
 init(copy: NSDictionary, layoutDirection: String, onEvent: @escaping (String) -> Void, onError: @escaping (String) -> Void) {}
 func note(_ value: String) { FileHandle.standardError.write(Data((value + "\\n").utf8)) }
 func present() -> Bool { note("present-without-work-window"); return true }
 func setState(_ state: String, message: String?) {}
 func configurePermissions(_ statuses: [String: String]) {}
 func updatePermissions(_ statuses: [String: String]) {}
 func setPermissionState(_ state: String, permission: String, message: String?) {}
 func close() { note("close-without-work-window") }
}`);
  const result = spawnSync("xcrun", [
    "swiftc",
    "-target",
    `${process.arch === "arm64" ? "arm64" : "x86_64"}-apple-macos12.0`,
    "-D",
    "INCODEX_PERMISSION_HOST_TESTING",
    "-module-name",
    "IncodexPermissionHostNoWindowTest",
    hostSource,
    fixture,
    "-o",
    executable,
  ], {
    cwd: repositoryRoot,
    encoding: "utf8",
    timeout: 90_000,
  });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
  expect(result.status, output || "no-window protocol fixture failed to build").toBe(0);
  return executable;
}

type HostResult = {
  code: number | null;
  signal: NodeJS.Signals | null;
  stdout: string;
  stderr: string;
};

function runHost(
  executable: string,
  write: (stdin: ChildProcessWithoutNullStreams["stdin"]) => void,
  timeoutMs = 3_000,
  extraEnv: Record<string, string> = {},
): Promise<HostResult> {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, ["--nonce", nonce()], {
      cwd: repositoryRoot,
      env: {
        ...process.env,
        // The behavior tests must never create an AppKit window, even if the
        // developer happens to have the official target in front.
        INCODEX_PERMISSION_HOST_DISABLE_PRESENTATION: "1",
        ...extraEnv,
      },
      stdio: ["pipe", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      child.kill("SIGKILL");
      reject(new Error(`native host did not exit within ${timeoutMs}ms\nstderr=${stderr}`));
    }, timeoutMs);
    child.stdout.setEncoding("utf8");
    child.stdin.on("error", () => {}); // A rejected oversized input may close the pipe mid-write.
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => { stdout += chunk; });
    child.stderr.on("data", (chunk: string) => { stderr += chunk; });
    child.once("error", (error) => {
      clearTimeout(timer);
      if (!settled) { settled = true; reject(error); }
    });
    child.once("close", (code, signal) => {
      clearTimeout(timer);
      if (settled) return;
      settled = true;
      resolve({ code, signal, stdout, stderr });
    });
    write(child.stdin);
  });
}

let host: { executable: string; directory: string } | undefined;
let noWindowPresenterHost: string | undefined;

beforeAll(() => {
  if (process.platform === "darwin") {
    host = buildHost();
    noWindowPresenterHost = buildNoWindowPresenterHost(host.directory);
  }
});

afterAll(() => {
  if (host) rmSync(host.directory, { recursive: true, force: true });
});

test.skipIf(process.platform !== "darwin")("invalid launch arguments exit nonzero before AppKit starts", () => {
  for (const args of [[], ["--nonce", "not-hex"], ["--nonce", "g".repeat(32)]]) {
    const result = spawnSync(host!.executable, args, { encoding: "utf8", timeout: 5_000 });
    expect(result.status).toBe(2);
    expect(result.stdout).toBe("");
    expect(result.stderr).toContain("--nonce HEX32");
  }
});

test.skipIf(process.platform !== "darwin")("locale request reads the target bundle without presenting a window", async () => {
  const bundle = join(host!.directory, "LocaleFixture.app");
  mkdirSync(join(bundle, "Contents", "Resources", "fr.lproj"), { recursive: true });
  writeFileSync(join(bundle, "Contents", "Info.plist"), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.openai.codex</string>
<key>CFBundleDevelopmentRegion</key><string>fr</string>
<key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>`);
  const result = await runHost(host!.executable, stdin => {
    stdin.end(
      line({ nonce: nonce(), type: "app-locale-request" }) +
      line({ nonce: nonce(), type: "close" }),
    );
  }, 3_000, { INCODEX_PERMISSION_HOST_TARGET_BUNDLE: bundle });
  const events = result.stdout.trim().split("\n").map(value => JSON.parse(value));
  expect(events).toEqual([{ nonce: nonce(), type: "app-locale", locale: "fr" }]);
  expect(result.code).toBe(0);
  expect(result.stdout).not.toContain('"type":"ready"');
});

test.skipIf(process.platform !== "darwin")("the presentation deadline also bounds waiting for configure", async () => {
  const result = await runHost(host!.executable, () => {}, 1_500);
  expect(result.stdout).toContain('"type":"error"');
  expect(result.stdout).toContain("timed out");
  expect(result.stdout).not.toContain('"type":"ready"');
});

test.skipIf(process.platform !== "darwin")("EOF cannot silently accept a truncated protocol line", async () => {
  const result = await runHost(host!.executable, stdin => stdin.end('{"nonce":'));
  expect(result.stdout).toContain('"type":"error"');
  expect(result.stdout).not.toContain('"type":"ready"');
});

test.skipIf(process.platform !== "darwin")("state cannot bypass the presentation readiness gate", async () => {
  const result = await runHost(host!.executable, stdin => stdin.end(
    line({ nonce: nonce(), type: "configure", copy: { title: "test" }, layoutDirection: "leftToRight" }) +
    line({ nonce: nonce(), type: "state", state: "granted" }),
  ));
  expect(result.stdout).toContain('"type":"error"');
  expect(result.stdout).not.toContain('"type":"ready"');
});

test.skipIf(process.platform !== "darwin")("a blocking presenter cannot acknowledge ready after its deadline", async () => {
  const fixture = join(host!.directory, "slow-presenter.swift");
  const executable = join(host!.directory, "slow-host");
  writeFileSync(fixture, `import AppKit
@MainActor final class PermissionHostPresenter {
 init(copy: NSDictionary, layoutDirection: String, onEvent: @escaping (String) -> Void, onError: @escaping (String) -> Void) {}
 func present() -> Bool {
  FileHandle.standardError.write(Data("stub-present\\n".utf8))
  Thread.sleep(forTimeInterval: 0.3)
  return true
 }
 func setState(_ state: String, message: String?) {}
 func configurePermissions(_ statuses: [String: String]) {}
 func updatePermissions(_ statuses: [String: String]) {}
 func setPermissionState(_ state: String, permission: String, message: String?) {}
 func close() { FileHandle.standardError.write(Data("stub-closed\\n".utf8)) }
}`);
  const built = spawnSync("xcrun", ["swiftc", "-O", "-D", "INCODEX_PERMISSION_HOST_TESTING", "-D", "INCODEX_PERMISSION_HOST_STUB", hostSource, fixture, "-o", executable], { encoding: "utf8", timeout: 60_000 });
  expect(built.status, built.stderr).toBe(0);
  const result = await runHost(executable, stdin => stdin.write(line({ nonce: nonce(), type: "configure", copy: {}, layoutDirection: "leftToRight" })));
  expect(result.stderr).toContain("stub-present");
  expect(result.stderr).toContain("stub-closed");
  expect(result.stdout).toContain("timed out");
  expect(result.stdout).not.toContain('"type":"ready"');
});

test.skipIf(process.platform !== "darwin")("dual-permission configure rejects missing, invented and non-status fields", async () => {
  for (const permissions of [
    { accessibility: "granted" },
    { accessibility: "granted", screen: "granted", microphone: "granted" },
    { accessibility: "granted", screen: "capture" },
    { accessibility: true, screen: "unknown" },
  ]) {
    const result = await runHost(host!.executable, stdin => stdin.end(line({
      nonce: nonce(), type: "configure", copy: {}, layoutDirection: "leftToRight", permissions,
    })));
    expect(result.stdout).toContain('"type":"error"');
    expect(result.stdout).not.toContain('"type":"ready"');
  }
});

test.skipIf(process.platform !== "darwin")("installation context reaches ready without a work window and exits on close or EOF", async () => {
  const bundle = join(host!.directory, "InstallationLocaleFixture.app");
  mkdirSync(join(bundle, "Contents", "Resources", "fr.lproj"), { recursive: true });
  writeFileSync(join(bundle, "Contents", "Info.plist"), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.openai.codex</string>
<key>CFBundleDevelopmentRegion</key><string>fr</string>
<key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>`);
  const configure = line({
    nonce: nonce(),
    type: "configure",
    copy: { title: "Installation", accessibilityTitle: "Accessibility", screenTitle: "Screen" },
    layoutDirection: "leftToRight",
    permissions: { accessibility: "not-granted", screen: "not-granted" },
    presentationContext: "installation",
  });
  const initialPage = line({ nonce: nonce(), type: "app-locale-request" }) + configure;
  const expectedEvents = [
    { nonce: nonce(), type: "app-locale", locale: "fr" },
    { nonce: nonce(), type: "ready" },
  ];

  const closed = await runHost(noWindowPresenterHost!, stdin => {
    stdin.end(initialPage + line({ nonce: nonce(), type: "close" }));
  }, 3_000, { INCODEX_PERMISSION_HOST_TARGET_BUNDLE: bundle });
  expect(closed.code).toBe(0);
  expect(closed.stdout.trim().split("\n").map(value => JSON.parse(value))).toEqual(expectedEvents);
  expect(closed.stderr).toContain("present-without-work-window\nclose-without-work-window");

  const eof = await runHost(noWindowPresenterHost!, stdin => {
    stdin.end(initialPage);
  }, 3_000, { INCODEX_PERMISSION_HOST_TARGET_BUNDLE: bundle });
  expect(eof.code).toBe(0);
  expect(eof.stdout.trim().split("\n").map(value => JSON.parse(value))).toEqual(expectedEvents);
  expect(eof.stderr).toContain("present-without-work-window\nclose-without-work-window");
});

test.skipIf(process.platform !== "darwin")("default context still needs the official foreground work window", async () => {
  const bundle = join(host!.directory, "DefaultLocaleFixture.app");
  mkdirSync(join(bundle, "Contents", "Resources", "fr.lproj"), { recursive: true });
  writeFileSync(join(bundle, "Contents", "Info.plist"), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.openai.codex</string>
<key>CFBundleDevelopmentRegion</key><string>fr</string>
<key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>`);
  const result = await runHost(noWindowPresenterHost!, stdin => {
    stdin.end(
      line({ nonce: nonce(), type: "app-locale-request" }) +
      line({
        nonce: nonce(),
        type: "configure",
        copy: { title: "Installation", accessibilityTitle: "Accessibility", screenTitle: "Screen" },
        layoutDirection: "leftToRight",
        permissions: { accessibility: "not-granted", screen: "not-granted" },
      }) +
      line({ nonce: nonce(), type: "close" }),
    );
  }, 3_000, { INCODEX_PERMISSION_HOST_TARGET_BUNDLE: bundle });
  expect(result.code).toBe(0);
  expect(result.stdout.trim().split("\n").map(value => JSON.parse(value))).toEqual([
    { nonce: nonce(), type: "app-locale", locale: "fr" },
  ]);
  expect(result.stdout).not.toContain('"type":"ready"');
  expect(result.stderr).not.toContain("present-without-work-window");
});

test.skipIf(process.platform !== "darwin")("rejects an unsupported presentation context", async () => {
  const result = await runHost(noWindowPresenterHost!, stdin => stdin.end(line({
    nonce: nonce(),
    type: "configure",
    copy: { title: "Installation", accessibilityTitle: "Accessibility", screenTitle: "Screen" },
    layoutDirection: "leftToRight",
    permissions: { accessibility: "not-granted", screen: "not-granted" },
    presentationContext: "unknown-context",
  })));
  expect(result.code).toBe(0);
  expect(result.stdout).toContain('"type":"error"');
  expect(result.stdout).not.toContain('"type":"ready"');
  expect(result.stderr).not.toContain("present-without-work-window");
});

test.skipIf(process.platform !== "darwin")("dual-permission wire routes snapshots and row states through a no-window presenter", async () => {
  const fixture = join(host!.directory, "dual-presenter.swift");
  const executable = join(host!.directory, "dual-host");
  writeFileSync(fixture, `import AppKit
@MainActor final class PermissionHostPresenter {
 init(copy: NSDictionary, layoutDirection: String, onEvent: @escaping (String) -> Void, onError: @escaping (String) -> Void) {}
 func note(_ value: String) { FileHandle.standardError.write(Data((value + "\\n").utf8)) }
 func present() -> Bool { note("present-without-window"); return true }
 func setState(_ state: String, message: String?) { note("global:" + state) }
 func configurePermissions(_ statuses: [String: String]) { note("configure:" + statuses["screen"]!) }
 func updatePermissions(_ statuses: [String: String]) { note("snapshot:" + statuses["screen"]!) }
 func setPermissionState(_ state: String, permission: String, message: String?) { note(permission + ":" + state) }
 func close() { note("close-without-window") }
}`);
  const built = spawnSync("xcrun", ["swiftc", "-O", "-D", "INCODEX_PERMISSION_HOST_TESTING", "-D", "INCODEX_PERMISSION_HOST_STUB", hostSource, fixture, "-o", executable], { encoding: "utf8", timeout: 60_000 });
  expect(built.status, built.stderr).toBe(0);
  const result = await runHost(executable, stdin => stdin.end(
    line({ nonce: nonce(), type: "configure", copy: {}, layoutDirection: "leftToRight", permissions: { accessibility: "granted", screen: "not-granted" } }) +
    line({ nonce: nonce(), type: "permissions", permissions: { accessibility: "not-granted", screen: "granted" } }) +
    line({ nonce: nonce(), type: "state", state: "awaiting-user", permission: "screen" }) +
    line({ nonce: nonce(), type: "state", state: "granted" }) +
    line({ nonce: nonce(), type: "close" }),
  ));
  expect(result.stdout).toContain('"type":"ready"');
  expect(result.stdout).not.toContain('"type":"error"');
  expect(result.stderr).toContain("configure:not-granted\npresent-without-window\nsnapshot:granted\nscreen:awaiting-user\nglobal:granted\nclose-without-window");
});

test.skipIf(process.platform !== "darwin")(
  "native host entry references the single external presenter and compiles it",
  () => {
    const entry = readFileSync(hostSource, "utf8");
    expect(entry).toContain("PermissionHostPresenter(");
    expect(entry).not.toMatch(/class\s+PermissionHostPresenter/);
    expect(readFileSync(presenterSource, "utf8")).toContain("public final class PermissionHostPresenter");
  },
);

test.skipIf(process.platform !== "darwin")(
  "rejects a message with the wrong nonce and exits without presenting",
  async () => {
    const result = await runHost(host!.executable, (stdin) => {
      stdin.end(line({ nonce: "ffffffffffffffffffffffffffffffff", type: "close" }));
    });
    expect(result.stdout).toContain('"type":"error"');
    expect(result.stdout).not.toContain('"type":"ready"');
    expect(result.code).toBe(0);
  },
);

test.skipIf(process.platform !== "darwin")(
  "rejects state before configure and never creates a presenter window",
  async () => {
    const result = await runHost(host!.executable, (stdin) => {
      stdin.end(line({ nonce: nonce(), type: "state", state: "awaiting-user" }));
    });
    expect(result.stdout).toContain('"type":"error"');
    expect(result.stdout).not.toContain('"type":"ready"');
  },
);

test.skipIf(process.platform !== "darwin")(
  "rejects a line larger than 64KiB before JSON dispatch",
  async () => {
    const oversized = `${"x".repeat(64 * 1024 + 1)}\n`;
    const result = await runHost(host!.executable, (stdin) => stdin.end(oversized));
    expect(result.stdout).toContain('"type":"error"');
    expect(result.stdout).not.toContain('"type":"ready"');
  },
);

test.skipIf(process.platform !== "darwin")(
  "EOF and an explicit close both cleanly terminate without ready",
  async () => {
    const eof = await runHost(host!.executable, (stdin) => stdin.end());
    expect(eof.stdout).toBe("");
    expect(eof.code).toBe(0);

    const closed = await runHost(host!.executable, (stdin) => {
      stdin.end(line({ nonce: nonce(), type: "close" }));
    });
    expect(closed.stdout).toBe("");
    expect(closed.code).toBe(0);
  },
);

test.skipIf(process.platform !== "darwin")(
  "a configured but unavailable presentation reaches its deadline without ready",
  async () => {
    const result = await runHost(host!.executable, (stdin) => {
      stdin.write(line({
        nonce: nonce(),
        type: "configure",
        copy: { title: "test", body: "test" },
        layoutDirection: "leftToRight",
      }));
      // Keep stdin open: only the bounded presentation deadline may finish
      // this case. The test build shortens that deadline under the explicit
      // INCODEX_PERMISSION_HOST_TESTING compilation flag.
    }, 3_000);
    expect(result.stdout).toContain('"type":"error"');
    expect(result.stdout).toContain("timed out");
    expect(result.stdout).not.toContain('"type":"ready"');
  },
);
