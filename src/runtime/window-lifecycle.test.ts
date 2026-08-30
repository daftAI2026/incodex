import { describe, expect, test } from "bun:test";

import { exitAfterLastMainWindowCloses } from "./incodex-window-lifecycle.cts";

interface TestWindow {
  isDestroyed(): boolean;
  isVisible(): boolean;
  on(event: "close" | "closed", callback: () => void): void;
}

function createWindow(): {
  window: TestWindow;
  emit(event: "close" | "closed"): void;
  hide(): void;
} {
  const listeners = new Map<string, Array<() => void>>();
  let destroyed = false;
  let visible = true;
  return {
    window: {
      isDestroyed: () => destroyed,
      isVisible: () => visible,
      on(event, callback) {
        const callbacks = listeners.get(event) || [];
        callbacks.push(callback);
        listeners.set(event, callbacks);
      },
    },
    emit(event) {
      if (event === "closed") destroyed = true;
      for (const callback of listeners.get(event) || []) callback();
    },
    hide() {
      visible = false;
    },
  };
}

describe("shared incognito window lifecycle", () => {
  test("exits after the last primary window is hidden instead of destroyed", () => {
    const fixture = createWindow();
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];

    exitAfterLastMainWindowCloses(
      fixture.window,
      () => false,
      (code) => exits.push(code),
      (callback) => scheduled.push(callback),
    );

    fixture.emit("close");
    fixture.hide();
    scheduled.shift()?.();

    expect(exits).toEqual([0]);
  });

  test("minimizing without a close event preserves the session", () => {
    const fixture = createWindow();
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];

    exitAfterLastMainWindowCloses(
      fixture.window,
      () => false,
      (code) => exits.push(code),
      (callback) => scheduled.push(callback),
    );

    fixture.hide();

    expect(scheduled).toEqual([]);
    expect(exits).toEqual([]);
  });

  test("a close probe and closed event can finish the session only once", () => {
    const fixture = createWindow();
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];

    exitAfterLastMainWindowCloses(
      fixture.window,
      () => false,
      (code) => exits.push(code),
      (callback) => scheduled.push(callback),
    );

    fixture.emit("close");
    fixture.emit("closed");
    scheduled.shift()?.();

    expect(exits).toEqual([0]);
  });
});
