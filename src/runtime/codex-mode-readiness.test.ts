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

    expect(page).toBe("blocked");
    expect(decideCodexModeAction(page, false, 0)).toBe("blocked");
  });

  test("rejects malformed snapshots instead of treating them as pending", () => {
    expect(() => deriveCodexModePageState(undefined)).toThrow("malformed");
    expect(() =>
      deriveCodexModePageState({
        modeAvailable: "no",
        modeLabel: null,
        officialBlockerVisible: 1,
      }),
    ).toThrow("malformed");
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

  test("shares one failure budget across rejected probes and logs unresolved once", async () => {
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
      maxProbeFailures: 2,
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

  test("keeps waiting through official blockers until Codex becomes available", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    let snapshot = {
      modeAvailable: false,
      modeLabel: "",
      officialBlockerVisible: true,
    };
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => snapshot,
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
    for (let check = 0; check < 25; check += 1) await scheduler.runNext();
    expect(events).toEqual(["codex-mode-blocked"]);
    expect(scheduler.activeTasks()).toHaveLength(1);

    snapshot = {
      modeAvailable: true,
      modeLabel: "Codex",
      officialBlockerVisible: false,
    };
    await scheduler.runNext();

    expect(events).toEqual(["codex-mode-blocked", "codex-mode-confirmed"]);
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("pauses the active deadline only while an official blocker is explicitly visible", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    let nowMs = 0;
    let snapshot = {
      modeAvailable: false,
      modeLabel: "",
      officialBlockerVisible: true,
    };
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => snapshot,
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      ...scheduler,
      activeTimeoutMs: 90_000,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      now: () => nowMs,
      selectFallback: () => true,
    });

    readiness.observe(win);
    await scheduler.runNext();
    nowMs = 10 * 60_000;
    await scheduler.runNext();
    expect(events).toEqual(["codex-mode-blocked"]);

    snapshot = {
      modeAvailable: true,
      modeLabel: "Codex",
      officialBlockerVisible: false,
    };
    await scheduler.runNext();

    expect(events).toEqual(["codex-mode-blocked", "codex-mode-confirmed"]);
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("bounds page-not-ready active time while letting Codex win at the deadline", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    let nowMs = 0;
    let snapshot = {
      modeAvailable: false,
      modeLabel: "",
      officialBlockerVisible: false,
    };
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => snapshot,
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      ...scheduler,
      activeTimeoutMs: 90_000,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      now: () => nowMs,
      selectFallback: () => true,
    });

    readiness.observe(win);
    nowMs = 89_999;
    await scheduler.runNext();
    expect(events).toEqual([]);

    snapshot = {
      modeAvailable: true,
      modeLabel: "Codex",
      officialBlockerVisible: false,
    };
    nowMs = 90_000;
    await scheduler.runNext();
    expect(events).toEqual(["codex-mode-confirmed"]);

    const terminalScheduler = controlledScheduler();
    const terminalEvents: string[] = [];
    nowMs = 0;
    const terminalReadiness = createCodexModeReadiness({
      ...terminalScheduler,
      activeTimeoutMs: 90_000,
      isIncognito: () => true,
      log: (event: string) => terminalEvents.push(event),
      now: () => nowMs,
      selectFallback: () => true,
    });
    terminalReadiness.observe(win);
    nowMs = 90_000;
    snapshot = {
      modeAvailable: false,
      modeLabel: "",
      officialBlockerVisible: false,
    };
    await terminalScheduler.runNext();

    expect(terminalEvents).toEqual(["codex-mode-unresolved"]);
    expect(terminalScheduler.activeTasks()).toHaveLength(0);
  });

  test("keeps an unfocused fallback pending without consuming failure budget", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    const fallbacks: unknown[] = [];
    let focused = false;
    let snapshot = {
      modeAvailable: true,
      modeLabel: "ChatGPT",
      officialBlockerVisible: false,
    };
    const win = {
      isDestroyed: () => false,
      isFocused: () => focused,
      once: () => {},
      webContents: {
        executeJavaScript: async () => snapshot,
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      ...scheduler,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      maxProbeFailures: 1,
      primaryOtherChecksRequired: 1,
      selectFallback: (selectedWindow: unknown) => {
        fallbacks.push(selectedWindow);
        return true;
      },
    });

    readiness.observe(win);
    for (let check = 0; check < 25; check += 1) await scheduler.runNext();
    expect(fallbacks).toHaveLength(0);
    expect(events).toEqual([]);

    focused = true;
    await scheduler.runNext();
    snapshot = {
      modeAvailable: true,
      modeLabel: "Codex",
      officialBlockerVisible: false,
    };
    await scheduler.runNext();

    expect(fallbacks).toHaveLength(1);
    expect(events).toEqual(["codex-mode-fallback-sent", "codex-mode-confirmed"]);
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("bounds an unfocused non-target page by active time", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    const fallbacks: unknown[] = [];
    let nowMs = 0;
    const win = {
      isDestroyed: () => false,
      isFocused: () => false,
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
      ...scheduler,
      activeTimeoutMs: 90_000,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      now: () => nowMs,
      primaryOtherChecksRequired: 1,
      selectFallback: (selectedWindow: unknown) => {
        fallbacks.push(selectedWindow);
        return true;
      },
    });

    readiness.observe(win);
    await scheduler.runNext();
    nowMs = 90_000;
    await scheduler.runNext();

    expect(fallbacks).toHaveLength(0);
    expect(events).toEqual(["codex-mode-unresolved"]);
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("requires consecutive non-target confirmations after the fallback", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    const snapshots = [
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: false, modeLabel: "", officialBlockerVisible: false },
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
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
      selectFallback: () => true,
    });

    readiness.observe(win);
    for (let check = 0; check < 6; check += 1) await scheduler.runNext();
    expect(events).toEqual(["codex-mode-fallback-sent"]);
    expect(scheduler.activeTasks()).toHaveLength(1);

    await scheduler.runNext();
    expect(events).toEqual(["codex-mode-fallback-sent", "codex-mode-unresolved"]);
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("an official blocker resets the pre-fallback non-target streak", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    const snapshots = [
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: false, modeLabel: "", officialBlockerVisible: true },
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
      { modeAvailable: true, modeLabel: "ChatGPT", officialBlockerVisible: false },
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
      selectFallback: () => true,
    });

    readiness.observe(win);
    for (let check = 0; check < 5; check += 1) await scheduler.runNext();
    expect(events).toEqual(["codex-mode-blocked"]);

    await scheduler.runNext();
    expect(events).toEqual(["codex-mode-blocked", "codex-mode-fallback-sent"]);
  });

  test("counts malformed resolved snapshots as technical failures", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => ({ modeAvailable: "yes" }),
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      ...scheduler,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      maxProbeFailures: 2,
      selectFallback: () => true,
    });

    readiness.observe(win);
    await scheduler.runNext();
    await scheduler.runNext();

    expect(events).toEqual([
      "codex-mode-probe-failed",
      "codex-mode-probe-failed",
      "codex-mode-unresolved",
    ]);
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
      maxProbeFailures: 1,
      selectFallback: () => true,
    });

    readiness.observe(win);
    await scheduler.runNext();
    expect(scheduler.activeTasks().map((task) => task.delay)).toEqual([2_000]);
    await scheduler.runNext();

    expect(events).toEqual(["codex-mode-probe-failed", "codex-mode-unresolved"]);
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("accepts Codex after nineteen probe failures", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    let failuresRemaining = 19;
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => {
          if (failuresRemaining > 0) {
            failuresRemaining -= 1;
            throw new Error("renderer unavailable");
          }
          return {
            modeAvailable: true,
            modeLabel: "Codex",
            officialBlockerVisible: false,
          };
        },
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      ...scheduler,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      maxProbeFailures: 20,
      selectFallback: () => true,
    });

    readiness.observe(win);
    for (let check = 0; check < 20; check += 1) await scheduler.runNext();

    expect(events).toEqual([
      ...Array.from({ length: 19 }, () => "codex-mode-probe-failed"),
      "codex-mode-confirmed",
    ]);
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("resets the probe failure streak after a successful pending snapshot", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    let attempt = 0;
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => {
          attempt += 1;
          if (attempt <= 19 || (attempt >= 21 && attempt <= 39)) {
            throw new Error("renderer unavailable");
          }
          if (attempt === 20) {
            return {
              modeAvailable: false,
              modeLabel: "",
              officialBlockerVisible: false,
            };
          }
          return {
            modeAvailable: true,
            modeLabel: "Codex",
            officialBlockerVisible: false,
          };
        },
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      ...scheduler,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      maxProbeFailures: 20,
      selectFallback: () => true,
    });

    readiness.observe(win);
    for (let check = 0; check < 40; check += 1) await scheduler.runNext();

    expect(events).toEqual([
      ...Array.from({ length: 38 }, () => "codex-mode-probe-failed"),
      "codex-mode-confirmed",
    ]);
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("requires twenty new consecutive failures after a successful pending snapshot", async () => {
    const scheduler = controlledScheduler();
    const events: string[] = [];
    let attempt = 0;
    const win = {
      isDestroyed: () => false,
      isFocused: () => true,
      once: () => {},
      webContents: {
        executeJavaScript: async () => {
          attempt += 1;
          if (attempt === 20) {
            return {
              modeAvailable: false,
              modeLabel: "",
              officialBlockerVisible: false,
            };
          }
          throw new Error("renderer unavailable");
        },
        isDestroyed: () => false,
      },
    };
    const readiness = createCodexModeReadiness({
      ...scheduler,
      isIncognito: () => true,
      log: (event: string) => events.push(event),
      maxProbeFailures: 20,
      selectFallback: () => true,
    });

    readiness.observe(win);
    for (let check = 0; check < 20; check += 1) await scheduler.runNext();
    await scheduler.runNext();
    expect(events.at(-1)).toBe("codex-mode-probe-failed");
    expect(scheduler.activeTasks()).toHaveLength(1);

    for (let check = 0; check < 18; check += 1) await scheduler.runNext();
    expect(events).not.toContain("codex-mode-unresolved");
    expect(scheduler.activeTasks()).toHaveLength(1);

    await scheduler.runNext();
    expect(events.at(-1)).toBe("codex-mode-unresolved");
    expect(scheduler.activeTasks()).toHaveLength(0);
  });

  test("probes nested accessible labels and blocks every official dialog shape", () => {
    expect(() => new Function(`return ${CODEX_MODE_PROBE_EXPRESSION}`)).not.toThrow();
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
