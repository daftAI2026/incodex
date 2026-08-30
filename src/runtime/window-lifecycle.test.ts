import { describe, expect, test } from "bun:test";

import {
  createIncognitoWindowLifecycle,
  exitAfterLastMainWindowCloses,
} from "./incodex-window-lifecycle.cts";

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
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
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
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
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
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );

    fixture.emit("close");
    fixture.emit("closed");
    scheduled.shift()?.();

    expect(exits).toEqual([0]);
  });

  test("a hidden non-final window stops probing instead of exiting on a later app hide", () => {
    const fixture = createWindow();
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];
    let anotherMainWindow = true;

    exitAfterLastMainWindowCloses(
      fixture.window,
      () => anotherMainWindow,
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );

    fixture.emit("close");
    fixture.hide();
    scheduled.shift()?.();

    expect(exits).toEqual([]);
    expect(scheduled).toEqual([]);

    anotherMainWindow = false;
    expect(exits).toEqual([]);
  });

  test("a minimized sibling remains active until that sibling is explicitly closed", () => {
    const first = createWindow();
    const sibling = createWindow();
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];
    const lifecycle = createIncognitoWindowLifecycle(
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );
    lifecycle.observe(first.window);
    lifecycle.observe(sibling.window);

    sibling.hide();
    first.emit("close");
    first.hide();
    scheduled.shift()?.();

    expect(exits).toEqual([]);

    sibling.emit("close");
    scheduled.shift()?.();

    expect(exits).toEqual([0]);
  });
});
