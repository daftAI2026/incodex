import { afterAll, beforeAll, expect, setDefaultTimeout, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
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
    viewSource,
    settingsSource,
    flightSource,
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
): Promise<HostResult> {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, ["--nonce", nonce()], {
      cwd: repositoryRoot,
      env: {
        ...process.env,
        // The behavior tests must never create an AppKit window, even if the
        // developer happens to have the official target in front.
        INCODEX_PERMISSION_HOST_DISABLE_PRESENTATION: "1",
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

beforeAll(() => {
  if (process.platform === "darwin") host = buildHost();
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
