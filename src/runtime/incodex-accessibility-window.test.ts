import { EventEmitter } from "node:events";
import { describe, expect, test } from "bun:test";
import { createAccessibilitySetupWindow } from "./incodex-accessibility-window.cts";

const APP_PATH = "/Applications/ChatGPT.app";
const PRELOAD_PATH = "/tmp/incodex-accessibility-preload.cjs";
const SETTINGS_URL = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const STATE_CHANNEL = "incodex-accessibility-state";
const ACTION_CHANNEL = "incodex-accessibility-action";
const ICON_DATA = "data:image/png;base64,CHATGPT-ICON";

const COPY = {
  title: "Finish ChatGPT script-control setup",
  body: "ChatGPT cannot currently control other apps through scripts.",
  repair: "Repair and open Settings",
  later: "Later",
  addedTitle: "Allow ChatGPT in System Settings",
  addedBody: "Drag ChatGPT into Accessibility and wait for the automatic check.",
  openSettings: "Open Settings",
  errorTitle: "ChatGPT permission setup is incomplete",
  errorBody: "Add ChatGPT in Accessibility settings, then try again.",
  checking: "Checking automatically",
  repairing: "Preparing System Settings…",
};

type BrowserWindowOptions = {
  webPreferences?: Record<string, unknown>;
  [key: string]: unknown;
};

class FakeWebContents extends EventEmitter {
  readonly mainFrame = { url: "" };
  readonly dragCalls: Array<Record<string, unknown>> = [];
  readonly executeCalls: string[] = [];
  readonly sendCalls: Array<{ channel: string; payload: unknown }> = [];
  windowOpenHandler: ((details: unknown) => unknown) | undefined;

  setWindowOpenHandler(handler: (details: unknown) => unknown): void {
    this.windowOpenHandler = handler;
  }

  startDrag(options: Record<string, unknown>): void {
    this.dragCalls.push(options);
  }

  executeJavaScript(source: string): Promise<unknown> {
    this.executeCalls.push(source);
    return Promise.resolve(undefined);
  }

  send(channel: string, payload: unknown): void {
    this.sendCalls.push({ channel, payload });
  }
}

class FakeBrowserWindow extends EventEmitter {
  static instances: FakeBrowserWindow[] = [];

  readonly webContents = new FakeWebContents();
  readonly loadedUrls: string[] = [];
  readonly options: BrowserWindowOptions;
  closeCalls = 0;
  showCalls = 0;
  focusCalls = 0;
  private destroyed = false;

  constructor(options: BrowserWindowOptions) {
    super();
    this.options = options;
    FakeBrowserWindow.instances.push(this);
  }

  async loadURL(url: string): Promise<void> {
    this.loadedUrls.push(url);
    this.webContents.mainFrame.url = url;
  }

  show(): void {
    this.showCalls += 1;
  }

  focus(): void {
    this.focusCalls += 1;
  }

  close(): void {
    if (this.destroyed) return;
    this.closeCalls += 1;
    this.emit("close", {});
    this.destroyed = true;
    this.emit("closed");
  }

  isDestroyed(): boolean {
    return this.destroyed;
  }
}

type Harness = {
  api: Awaited<ReturnType<typeof createAccessibilitySetupWindow>>;
  electron: {
    BrowserWindow: typeof FakeBrowserWindow;
    app: {
      getFileIcon: (file: string) => Promise<unknown>;
    };
    shell: {
      openExternal: (url: string) => Promise<boolean>;
    };
    dialog: {
      showErrorBox: (...args: unknown[]) => void;
    };
  };
  window: FakeBrowserWindow;
  icon: { toDataURL: () => string };
  iconCalls: string[];
  settingsCalls: string[];
  dialogCalls: unknown[][];
  appFocusCalls: Array<{ steal?: boolean }>;
};

function dataUrlDocument(url: string): string {
  const comma = url.indexOf(",");
  if (comma < 0) return "";
  const metadata = url.slice(0, comma);
  const payload = url.slice(comma + 1);
  if (metadata.endsWith(";base64")) return Buffer.from(payload, "base64").toString("utf8");
  return decodeURIComponent(payload);
}

async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

async function makeHarness(): Promise<Harness> {
  FakeBrowserWindow.instances = [];
  const icon = { toDataURL: () => ICON_DATA, isEmpty: () => false, resize() { return this; } };
  const iconCalls: string[] = [];
  const settingsCalls: string[] = [];
  const dialogCalls: unknown[][] = [];
  const appFocusCalls: Array<{ steal?: boolean }> = [];
  const electron = {
    BrowserWindow: FakeBrowserWindow,
    app: {
      focus(options?: { steal?: boolean }): void {
        appFocusCalls.push(options ?? {});
      },
      async getFileIcon(file: string): Promise<unknown> {
        throw new Error(`IconServices must not be called: ${file}`);
      },
    },
    nativeImage: { createFromPath(file: string) { iconCalls.push(file); return icon; } },
    shell: {
      async openExternal(url: string): Promise<boolean> {
        settingsCalls.push(url);
        return true;
      },
    },
    dialog: {
      showErrorBox(...args: unknown[]): void {
        dialogCalls.push(args);
      },
    },
  };
  const api = await createAccessibilitySetupWindow({
    electron,
    copy: COPY,
    appPath: APP_PATH,
    preloadPath: PRELOAD_PATH,
  });
  const window = FakeBrowserWindow.instances[0];
  if (!window) throw new Error("accessibility setup window was not created");
  return { api, electron, window, icon, iconCalls, settingsCalls, dialogCalls, appFocusCalls };
}

function emitAction(
  harness: Harness,
  action: unknown,
  senderFrame = harness.window.webContents.mainFrame,
  channel = ACTION_CHANNEL,
): void {
  harness.window.webContents.emit("ipc-message", { senderFrame }, channel, action);
}

function stateMessages(harness: Harness): Array<{ channel: string; payload: any }> {
  return harness.window.webContents.sendCalls.filter(({ channel }) => channel === STATE_CHANNEL);
}

describe("bounded Accessibility setup window", () => {
  test("creates one isolated bounded window with a centered app icon and frozen data document", async () => {
    const harness = await makeHarness();
    const preferences = harness.window.options.webPreferences ?? {};
    const document = dataUrlDocument(harness.window.loadedUrls[0] ?? "");

    expect(FakeBrowserWindow.instances).toHaveLength(1);
    expect(preferences).toMatchObject({
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      preload: PRELOAD_PATH,
      additionalArguments: ["--incodex-accessibility-setup"],
    });
    expect(harness.window.options.alwaysOnTop).toBe(true);
    expect(harness.window.options.modal).toBe(false);
    expect(harness.window.options.width).toBeGreaterThanOrEqual(340);
    expect(harness.window.options.width).toBeLessThanOrEqual(430);
    expect(typeof preferences.partition).toBe("string");
    expect(String(preferences.partition).startsWith("persist:")).toBe(false);
    expect(harness.window.loadedUrls).toHaveLength(1);
    expect(harness.window.loadedUrls[0]).toMatch(/^data:text\/html/);
    expect(harness.window.webContents.mainFrame.url).toBe(harness.window.loadedUrls[0]);
    expect(harness.appFocusCalls).toEqual([{ steal: true }]);
    expect(harness.window.showCalls).toBe(1);
    expect(harness.window.focusCalls).toBe(1);
    expect(harness.iconCalls).toEqual([`${APP_PATH}/Contents/Resources/icon-chatgpt.png`]);
    expect(document).toContain(`id="app-icon"`);
    expect(document).toContain(`id="title"`);
    expect(document).toContain(`id="body"`);
    expect(document).toContain(`id="status"`);
    expect(document).toContain(`id="repair"`);
    expect(document).toContain(`id="later"`);
    expect(document).toContain(`id="settings"`);
    expect(document).toContain(ICON_DATA);
    expect(document).toContain(COPY.repair);
    expect(document).toContain(COPY.later);
  });

  test("denies navigation and every new window from the data document", async () => {
    const harness = await makeHarness();
    const webContents = harness.window.webContents;

    expect(webContents.windowOpenHandler?.({ url: "https://example.invalid" })).toEqual({
      action: "deny",
    });

    let prevented = false;
    webContents.emit("will-navigate", {
      preventDefault: () => {
        prevented = true;
      },
    }, "https://example.invalid");
    expect(prevented).toBe(true);
  });

  test("accepts pending repair and later only from the frozen top frame", async () => {
    const harness = await makeHarness();
    const frame = harness.window.webContents.mainFrame;
    const frozenUrl = frame.url;
    let result: "repair" | "later" | undefined;
    void harness.api.choice.then((choice) => {
      result = choice;
    });

    emitAction(harness, "repair", { url: frozenUrl });
    emitAction(harness, "repair", frame, "wrong-channel");
    frame.url = `${frozenUrl}#changed`;
    emitAction(harness, "repair", frame);
    frame.url = frozenUrl;
    emitAction(harness, { action: "repair" }, frame);
    await flush();
    expect(result).toBeUndefined();

    emitAction(harness, "later", frame);
    await expect(harness.api.choice).resolves.toBe("later");
    await flush();
    expect(harness.window.closeCalls).toBe(1);
  });

  test("ignores drag and settings while pending", async () => {
    const harness = await makeHarness();

    emitAction(harness, "drag");
    emitAction(harness, "settings");
    await flush();

    expect(harness.window.webContents.dragCalls).toHaveLength(0);
    expect(harness.settingsCalls).toHaveLength(0);
  });

  test("awaiting-user enables only the real app drag and fixed Accessibility settings action", async () => {
    const harness = await makeHarness();

    await harness.api.setState("awaiting-user");
    const appFocusCount = harness.appFocusCalls.length;
    const windowFocusCount = harness.window.focusCalls;
    emitAction(harness, "drag");
    emitAction(harness, "settings");
    await flush();

    expect(harness.window.webContents.dragCalls).toEqual([
      { file: APP_PATH, icon: harness.icon },
    ]);
    expect(harness.settingsCalls).toEqual([SETTINGS_URL]);
    expect(harness.appFocusCalls).toHaveLength(appFocusCount);
    expect(harness.window.focusCalls).toBe(windowFocusCount);
    expect(harness.api.isDestroyed()).toBe(false);
  });

  test("repairing blocks repair, drag, and settings while leaving later available", async () => {
    const harness = await makeHarness();
    let result: "repair" | "later" | undefined;
    void harness.api.choice.then((choice) => {
      result = choice;
    });

    await harness.api.setState("repairing");
    emitAction(harness, "repair");
    emitAction(harness, "drag");
    emitAction(harness, "settings");
    await flush();
    expect(result).toBeUndefined();
    expect(harness.window.webContents.dragCalls).toHaveLength(0);
    expect(harness.settingsCalls).toHaveLength(0);

    emitAction(harness, "later");
    await expect(harness.api.choice).resolves.toBe("later");
  });

  test("later closes the guide after repair has already settled its choice", async () => {
    const harness = await makeHarness();

    emitAction(harness, "repair");
    await expect(harness.api.choice).resolves.toBe("repair");
    await harness.api.setState("repairing");
    emitAction(harness, "later");
    await flush();

    expect(harness.window.closeCalls).toBe(1);
    expect(harness.api.isDestroyed()).toBe(true);
  });

  test("rejects a non-default app path before creating a window", async () => {
    FakeBrowserWindow.instances = [];
    const electron = {
      BrowserWindow: FakeBrowserWindow,
      app: { getFileIcon: async () => ({ toDataURL: () => ICON_DATA }) },
    };

    await expect(createAccessibilitySetupWindow({
      electron,
      copy: COPY,
      appPath: "/Applications/Other.app",
      preloadPath: PRELOAD_PATH,
    })).rejects.toThrow("default ChatGPT app");
    expect(FakeBrowserWindow.instances).toHaveLength(0);
  });

  test("publishes state text and control visibility through the frozen state channel", async () => {
    const harness = await makeHarness();

    await harness.api.setState("awaiting-user");
    const awaiting = stateMessages(harness).at(-1)?.payload;
    expect(awaiting).toMatchObject({
      state: "awaiting-user",
      title: COPY.addedTitle,
      body: COPY.addedBody,
      status: COPY.checking,
      repair: { text: COPY.repair, hidden: true },
      settings: { text: COPY.openSettings, hidden: false },
      appIcon: { draggable: true },
    });

    await harness.api.setState("error");
    const error = stateMessages(harness).at(-1)?.payload;
    expect(error).toMatchObject({
      state: "error",
      title: COPY.errorTitle,
      body: COPY.errorBody,
    });
    expect(harness.dialogCalls).toHaveLength(0);
    expect(FakeBrowserWindow.instances).toHaveLength(1);
  });

  test("keeps granted inert and does not create a second prompt", async () => {
    const harness = await makeHarness();
    let resolved = false;
    void harness.api.choice.then(() => {
      resolved = true;
    });

    await harness.api.setState("granted");
    emitAction(harness, "repair");
    emitAction(harness, "later");
    emitAction(harness, "drag");
    emitAction(harness, "settings");
    await flush();

    expect(resolved).toBe(false);
    expect(harness.window.webContents.dragCalls).toHaveLength(0);
    expect(harness.settingsCalls).toHaveLength(0);
    expect(harness.dialogCalls).toHaveLength(0);
    expect(FakeBrowserWindow.instances).toHaveLength(1);
  });

  test("allows later to close the original error or unknown guide", async () => {
    for (const state of ["error", "unknown"] as const) {
      const harness = await makeHarness();

      await harness.api.setState(state);
      emitAction(harness, "repair");
      emitAction(harness, "drag");
      emitAction(harness, "settings");
      await flush();

      expect(harness.window.webContents.dragCalls).toHaveLength(0);
      expect(harness.settingsCalls).toHaveLength(0);
      emitAction(harness, "later");
      await expect(harness.api.choice).resolves.toBe("later");
    }
  });

  test("exposes idempotent close, destruction, and one close callback", async () => {
    const harness = await makeHarness();
    let closed = 0;
    harness.api.onClose(() => {
      closed += 1;
    });

    expect(harness.api.isDestroyed()).toBe(false);
    harness.api.close();
    harness.api.close();

    expect(harness.window.closeCalls).toBe(1);
    expect(harness.api.isDestroyed()).toBe(true);
    expect(closed).toBe(1);
  });
});
