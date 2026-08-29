import { describe, expect, test } from "bun:test";
import {
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

  test("uses Control+3 only after the primary route settles outside Codex", () => {
    const page = deriveCodexModePageState({
      modeAvailable: true,
      modeLabel: "ChatGPT",
      officialBlockerVisible: false,
    });

    expect(page).toBe("other");
    expect(decideCodexModeAction(page, false, 0)).toBe("select-fallback");
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
});
