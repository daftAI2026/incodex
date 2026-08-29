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
