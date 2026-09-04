import { describe, expect, test } from "bun:test";
import {
  CODEX_MODE_PROBE_EXPRESSION,
  createCodexModeReadiness,
  decideCodexModeAction,
  deriveCodexModePageState,
} from "./incodex-codex-mode.cts";

type ScheduledTask = { callback: () => void; delay: number };

function controlledScheduler() {
  type Task = ScheduledTask & { active: boolean };
  const tasks: Task[] = [];
  return {
    activeTasks: () => tasks.filter((task) => task.active),
    cancelTimer: (task: Task) => {
      task.active = false;
    },
    runNext: async () => {
      const task = tasks.find((candidate) => candidate.active);
      expect(task).toBeDefined();
      if (!task) return;
      task.active = false;
      task.callback();
      for (let turn = 0; turn < 6; turn += 1) await Promise.resolve();
    },
    scheduleTimer: (callback: () => void, delay: number) => {
      const task = { active: true, callback, delay };
      tasks.push(task);
      return task;
    },
  };
}

async function runNext(tasks: ScheduledTask[]): Promise<void> {
  const task = tasks.shift();
  expect(task).toBeDefined();
  task?.callback();
  for (let turn = 0; turn < 6; turn += 1) await Promise.resolve();
}

describe("Codex mode readiness", () => {
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

  test("never repeats the fallback after bounded confirmation fails", () => {
    expect(decideCodexModeAction("other", true, 0)).toBe("wait");
    expect(decideCodexModeAction("other", true, 2)).toBe("unresolved");
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
      cancelTimer: (task: ScheduledTask) => {
        const index = tasks.indexOf(task);
        if (index >= 0) tasks.splice(index, 1);
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
      cancelTimer: (task: ScheduledTask) => {
        const index = tasks.indexOf(task);
        if (index >= 0) tasks.splice(index, 1);
      },
    });

    readiness.observe(win);
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
      cancelTimer: (task: ScheduledTask) => {
        const index = tasks.indexOf(task);
        if (index >= 0) tasks.splice(index, 1);
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
    for (let turn = 0; turn < 6; turn += 1) await Promise.resolve();

    expect(fallbacks).toHaveLength(0);
    expect(tasks).toHaveLength(0);
  });

  test("shares one total budget across rejected probes and logs unresolved once", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => {
          throw new Error("renderer unavailable");
        },
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      ...scheduler,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      maxChecks: 2,
      selectFallback: () => true,
    });

    readiness.observe(win);
    await scheduler.runNext();
    await scheduler.runNext();
    readiness.observe(win);

    expect(events).toEqual([
      "codex-mode-probe-failed",
      "codex-mode-probe-failed",
      "codex-mode-unresolved",
    ]);
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("bounds permanent pending pages without leaving a scheduled timer", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
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
      ...scheduler,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      selectFallback: () => true,
    });

    readiness.observe(win);
    for (let check = 0; check < 20; check += 1) await scheduler.runNext();

    expect(events).toEqual(["codex-mode-unresolved"]);
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("shares the budget across blocked and unfocused fallback checks", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    const fallbacks: unknown[] = [];
    const snapshots = [
      { modeAvailable: false, modeLabel: "", officialBlockerVisible: true },
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: false, modeLabel: "", officialBlockerVisible: true },
    ];
    const win = {
      isDestroyed: () => false,
      isFocused: () => false,
      once: () => {},
      webContents: {
        executeJavaScript: async () => snapshots.shift(),
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      ...scheduler,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      maxChecks: 3,
      primaryOtherChecksRequired: 1,
      selectFallback: (selectedWindow: unknown) => {
        fallbacks.push(selectedWindow);
        return true;
      },
    });

    readiness.observe(win);
    await scheduler.runNext();
    await scheduler.runNext();
    await scheduler.runNext();

    expect(fallbacks).toHaveLength(0);
    expect(events).toEqual(["codex-mode-unresolved"]);
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("gives each renderer probe a two second deadline", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    const never = new Promise(() => {});
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: () => never,
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      ...scheduler,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      maxChecks: 1,
      selectFallback: () => true,
    });

    readiness.observe(win);
    await scheduler.runNext();
    expect(scheduler.activeTasks().map((task) => task.delay)).toEqual([2_000]);
    await scheduler.runNext();

    expect(events).toEqual(["codex-mode-probe-failed", "codex-mode-unresolved"]);
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("accepts Codex on the final allowed check before applying the limit", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
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
      ...scheduler,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      maxChecks: 2,
      selectFallback: () => true,
    });

    readiness.observe(win);
    await scheduler.runNext();
    await scheduler.runNext();

    expect(events).toEqual(["codex-mode-confirmed"]);
    expect(scheduler.activeTasks()).toHaveLength(0);
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
