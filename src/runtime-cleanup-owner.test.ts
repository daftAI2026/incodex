import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

type Handler = (...args: any[]) => any;

class EventTargetStub {
  readonly handlers = new Map<string, Handler[]>();

  on(name: string, handler: Handler): this {
    const handlers = this.handlers.get(name) ?? [];
    handlers.push(handler);
    this.handlers.set(name, handlers);
    return this;
  }

  once(name: string, handler: Handler): this {
    const onceHandler: Handler = (...args) => {
      this.remove(name, onceHandler);
      return handler(...args);
    };
    return this.on(name, onceHandler);
  }

  emit(name: string, ...args: any[]): void {
    for (const handler of [...(this.handlers.get(name) ?? [])]) handler(...args);
  }

  private remove(name: string, handler: Handler): void {
    this.handlers.set(
      name,
      (this.handlers.get(name) ?? []).filter((candidate) => candidate !== handler),
    );
  }
}

class AppStub extends EventTargetStub {
  exitCalls = 0;
  quitCalls = 0;

  constructor(private readonly lifecycleEvents: string[]) {
    super();
  }

  getAppPath(): string {
    return "/Applications/ChatGPT.app/Contents/Resources/app.asar";
  }

  isReady(): boolean {
    return true;
  }

  whenReady(): Promise<void> {
    return Promise.resolve();
  }

  focus(): void {}

  exit(code?: number): void {
    this.exitCalls += 1;
    this.lifecycleEvents.push(`app.exit:${code ?? "default"}`);
  }

  quit(): void {
    this.quitCalls += 1;
    this.lifecycleEvents.push("app.quit");
  }
}

class WindowStub extends EventTargetStub {
  readonly id = 1;
  readonly webContents = {
    session: {},
    isDestroyed: () => false,
    getURL: () => "https://chatgpt.com/",
    on: () => {},
    executeJavaScript: async () => undefined,
    sendInputEvent: () => {},
  };

  isDestroyed(): boolean {
    return false;
  }

  isAlwaysOnTop(): boolean {
    return false;
  }

  isFocusable(): boolean {
    return true;
  }

  getParentWindow(): null {
    return null;
  }

  getBounds(): { x: number; y: number; width: number; height: number } {
    return { x: 0, y: 0, width: 1_000, height: 800 };
  }
}

interface RuntimeHarness {
  app: AppStub;
  burnCount: () => number;
  events: string[];
  ipcAction: Handler;
  openWindow: () => WindowStub;
  runtimeOwnedSessionEnv: (session: Record<string, unknown>, sourceBounds: string) => NodeJS.ProcessEnv;
}

async function loadRuntime(cleanupOwner?: string): Promise<RuntimeHarness> {
  const source = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
  const events: string[] = [];
  const app = new AppStub(events);
  const ipcHandlers = new Map<string, Handler>();
  const windows: WindowStub[] = [];
  let burns = 0;
  const electron = {
    app,
    ipcMain: {
      handle: (name: string, handler: Handler) => ipcHandlers.set(name, handler),
    },
    session: { defaultSession: {} },
    BrowserWindow: {
      getAllWindows: () => windows,
      getFocusedWindow: () => null,
    },
  };
  const safeHome = {
    resolveSourceHome: (home: string | undefined, fallback: string) => home || fallback,
    isManagedSessionHome: () => true,
    burnSessionHomeWithOwner: () => {
      burns += 1;
      events.push("session.burn");
      return true;
    },
    writeBurnProof: () => true,
    rotateAndAppendLog: () => {},
    writeReady: () => {},
  };
  const instance = {
    targetIdFromExec: () => "test-target",
    targetStateDir: () => "/tmp/incodex-runtime-cleanup-owner",
    processIdentity: () => ({ processStartIdentity: "runtime-process" }),
    currentOwner: (sessionId: string) => ({ sessionId, token: "owner-token" }),
    acquireOwnerLease: async () => ({ sessionId: "test-session", token: "owner-token" }),
    releaseOwnerLease: async () => {
      events.push("lease.release");
      return true;
    },
    listenForRaise: () => ({ listening: true, once: () => {}, close: () => {} }),
    sessionProcessIdsFromPs: () => [],
  };
  const ipcGuard = {
    navigationOrigin: () => "file://app",
    bindWindowIdentity: () => true,
    snapshotFromEvent: () => ({}),
    authorizeSender: () => ({ ok: true }),
    actionResponse: (requestId: string, response: Record<string, unknown>) => {
      events.push(`ipc.response:${String(response.code)}`);
      return { requestId, ...response };
    },
  };
  const nativeRequire = createRequire(import.meta.url);
  const runtimeRequire = (id: string): unknown => {
    if (id === "electron") return electron;
    if (id === "./incodex-safe-home.cjs") return safeHome;
    if (id === "./incodex-ipc-guard.cjs") return ipcGuard;
    if (id === "./incodex-instance.cjs") return instance;
    if (id === "./incodex-window-kind.cjs") {
      return { isAuxiliarySnapshot: () => false };
    }
    if (id === "./incodex-window-lifecycle.cjs") {
      return nativeRequire("./runtime/incodex-window-lifecycle.cts");
    }
    if (id === "./incodex-codex-mode.cjs") {
      return { createCodexModeReadiness: () => ({ observe: () => {} }) };
    }
    if (id === "./incodex-runtime-load.cjs") {
      return { resolveRuntimeFile: () => "/path/that/does/not/exist" };
    }
    return nativeRequire(id);
  };
  const env: Record<string, string> = {
    CODEX_HOME: "/tmp/incodex-runtime-cleanup-owner/home",
    INCODEX_INCOGNITO: "1",
    INCODEX_SESSION_ID: "test-session",
    INCODEX_SESSION_ROOT: "/tmp/incodex-runtime-cleanup-owner/session",
    INCODEX_SESSION_INO: "10",
    INCODEX_SESSION_DEV: "20",
  };
  if (cleanupOwner !== undefined) env.INCODEX_CLEANUP_OWNER = cleanupOwner;
  const runtimeProcess = {
    env,
    execPath: "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
    pid: process.pid,
    platform: "linux",
  };
  const runtimeModule = { exports: {} as Record<string, unknown> };
  const evaluate = new Function("require", "module", "exports", "process", "__dirname", source);
  evaluate(runtimeRequire, runtimeModule, runtimeModule.exports, runtimeProcess, import.meta.dir);
  await (runtimeModule.exports.startupGate as Promise<void>);

  return {
    app,
    burnCount: () => burns,
    events,
    ipcAction: ipcHandlers.get("incodex-action") as Handler,
    runtimeOwnedSessionEnv: runtimeModule.exports.runtimeOwnedSessionEnv as RuntimeHarness["runtimeOwnedSessionEnv"],
    openWindow: () => {
      const win = new WindowStub();
      windows.push(win);
      app.emit("browser-window-created", {}, win);
      return win;
    },
  };
}

async function exerciseRuntimeBurnPaths(runtime: RuntimeHarness): Promise<unknown> {
  runtime.openWindow().emit("closed");
  runtime.app.emit("window-all-closed");
  runtime.app.emit("before-quit");
  return runtime.ipcAction({}, { requestId: "quit", action: "quit" });
}

describe("Electron session cleanup ownership", () => {
  test("Native-open marker leaves every session burn path to the Native parent", async () => {
    const runtime = await loadRuntime("native");

    const response = await exerciseRuntimeBurnPaths(runtime);

    expect(runtime.burnCount()).toBe(0);
    expect(runtime.app.exitCalls).toBe(1);
    expect(runtime.app.quitCalls).toBe(1);
    expect(response).toEqual({ requestId: "quit", ok: true, code: "OK" });
    expect(runtime.events).toEqual([
      "lease.release",
      "app.exit:0",
      "lease.release",
      "lease.release",
      "app.quit",
      "ipc.response:OK",
    ]);
  });

  test("a Runtime-created child overrides an inherited Native ownership marker", async () => {
    const runtime = await loadRuntime("native");
    const env = runtime.runtimeOwnedSessionEnv(
      {
        home: "/tmp/runtime-owned/home",
        chromium: "/tmp/runtime-owned/chromium",
        root: "/tmp/runtime-owned",
        sessionId: "runtime-child",
        ino: 30,
        dev: 40,
      },
      "0,0,1000,800",
    );

    expect(env.INCODEX_CLEANUP_OWNER).toBe("runtime");
    expect(env.INCODEX_SESSION_ID).toBe("runtime-child");
  });

  test("missing cleanup owner preserves Runtime cleanup for in-app sessions", async () => {
    const runtime = await loadRuntime();

    await exerciseRuntimeBurnPaths(runtime);

    expect(runtime.burnCount()).toBe(3);
  });

  test("unknown cleanup owner fails closed to existing Runtime cleanup", async () => {
    const runtime = await loadRuntime("future-owner");

    runtime.app.emit("before-quit");

    expect(runtime.burnCount()).toBe(1);
  });
});
