import { describe, expect, test } from "bun:test";
import {
  CODEX_MODE_PROBE_EXPRESSION,
  createCodexModeReadiness,
  decideCodexModeAction,
  deriveCodexModePageState,
} from "./incodex-codex-mode.cts";

type ScheduledTask = { callback: () => void; delay: number };

async function runNext(tasks: ScheduledTask[]): Promise<void> {
  const task = tasks.shift();
  expect(task).toBeDefined();
  task?.callback();
  await Promise.resolve();
  await Promise.resolve();
}

describe("Codex mode readiness", () => {
  test("emits a syntactically valid renderer probe", () => {
    expect(() => new Function(`return ${CODEX_MODE_PROBE_EXPRESSION}`)).not.toThrow();
  });

  test("confirms the primary codex route without invoking its keyboard fallback", () => {
    const page = deriveCodexModePageState({
      modeAvailable: true,
      modeLabel: "Codex",
      officialBlockerVisible: false,
    });

    expect(page).toBe("codex");
    expect(decideCodexModeAction(page, false, 0)).toBe("confirmed");
  });

  test("waits while official onboarding keeps the final mode unavailable", () => {
    const page = deriveCodexModePageState({
      modeAvailable: false,
      modeLabel: "",
      officialBlockerVisible: true,
    });

    expect(page).toBe("pending");
    expect(decideCodexModeAction(page, false, 0)).toBe("wait");
  });

  test("bounds a settled page whose mode control is temporarily absent", () => {
    const page = deriveCodexModePageState({
      modeAvailable: false,
      modeLabel: "",
      officialBlockerVisible: false,
    });

    expect(page).toBe("missing");
    expect(decideCodexModeAction(page, false, 0, 1)).toBe("wait");
    expect(decideCodexModeAction(page, false, 0, 3)).toBe("select-fallback");
  });

  test("uses Control+3 only after repeated stable evidence that the primary route missed Codex", () => {
    const page = deriveCodexModePageState({
      modeAvailable: true,
      modeLabel: "ChatGPT",
      officialBlockerVisible: false,
    });

    expect(page).toBe("other");
    expect(decideCodexModeAction(page, false, 0, 1)).toBe("wait");
    expect(decideCodexModeAction(page, false, 0, 2)).toBe("wait");
    expect(decideCodexModeAction(page, false, 0, 3)).toBe("select-fallback");
  });

  test("gives the official renderer enough time to complete an accepted fallback", () => {
    expect(decideCodexModeAction("other", true, 0)).toBe("wait");
    expect(decideCodexModeAction("other", true, 2)).toBe("wait");
    expect(decideCodexModeAction("other", true, 8)).toBe("wait");
    expect(decideCodexModeAction("other", true, 20)).toBe("unresolved");
  });

  test("confirms a renderer fallback whose official route settles after several polls", async () => {
    const tasks: ScheduledTask[] = [];
    const logs: Array<[string, unknown]> = [];
    const snapshots = [
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: true, modeLabel: "Codex", officialBlockerVisible: false },
    ];
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => snapshots.shift(),
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      isIncognito: () => true,
      log: (event: string, detail: unknown) => logs.push([event, detail]),
      primaryOtherChecksRequired: 1,
      selectFallback: async () => true,
      scheduleTimer: (callback: () => void, delay: number) => {
        const task = { callback, delay };
        tasks.push(task);
        return task;
      },
    });

    readiness.observe(win);
    await runNext(tasks);
    await runNext(tasks);
    await runNext(tasks);
    await runNext(tasks);

    expect(tasks).toHaveLength(0);
    expect(logs).toContainEqual(["codex-mode-confirmed", { fallback: true }]);
  });

  test("keeps observing onboarding and accepts the primary route without a fallback", async () => {
    const tasks: ScheduledTask[] = [];
    const fallbacks: unknown[] = [];
    const snapshots = [
      { modeAvailable: false, modeLabel: "", officialBlockerVisible: true },
      { modeAvailable: true, modeLabel: "Codex", officialBlockerVisible: false },
    ];
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => snapshots.shift(),
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      isIncognito: () => true,
      log: () => {},
      selectFallback: (selectedWindow: unknown) => {
        fallbacks.push(selectedWindow);
        return true;
      },
      scheduleTimer: (callback: () => void, delay: number) => {
        const task = { callback, delay };
        tasks.push(task);
        return task;
      },
    });

    readiness.observe(win);
    expect(tasks[0]?.delay).toBe(1_500);
    await runNext(tasks);
    expect(tasks[0]?.delay).toBe(750);
    await runNext(tasks);

    expect(fallbacks).toHaveLength(0);
    expect(tasks).toHaveLength(0);
  });

  test("stops polling when an official blocker never leaves", async () => {
    const tasks: ScheduledTask[] = [];
    const logs: Array<[string, unknown]> = [];
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => ({
          modeAvailable: false,
          modeLabel: "",
          officialBlockerVisible: true,
        }),
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      isIncognito: () => true,
      log: (event: string, detail: unknown) => logs.push([event, detail]),
      selectFallback: () => true,
      totalChecksRequired: 3,
      scheduleTimer: (callback: () => void, delay: number) => {
        const task = { callback, delay };
        tasks.push(task);
        return task;
      },
    });

    readiness.observe(win);
    await runNext(tasks);
    await runNext(tasks);
    await runNext(tasks);

    expect(tasks).toHaveLength(0);
    expect(logs.at(-1)).toEqual([
      "codex-mode-unresolved",
      { fallback: false, reason: "readiness-deadline" },
    ]);
  });

  test("bounds a renderer probe that never settles", async () => {
    const tasks: ScheduledTask[] = [];
    const logs: Array<[string, unknown]> = [];
    const never = new Promise(() => {});
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => never,
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      isIncognito: () => true,
      log: (event: string, detail: unknown) => logs.push([event, detail]),
      probeFailuresRequired: 1,
      probeTimeoutMs: 5,
      selectFallback: () => true,
      scheduleTimer: (callback: () => void, delay: number) => {
        const task = { callback, delay };
        tasks.push(task);
        return task;
      },
    });

    readiness.observe(win);
    const task = tasks.shift();
    task?.callback();
    await Bun.sleep(20);

    expect(tasks).toHaveLength(0);
    expect(logs).toHaveLength(2);
    expect(String(logs[0]?.[1])).toContain("timed out");
    expect(logs.at(-1)).toEqual([
      "codex-mode-unresolved",
      { fallback: false, reason: "probe-failed" },
    ]);
  });

  test("attempts its keyboard fallback at most once even when selection fails", async () => {
    const tasks: ScheduledTask[] = [];
    const fallbacks: unknown[] = [];
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => ({
          modeAvailable: true,
          modeLabel: "ChatGPT",
          officialBlockerVisible: false,
        }),
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      isIncognito: () => true,
      log: () => {},
      primaryOtherChecksRequired: 1,
      selectFallback: (selectedWindow: unknown) => {
        fallbacks.push(selectedWindow);
        return false;
      },
      scheduleTimer: (callback: () => void, delay: number) => {
        const task = { callback, delay };
        tasks.push(task);
        return task;
      },
    });

    readiness.observe(win);
    await runNext(tasks);

    expect(fallbacks).toHaveLength(1);
    expect(tasks).toHaveLength(0);
  });

  test("waits for an asynchronous renderer fallback before confirming it was sent", async () => {
    const tasks: ScheduledTask[] = [];
    const logs: string[] = [];
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => ({
          modeAvailable: true,
          modeLabel: "ChatGPT",
          officialBlockerVisible: false,
        }),
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      isIncognito: () => true,
      log: (event: string) => logs.push(event),
      primaryOtherChecksRequired: 1,
      selectFallback: async () => true,
      scheduleTimer: (callback: () => void, delay: number) => {
        const task = { callback, delay };
        tasks.push(task);
        return task;
      },
    });

    readiness.observe(win);
    await runNext(tasks);

    expect(logs).toContain("codex-mode-fallback-sent");
    expect(tasks).toHaveLength(1);
  });

  test("does not poll forever when a settled page never exposes its mode control", async () => {
    const tasks: ScheduledTask[] = [];
    const fallbacks: unknown[] = [];
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => ({
          modeAvailable: false,
          modeLabel: "",
          officialBlockerVisible: false,
        }),
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      isIncognito: () => true,
      log: () => {},
      primaryOtherChecksRequired: 3,
      selectFallback: (selectedWindow: unknown) => {
        fallbacks.push(selectedWindow);
        return false;
      },
      scheduleTimer: (callback: () => void, delay: number) => {
        const task = { callback, delay };
        tasks.push(task);
        return task;
      },
    });

    readiness.observe(win);
    await runNext(tasks);
    await runNext(tasks);
    await runNext(tasks);

    expect(fallbacks).toHaveLength(1);
    expect(tasks).toHaveLength(0);
  });

  test("does not send a fallback when the window closes during its DOM probe", async () => {
    const tasks: ScheduledTask[] = [];
    const fallbacks: unknown[] = [];
    let closeWindow = () => {};
    let resolveSnapshot = (_snapshot: unknown) => {};
    const snapshot = new Promise((resolve) => {
      resolveSnapshot = resolve;
    });
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: (_event: string, callback: () => void) => {
        closeWindow = callback;
      },
      webContents: {
        executeJavaScript: async () => snapshot,
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      isIncognito: () => true,
      log: () => {},
      primaryOtherChecksRequired: 1,
      selectFallback: (selectedWindow: unknown) => {
        fallbacks.push(selectedWindow);
        return true;
      },
      scheduleTimer: (callback: () => void, delay: number) => {
        const task = { callback, delay };
        tasks.push(task);
        return task;
      },
    });

    readiness.observe(win);
    const task = tasks.shift();
    expect(task).toBeDefined();
    task?.callback();
    await Promise.resolve();
    closeWindow();
    resolveSnapshot({
      modeAvailable: true,
      modeLabel: "ChatGPT",
      officialBlockerVisible: false,
    });
    await Promise.resolve();
    await Promise.resolve();

    expect(fallbacks).toHaveLength(0);
    expect(tasks).toHaveLength(0);
  });

  test("stops polling after repeated renderer probe failures", async () => {
    const tasks: ScheduledTask[] = [];
    const logs: Array<[string, unknown]> = [];
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => {
          throw new Error("execution context disappeared");
        },
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      isIncognito: () => true,
      log: (event: string, detail: unknown) => logs.push([event, detail]),
      probeFailuresRequired: 3,
      selectFallback: () => true,
      scheduleTimer: (callback: () => void, delay: number) => {
        const task = { callback, delay };
        tasks.push(task);
        return task;
      },
    });

    readiness.observe(win);
    await runNext(tasks);
    await runNext(tasks);
    await runNext(tasks);

    expect(tasks).toHaveLength(0);
    expect(logs.filter(([event]) => event === "codex-mode-probe-failed")).toHaveLength(3);
    expect(logs.at(-1)).toEqual([
      "codex-mode-unresolved",
      { fallback: false, reason: "probe-failed" },
    ]);
  });

  test("probes nested accessible labels and blocks every official dialog shape", () => {
    expect(CODEX_MODE_PROBE_EXPRESSION).toContain("textContent");
    expect(CODEX_MODE_PROBE_EXPRESSION).toContain('getAttribute("aria-label")');
    expect(CODEX_MODE_PROBE_EXPRESSION).toContain('[role="button"]');
    expect(CODEX_MODE_PROBE_EXPRESSION).toContain("dialog[open]");
    expect(CODEX_MODE_PROBE_EXPRESSION).toContain('[role="alertdialog"]');
    expect(CODEX_MODE_PROBE_EXPRESSION).toContain('[aria-hidden="true"]');
    expect(CODEX_MODE_PROBE_EXPRESSION).toContain("[inert]");
    expect(CODEX_MODE_PROBE_EXPRESSION).toContain("opacity");
    expect(CODEX_MODE_PROBE_EXPRESSION).toContain("getClientRects");
  });
});
