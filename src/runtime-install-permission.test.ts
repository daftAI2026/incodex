/**
 * [INPUT]: 依赖真实 loader 源码、隔离 Runtime catalog 与 Electron 启动夹具。
 * [OUTPUT]: 验证安装权限探针不会载入官方 main，并保留普通启动顺序。
 * [POS]: loader 安装前置权限模式的真实 bootstrap 回归，不操作宿主 App。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import * as nodeCrypto from "node:crypto";
import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import * as nodePath from "node:path";
import { join } from "node:path";
import vm from "node:vm";
import { RUNTIME_EXTERNAL_ARTIFACT_NAMES } from "./runtime-manifest.ts";

const PREFLIGHT_ENV = "INCODEX_INTERNAL_PERMISSION_PREFLIGHT";
const ORIGINAL_MAIN = "official-main.cjs";
const RUNTIME_VERSION = "1.0.0";

function sha256(value: string | Buffer): string {
  return createHash("sha256").update(value).digest("hex");
}

function createBundleFixture() {
  const root = mkdtempSync(join(tmpdir(), "incodex-install-permission-"));
  const home = join(root, "home");
  const bundle = join(root, "ChatGPT.app", "Contents", "Resources", "app.asar.unpacked");
  const runtimeRoot = join(home, ".incodex", "runtime");
  const runtimeFiles: Record<string, string> = Object.fromEntries(
    RUNTIME_EXTERNAL_ARTIFACT_NAMES.map((name) => [name, `fixture:${name}`]),
  );
  const fileHashes = Object.fromEntries(
    Object.entries(runtimeFiles).map(([name, bytes]) => [name, sha256(bytes)]),
  );
  const manifest = Buffer.from(`${JSON.stringify({
    runtimeVersion: RUNTIME_VERSION,
    sourceCommit: "",
    files: fileHashes,
  })}\n`);
  const manifestSha256 = sha256(manifest);
  const release = `releases/${RUNTIME_VERSION}-${manifestSha256}`;
  const releaseDir = join(runtimeRoot, release);

  mkdirSync(bundle, { recursive: true });
  mkdirSync(releaseDir, { recursive: true });
  writeFileSync(join(bundle, "package.json"), JSON.stringify({
    __incodex: { originalMain: ORIGINAL_MAIN },
  }));
  writeFileSync(join(bundle, ORIGINAL_MAIN), "// isolated official-main marker\n");
  for (const [name, bytes] of Object.entries(runtimeFiles)) {
    writeFileSync(join(releaseDir, name), bytes);
  }
  writeFileSync(join(releaseDir, "runtime-manifest.json"), manifest);
  writeFileSync(join(runtimeRoot, "current.json"), `${JSON.stringify({
    schemaVersion: 1,
    version: RUNTIME_VERSION,
    sourceCommit: "",
    release,
    manifestSha256,
    files: fileHashes,
  })}\n`);

  return { root, home, bundle, runtimeFile: join(releaseDir, "incodex-main.cjs") };
}

function loadActualBootstrap(options: {
  platform: string;
  env?: Record<string, string>;
  runtime: Record<string, unknown>;
}) {
  const fixture = createBundleFixture();
  const events: string[] = [];
  const runtimeFile = fixture.runtimeFile;
  const originalMain = join(fixture.bundle, ORIGINAL_MAIN);
  const runtimeRequire = (request: string) => {
    if (request === "node:crypto") return nodeCrypto;
    if (request === "node:fs") return nodeFs;
    if (request === "node:path") return nodePath;
    if (request === runtimeFile) {
      events.push("runtime-required");
      return options.runtime;
    }
    if (request === originalMain) {
      events.push("official-main-required");
      return {};
    }
    throw new Error(`unexpected loader require: ${request}`);
  };
  const context = vm.createContext({
    __dirname: fixture.bundle,
    process: {
      platform: options.platform,
      execPath: "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
      env: { HOME: fixture.home, ...options.env },
    },
    require: runtimeRequire,
    console: {
      error: () => events.push("loader-error"),
    },
  });
  const source = readFileSync(join(import.meta.dir, "runtime/incodex-loader.cts"), "utf8");
  const marker = '"__INCODEX_RUNTIME_FILES__"';
  const artifactCatalogSource = source.replace(marker, JSON.stringify(RUNTIME_EXTERNAL_ARTIFACT_NAMES));
  if (artifactCatalogSource === source) throw new Error("loader artifact marker was not embedded");
  new vm.Script(artifactCatalogSource, { filename: "incodex-loader.cts" }).runInContext(context);
  return { ...fixture, events };
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

async function nextTurn() {
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

describe("install permission preflight loader mode", () => {
  test("Darwin awaits the explicit probe and exits without requiring official main", async () => {
    const probe = deferred<void>();
    const events: string[] = [];
    const electron = { app: { quit: () => events.push("app-quit") } };
    const runtime = {
      runInstallPermissionProbe() {
        events.push("probe-started");
        return probe.promise.then(() => {
          events.push("probe-completed");
          electron.app.quit();
        });
      },
      startupGate: Promise.resolve(),
    };
    const fixture = loadActualBootstrap({
      platform: "darwin",
      env: { [PREFLIGHT_ENV]: "1" },
      runtime,
    });

    try {
      expect(fixture.events).not.toContain("official-main-required");
      expect(events).toEqual(["probe-started"]);
      probe.resolve();
      await nextTurn();
      expect(events).toEqual(["probe-started", "probe-completed", "app-quit"]);
      expect(fixture.events).not.toContain("official-main-required");
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("a missing probe export never falls through to official main", async () => {
    const fixture = loadActualBootstrap({
      platform: "darwin",
      env: { [PREFLIGHT_ENV]: "1" },
      runtime: { startupGate: Promise.resolve() },
    });
    try {
      await nextTurn();
      expect(fixture.events).not.toContain("official-main-required");
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("a synchronous probe exception never falls through to official main", async () => {
    const fixture = loadActualBootstrap({
      platform: "darwin",
      env: { [PREFLIGHT_ENV]: "1" },
      runtime: {
        runInstallPermissionProbe() {
          throw new Error("isolated probe failure");
        },
      },
    });
    try {
      await nextTurn();
      expect(fixture.events).not.toContain("official-main-required");
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("a rejected probe promise never falls through to official main", async () => {
    const fixture = loadActualBootstrap({
      platform: "darwin",
      env: { [PREFLIGHT_ENV]: "1" },
      runtime: {
        runInstallPermissionProbe() {
          return Promise.reject(new Error("isolated probe rejection"));
        },
      },
    });
    try {
      await nextTurn();
      expect(fixture.events).not.toContain("official-main-required");
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("ordinary Darwin startup requires official main before awaiting Runtime", async () => {
    const startup = deferred<void>();
    let probeCalls = 0;
    const fixture = loadActualBootstrap({
      platform: "darwin",
      runtime: {
        startupGate: startup.promise,
        runInstallPermissionProbe() {
          probeCalls += 1;
          return Promise.resolve();
        },
      },
    });
    try {
      expect(fixture.events).toContain("official-main-required");
      expect(probeCalls).toBe(0);
      startup.resolve();
      await nextTurn();
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });

  test("the explicit environment flag does not enable the probe on non-Darwin", async () => {
    let probeCalls = 0;
    const fixture = loadActualBootstrap({
      platform: "linux",
      env: { [PREFLIGHT_ENV]: "1" },
      runtime: {
        startupGate: Promise.resolve(),
        runInstallPermissionProbe() {
          probeCalls += 1;
          return Promise.resolve();
        },
      },
    });
    try {
      expect(fixture.events).toContain("official-main-required");
      expect(probeCalls).toBe(0);
      await nextTurn();
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  });
});
