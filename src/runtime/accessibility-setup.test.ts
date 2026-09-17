import { describe, expect, test } from "bun:test";
import * as nodeFs from "node:fs";
import { EventEmitter } from "node:events";
import { mkdtempSync, chmodSync, mkdirSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// The generated Runtime is the module shipped to Electron.  The controller is
// deliberately tested through that boundary so the main-process export cannot
// drift away from the artifact that gets loaded by the app.
import * as runtimeMain from "../../dist/incodex-main.cjs";
import { COPY as SUPPORTED_COPY, ACCESSIBILITY_SETUP_COPY, resolveLocale } from "./incognito-copy.ts";

const APP_PATH = "/Applications/ChatGPT.app";
const BUNDLE_ID = "com.openai.codex";
const INSTALL_ID = "11111111-1111-4111-8111-111111111111";

const COPY_VALUES = {
  title: "Accessibility access",
  body: "Allow the installed Codex app in Accessibility settings, then check again.",
  repair: "Repair access",
  later: "Later",
  addedTitle: "Check Accessibility access",
  addedBody: "After adding Codex, check again.",
  checkAgain: "Check again",
  openSettings: "Open Settings",
  errorTitle: "Accessibility repair failed",
  errorBody: "The repair command failed. Open Settings and add Codex manually.",
};

// A callable object also exposes named fields.  This keeps the test independent
// of whether the Runtime passes localized copy as a table or a key resolver.
const COPY = Object.assign(
  (key: string) => COPY_VALUES[key as keyof typeof COPY_VALUES] ?? key,
  COPY_VALUES,
);

type Marker = {
  schemaVersion: number;
  installId: string;
  appPath: string;
  requestedAtMs: number;
  state: string;
  [key: string]: unknown;
};

type Harness = {
  root: string;
  requestPath: string;
  dialog: {
    calls: any[];
    responses: number[];
    showMessageBox: (...args: any[]) => Promise<{ response: number }>;
    showErrorBox: (title: string, content: string) => void;
  };
  shell: {
    opened: string[];
    revealed: string[];
    openExternal: (url: string) => Promise<boolean>;
    showItemInFolder: (file: string) => void;
  };
  spawnCalls: Array<{ file: string; args: string[] }>;
  probes: unknown[];
  controller: any;
  panel: any;
  tick: () => void;
  timerActive: () => boolean;
};

function temporaryRoot(): string {
  return mkdtempSync(join(tmpdir(), "incodex-accessibility-setup-"));
}

function markerPath(root: string, installId = INSTALL_ID): string {
  return join(root, "transactions", installId, "accessibility-setup.json");
}

function writeMarker(requestPath: string, overrides: Partial<Marker> = {}, mode = 0o600): Marker {
  mkdirSync(join(requestPath, ".."), { recursive: true, mode: 0o700 });
  const marker: Marker = {
    schemaVersion: 1,
    installId: INSTALL_ID,
    appPath: APP_PATH,
    requestedAtMs: 1_700_000_000_000,
    state: "pending",
    ...overrides,
  };
  writeFileSync(requestPath, `${JSON.stringify(marker)}\n`, { mode });
  chmodSync(requestPath, mode);
  return marker;
}

function readMarker(requestPath: string): Marker {
  return JSON.parse(readFileSync(requestPath, "utf8")) as Marker;
}

function makeHarness(options: {
  marker?: Partial<Marker>;
  requestPath?: string;
  markerMode?: number;
  probes?: unknown[];
  dialogResponses?: number[];
  spawn?: (file: string, args: string[]) => EventEmitter;
  openExternal?: (url: string) => Promise<boolean>;
  appPath?: string;
  installId?: string;
  writeMarker?: boolean;
  copy?: unknown;
  fs?: any;
} = {}): Harness {
  const root = temporaryRoot();
  const requestPath = options.requestPath ?? markerPath(root, options.installId ?? INSTALL_ID);
  if (options.writeMarker ?? options.requestPath === undefined) {
    writeMarker(requestPath, options.marker, options.markerMode ?? 0o600);
  }

  const dialog = {
    calls: [] as any[],
    responses: [...(options.dialogResponses ?? [1])],
    async showMessageBox(...args: any[]): Promise<{ response: number }> {
      dialog.calls.push(args.at(-1));
      return { response: dialog.responses.shift() ?? 1 };
    },
    showErrorBox(title: string, content: string): void {
      dialog.calls.push({ title, message: content });
    },
  };
  const shell = {
    opened: [] as string[],
    revealed: [] as string[],
    async openExternal(url: string): Promise<boolean> {
      shell.opened.push(url);
      return options.openExternal ? options.openExternal(url) : true;
    },
    showItemInFolder(file: string): void {
      shell.revealed.push(file);
    },
  };
  const spawnCalls: Array<{ file: string; args: string[] }> = [];
  const probes = [...(options.probes ?? [false])];
  const systemPreferences = {
    isTrustedAccessibilityClient: (prompt: boolean): unknown => {
      expect(prompt).toBe(false);
      return probes.shift();
    },
  };
  const spawn =
    options.spawn ??
    ((file: string, args: string[]): EventEmitter => {
      spawnCalls.push({ file, args: [...args] });
      const child = new EventEmitter();
      queueMicrotask(() => child.emit("close", 0));
      return child;
    });

  const createController = (runtimeMain as any).createAccessibilitySetupController;
  if (typeof createController !== "function") {
    throw new Error("dist/incodex-main.cjs does not export createAccessibilitySetupController");
  }

  let polling: (() => void) | null = null;
  let closed = false;
  let closeHandler = () => {};
  const panel = {
    states: [] as string[],
    openedBeforeHandoff: false,
    retryCallbacks: [] as Array<() => unknown>,
    setState(state: string) { this.states.push(state); if (state === "awaiting-user") panel.openedBeforeHandoff = shell.opened.length > 0; },
    close() { closed = true; closeHandler(); },
    isDestroyed: () => closed,
    onClose(fn: () => void) { closeHandler = fn; },
    onRetry(fn: () => unknown) { this.retryCallbacks.push(fn); return () => {}; },
    triggerRetry() { return this.retryCallbacks.at(-1)?.(); },
  };
  const controller = createController({
    app: { getLocale: () => "en-US", isReady: () => true },
    shell,
    dialog,
    systemPreferences,
    spawn,
    fs: options.fs ?? nodeFs,
    requestPath,
    appPath: options.appPath ?? APP_PATH,
    installId: options.installId ?? INSTALL_ID,
    bundleId: BUNDLE_ID,
    platform: "darwin",
    isIncognito: false,
    copy: options.copy ?? COPY,
    now: () => 1_700_000_000_100,
    createSetupWindow: async () => ({
      ...panel,
      choice: dialog.showMessageBox({ message: COPY_VALUES.body }).then(({ response }) => response === 0 ? "repair" : "later"),
    }),
    setInterval: (fn: () => void) => { polling = fn; return 1; },
    clearInterval: () => { polling = null; },
    // Tests exercise activation rechecks without waiting on wall-clock timers.
    pollDelaysMs: [],
    sleep: async () => {},
  });

  return { root, requestPath, dialog, shell, spawnCalls, probes, controller, panel, tick: () => polling?.(), timerActive: () => polling !== null };
}

test("accepts root-owned ASAR package metadata for the default app identity", () => {
  const packagePath = "/Applications/ChatGPT.app/Contents/Resources/app.asar/package.json";
  const fakeFs = {
    lstatSync: (file: string) => {
      expect(file).toBe(packagePath);
      return {
        size: 128,
        uid: 0,
        isSymbolicLink: () => false,
        isFile: () => true,
      };
    },
    readFileSync: (file: string) => {
      expect(file).toBe(packagePath);
      return JSON.stringify({ __incodex: { installId: INSTALL_ID } });
    },
  };
  const readIdentity = (runtimeMain as any).readInstalledRuntimeIdentity;
  expect(typeof readIdentity).toBe("function");
  expect(readIdentity({ getAppPath: () => packagePath.slice(0, -13) }, fakeFs)).toEqual({
    appPath: APP_PATH,
    installId: INSTALL_ID,
  });
});

describe("Accessibility setup controller", () => {
  test("marks a pending request granted silently when the actual host is trusted", async () => {
    const harness = makeHarness({ probes: [true] });

    await harness.controller.run();

    expect(readMarker(harness.requestPath).state).toBe("granted");
    expect(harness.dialog.calls).toHaveLength(0);
    expect(harness.spawnCalls).toHaveLength(0);
    expect(harness.shell.opened).toHaveLength(0);
    expect(harness.shell.revealed).toHaveLength(0);
  });

  test("treats a non-boolean host probe as unknown and never resets", async () => {
    const harness = makeHarness({ probes: [undefined] });

    await harness.controller.run();

    expect(readMarker(harness.requestPath).state).toBe("pending");
    expect(harness.dialog.calls).toHaveLength(0);
    expect(harness.spawnCalls).toHaveLength(0);
    expect(harness.shell.opened).toHaveLength(0);
  });

  test("treats an unbuilt or incomplete copy table as unknown and never prompts", async () => {
    const harness = makeHarness({ probes: [false], copy: {} });

    await harness.controller.run();

    expect(readMarker(harness.requestPath).state).toBe("pending");
    expect(harness.dialog.calls).toHaveLength(0);
    expect(harness.spawnCalls).toHaveLength(0);
  });

  test("records deferred and does not prompt again on an ordinary activation", async () => {
    const harness = makeHarness({ probes: [false, false], dialogResponses: [1] });

    await harness.controller.run();
    await harness.controller.run();

    expect(readMarker(harness.requestPath).state).toBe("deferred");
    expect(harness.dialog.calls).toHaveLength(1);
    expect(harness.spawnCalls).toHaveLength(0);
    expect(harness.shell.opened).toHaveLength(0);
  });

  test("does not reset when the user chooses repair but the second probe is already trusted", async () => {
    const harness = makeHarness({ probes: [false, true], dialogResponses: [0] });

    await harness.controller.run();

    expect(readMarker(harness.requestPath).state).toBe("granted");
    expect(harness.spawnCalls).toHaveLength(0);
    expect(harness.shell.opened).toHaveLength(0);
    expect(harness.shell.revealed).toHaveLength(0);
  });

  test("resets only after explicit repair, opens Settings, and never grants on a false probe", async () => {
    const harness = makeHarness({ probes: [false, false], dialogResponses: [0] });

    await harness.controller.run();

    expect(harness.spawnCalls).toEqual([
      { file: "/usr/bin/tccutil", args: ["reset", "Accessibility", BUNDLE_ID] },
    ]);
    expect(harness.shell.opened).toHaveLength(1);
    expect(harness.shell.opened[0]).toMatch(/^x-apple\.systempreferences:/);
    expect(harness.shell.revealed).toEqual([]);
    expect(readMarker(harness.requestPath).state).toBe("awaiting-user");

    // A later activation may observe a still-false decision, but cannot reset
    // again or show a second native prompt without a fresh install request.
    const second = makeHarness({
      requestPath: harness.requestPath,
      probes: [false],
      dialogResponses: [0],
    });
    await second.controller.run();
    expect(readMarker(harness.requestPath).state).toBe("awaiting-user");
    expect(second.dialog.calls).toHaveLength(0);
    expect(second.spawnCalls).toHaveLength(0);

    // Only a later positive host probe completes the request.
    const granted = makeHarness({ requestPath: harness.requestPath, probes: [true] });
    await granted.controller.run();
    expect(readMarker(harness.requestPath).state).toBe("granted");
  });

  test("persists error and explains a failed reset without opening misleading Settings", async () => {
    const spawn = (): EventEmitter => {
      throw new Error("tccutil unavailable");
    };
    const harness = makeHarness({ probes: [false, false], dialogResponses: [0], spawn });

    await harness.controller.run();

    expect(readMarker(harness.requestPath).state).toBe("error");
    expect(harness.shell.opened).toHaveLength(0);
    expect(harness.shell.revealed).toHaveLength(0);
    expect(harness.panel.states).toContain("error");
    expect(harness.dialog.calls).toHaveLength(1);
  });

  test("single-flights concurrent activation checks so one pending request has one prompt", async () => {
    const harness = makeHarness({ probes: [false], dialogResponses: [1] });

    await Promise.all([harness.controller.run(), harness.controller.run()]);

    expect(harness.dialog.calls).toHaveLength(1);
    expect(readMarker(harness.requestPath).state).toBe("deferred");
  });

  test("does not let an old dialog reset or overwrite a re-armed request", async () => {
    const harness = makeHarness({
      marker: { requestId: "old-request" },
      probes: [false],
      dialogResponses: [0],
    });
    const originalShowMessageBox = harness.dialog.showMessageBox;
    harness.dialog.showMessageBox = async (...args: any[]) => {
      const result = await originalShowMessageBox(...args);
      const current = readMarker(harness.requestPath);
      writeMarker(harness.requestPath, {
        ...current,
        requestId: "new-request",
        state: "pending",
      });
      return result;
    };

    await harness.controller.run();

    expect(readMarker(harness.requestPath).requestId).toBe("new-request");
    expect(readMarker(harness.requestPath).state).toBe("pending");
    expect(harness.spawnCalls).toHaveLength(0);
    expect(harness.shell.opened).toHaveLength(0);
    expect(harness.shell.revealed).toHaveLength(0);
  });

  test("rechecks the request before replacing the marker after a write race", async () => {
    let harness: Harness;
    const racedFs = {
      ...nodeFs,
      writeFileSync(target: any, data: any, options?: any) {
        if (typeof target === "number") {
          const current = readMarker(harness.requestPath);
          writeMarker(harness.requestPath, {
            ...current,
            requestId: "replacement-during-write",
            state: "pending",
          });
        }
        return nodeFs.writeFileSync(target, data, options);
      },
    };
    harness = makeHarness({ probes: [true], fs: racedFs });

    await harness.controller.run();

    expect(readMarker(harness.requestPath).requestId).toBe("replacement-during-write");
    expect(readMarker(harness.requestPath).state).toBe("pending");
    expect(harness.dialog.calls).toHaveLength(0);
    expect(harness.spawnCalls).toHaveLength(0);
  });

  test("rejects a missing request without probing, resetting, or prompting", async () => {
    const root = temporaryRoot();
    const requestPath = markerPath(root);
    const createController = (runtimeMain as any).createAccessibilitySetupController;
    if (typeof createController !== "function") {
      throw new Error("dist/incodex-main.cjs does not export createAccessibilitySetupController");
    }
    let probes = 0;
    let resets = 0;
    let prompts = 0;
    const controller = createController({
      app: { getLocale: () => "en-US", isReady: () => true },
      shell: { openExternal: async () => true, showItemInFolder: () => {} },
      dialog: { showMessageBox: async () => { prompts += 1; return { response: 0 }; } },
      systemPreferences: { isTrustedAccessibilityClient: () => { probes += 1; return false; } },
      spawn: () => { resets += 1; throw new Error("must not run"); },
      fs: nodeFs,
      requestPath,
      appPath: APP_PATH,
      installId: INSTALL_ID,
      bundleId: BUNDLE_ID,
      platform: "darwin",
      isIncognito: false,
      copy: COPY,
      now: () => 1_700_000_000_100,
      pollDelaysMs: [],
      sleep: async () => {},
    });

    await controller.run();

    expect({ probes, resets, prompts }).toEqual({ probes: 0, resets: 0, prompts: 0 });
  });

  test.each([
    ["install id", { marker: { installId: "different-install" }, installId: INSTALL_ID }],
    ["app path", { marker: { appPath: "/Applications/Other.app" }, installId: INSTALL_ID }],
    ["non-default target", { marker: {}, appPath: "/Applications/Clone.app", installId: INSTALL_ID }],
  ])("rejects %s mismatch before probing", async (_name, options: any) => {
    const harness = makeHarness(options);
    await harness.controller.run();
    expect(harness.probes).toHaveLength(1);
    expect(harness.dialog.calls).toHaveLength(0);
    expect(harness.spawnCalls).toHaveLength(0);
    expect(readMarker(harness.requestPath).state).toBe("pending");
  });

  test("rejects a symlinked marker ancestry", async () => {
    const root = temporaryRoot();
    const realTransactions = join(root, "real-transactions");
    const linkedTransactions = join(root, "transactions");
    mkdirSync(realTransactions, { recursive: true, mode: 0o700 });
    symlinkSync(realTransactions, linkedTransactions, "dir");
    const requestPath = markerPath(root);
    writeMarker(requestPath);

    const harness = makeHarness({ requestPath, probes: [false] });
    await harness.controller.run();

    expect(harness.probes).toHaveLength(1);
    expect(harness.dialog.calls).toHaveLength(0);
    expect(readMarker(requestPath).state).toBe("pending");
  });

  test("rejects a group/world-writable marker and an oversized marker", async () => {
    const writable = makeHarness({ probes: [false], markerMode: 0o620 });
    await writable.controller.run();
    expect(writable.dialog.calls).toHaveLength(0);
    expect(readMarker(writable.requestPath).state).toBe("pending");

    const oversized = makeHarness({ probes: [false] });
    writeFileSync(oversized.requestPath, "x".repeat(8 * 1024 + 1));
    await oversized.controller.run();
    expect(oversized.dialog.calls).toHaveLength(0);
    expect(readFileSync(oversized.requestPath, "utf8")).toHaveLength(8 * 1024 + 1);
  });
});


describe("single-window Accessibility setup", () => {
  test("keeps the native guide in the matching supported language", () => {
    const resolveCopy = (runtimeMain as any).resolveAccessibilityCopy;
    expect(typeof resolveCopy).toBe("function");
    expect(resolveCopy("zh-CN").body).toBe("安装 Incodex 会修改 ChatGPT，因此需要重新授予它辅助功能权限。");
    expect(resolveCopy("zh-HK").later).toBe("稍後");
    expect(resolveCopy("zh-TW").later).toBe("稍後");
    expect(resolveCopy("en").back).toBe("Back");
    expect(resolveCopy("zh-CN").back).toBe("返回");
    expect(resolveCopy("zh-HK").back).toBe("返回");
    expect(resolveCopy("ja-JP").body).not.toBe(resolveCopy("en").body);
  });

  test("keeps one window and detects grant while Settings remains frontmost", async () => {
    const h = makeHarness({ probes: [false, false, false, true], dialogResponses: [0] });
    await h.controller.run();
    expect(h.dialog.calls).toHaveLength(1);
    expect(h.shell.revealed).toHaveLength(0);
    expect(h.panel.states).toContain("awaiting-user");
    expect(h.timerActive()).toBe(true);
    h.tick();
    expect(readMarker(h.requestPath).state).toBe("awaiting-user");
    h.tick();
    expect(readMarker(h.requestPath).state).toBe("granted");
    expect(h.panel.isDestroyed()).toBe(true);
    expect(h.timerActive()).toBe(false);
    expect(h.spawnCalls).toHaveLength(1);
  });

  test("retries the same awaiting request without a second reset", async () => {
    const h = makeHarness({ probes: [false, false, false], dialogResponses: [0] });

    await h.controller.run();
    expect(readMarker(h.requestPath).state).toBe("awaiting-user");
    expect(h.spawnCalls).toHaveLength(1);
    expect(h.shell.opened).toHaveLength(1);

    await h.panel.triggerRetry();

    expect(readMarker(h.requestPath).state).toBe("awaiting-user");
    expect(h.spawnCalls).toHaveLength(1);
    expect(h.shell.opened).toHaveLength(2);
    expect(h.panel.states).toEqual(expect.arrayContaining(["repairing", "awaiting-user"]));
  });

  test("completes an existing awaiting request when retry observes host trust", async () => {
    const h = makeHarness({ probes: [false, false, true], dialogResponses: [0] });

    await h.controller.run();
    await h.panel.triggerRetry();

    expect(readMarker(h.requestPath).state).toBe("granted");
    expect(h.spawnCalls).toHaveLength(1);
    expect(h.shell.opened).toHaveLength(1);
    expect(h.panel.isDestroyed()).toBe(true);
    expect(h.timerActive()).toBe(false);
  });

  test("does not resume a retry after the guide closes while Settings is opening", async () => {
    let openCalls = 0;
    let release!: (value: boolean) => void;
    const secondOpen = new Promise<boolean>((resolve) => { release = resolve; });
    const h = makeHarness({
      probes: [false, false, false],
      dialogResponses: [0],
      openExternal: async () => {
        openCalls += 1;
        return openCalls === 2 ? secondOpen : true;
      },
    });

    await h.controller.run();
    const retry = h.panel.triggerRetry();
    h.panel.close();
    release(true);
    await retry;

    expect(readMarker(h.requestPath).state).toBe("deferred");
    expect(h.panel.states.filter((state: string) => state === "awaiting-user")).toHaveLength(1);
    expect(h.spawnCalls).toHaveLength(1);
  });

  test("closes a retry callback from an expired request without overwriting it", async () => {
    const h = makeHarness({ probes: [false, false, false], dialogResponses: [0] });

    await h.controller.run();
    writeMarker(h.requestPath, {
      requestId: "replacement-request",
      requestedAtMs: 1_700_000_000_200,
      state: "awaiting-user",
    });
    await h.panel.triggerRetry();

    expect(readMarker(h.requestPath).requestId).toBe("replacement-request");
    expect(readMarker(h.requestPath).state).toBe("awaiting-user");
    expect(h.shell.opened).toHaveLength(1);
    expect(h.spawnCalls).toHaveLength(1);
    expect(h.panel.isDestroyed()).toBe(true);
  });

  test("closing the guide cancels polling and cannot leave automatic reset work", async () => {
    const h = makeHarness({ probes: [false, false], dialogResponses: [0] });
    await h.controller.run();
    expect(h.timerActive()).toBe(true);
    h.panel.close();
    expect(h.timerActive()).toBe(false);
    expect(readMarker(h.requestPath).state).toBe("deferred");
  });

  test("a stale polling callback cannot grant or overwrite a fresh install request", async () => {
    const h = makeHarness({ marker: { requestId: "old" }, probes: [false, false, true], dialogResponses: [0] });
    await h.controller.run();
    writeMarker(h.requestPath, { requestId: "new", state: "pending" });
    h.tick();
    expect(readMarker(h.requestPath).state).toBe("pending");
    expect(h.timerActive()).toBe(false);
  });
});

test("a reset timeout kills and reaps tccutil before reporting failure", async () => {
  const child = Object.assign(new EventEmitter(), {
    killedBy: "",
    kill(this: EventEmitter & { killedBy: string }, signal: string) { this.killedBy = signal; queueMicrotask(() => this.emit("close", null, signal)); return true; },
  });
  const h = makeHarness({ probes: [false, false], dialogResponses: [0], spawn: () => child });
  await h.controller.run();
  expect(child.killedBy).toBe("SIGKILL");
  expect(readMarker(h.requestPath).state).toBe("error");
  expect(h.shell.opened).toHaveLength(0);
}, 7000);

test("opens Settings before asking the guide to locate its handoff destination", async () => {
  const h = makeHarness({ probes: [false, false], dialogResponses: [0] });
  await h.controller.run();
  expect(h.panel.openedBeforeHandoff).toBe(true);
});


test("published permission resolver follows the shared locale selection for all languages and aliases", () => {
  const guide = ACCESSIBILITY_SETUP_COPY as Record<string, Record<string, string>>;
  const resolveCopy = (runtimeMain as any).resolveAccessibilityCopy;
  const locales = [...Object.keys(SUPPORTED_COPY), "fr", "pt", "es", "no", "de", "JA_jp", "zh-Hant-HK", "zh-Hant", "en-GB", "unknown"];
  for (const locale of locales) {
    const expected = guide[resolveLocale(locale)];
    expect(expected).toBeDefined();
    expect(resolveCopy(locale)).toEqual(expected);
  }
});

test("regional accessibility guides use verified macOS terminology", () => {
  const guide = ACCESSIBILITY_SETUP_COPY as Record<string, Record<string, string>>;

  expect(guide["es-ES"]).toMatchObject({
    addedTitle: "Permitir ChatGPT en Ajustes del Sistema",
    completeInSettings: "Completar en Ajustes del Sistema",
    repairing: "Preparando Ajustes del Sistema…",
    openSettings: "Abrir ajustes",
    errorBody: "No se pudo completar la configuración de permisos. Añade /Applications/ChatGPT.app en Ajustes del Sistema → Privacidad y seguridad → Accesibilidad y luego ejecuta incodex install para volver a comprobarlo.",
  });
  expect(guide["ca-ES"]).toMatchObject({
    addedTitle: "Permet ChatGPT a la Configuració del sistema",
    completeInSettings: "Completa-ho a la Configuració del sistema",
    repairing: "Preparant la Configuració del sistema…",
    openSettings: "Obre la configuració",
    errorBody: "No s’ha pogut completar la configuració dels permisos. Afegeix /Applications/ChatGPT.app a Configuració del sistema → Privacitat i seguretat → Accessibilitat i, després, executa incodex install per tornar-ho a comprovar.",
  });
  expect(guide["bg-BG"]).toMatchObject({
    body: "Инсталирането на Incodex променя ChatGPT, затова разрешението за улеснен достъп трябва да бъде дадено отново.",
    permissionTitle: "Улеснен достъп",
    addedBody: "Плъзнете иконата на ChatGPT по-горе в списъка за улеснен достъп и я разрешете. Завършете всяко удостоверяване на macOS. Достъпът се проверява автоматично; този прозорец се затваря, когато достъпът е готов.",
    dragInstruction: "Плъзнете ChatGPT в списъка по-горе, за да разрешите Улеснен достъп",
    errorBody: "Настройването на разрешенията не можа да бъде завършено. Добавете /Applications/ChatGPT.app в Системни настройки → Поверителност и сигурност → Улеснен достъп, след което изпълнете incodex install, за да проверите отново.",
  });
  expect(guide["ro-RO"].errorBody).toBe("Configurarea permisiunii nu a putut fi finalizată. Adăugați /Applications/ChatGPT.app în Configurări sistem → Intimitate și securitate → Accesibilitate, apoi rulați incodex install pentru a verifica din nou.");
  expect(guide["pl-PL"].permissionTitle).toBe("Dostępność");
});
