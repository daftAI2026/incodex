import { describe, expect, test } from "bun:test";

import {
  createIncognitoWindowLifecycle,
} from "./incodex-window-lifecycle.cts";

interface TestWindow {
  isDestroyed(): boolean;
  isVisible(): boolean;
  on(
    event: "close" | "closed",
    callback: (event?: { defaultPrevented?: boolean }) => void,
  ): void;
}

function createWindow(): {
  window: TestWindow;
  emit(event: "close" | "closed", value?: { defaultPrevented?: boolean }): void;
  hide(): void;
} {
  const listeners = new Map<
    string,
    Array<(event?: { defaultPrevented?: boolean }) => void>
  >();
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
    emit(event, value) {
      if (event === "closed") destroyed = true;
      for (const callback of listeners.get(event) || []) callback(value);
    },
    hide() {
      visible = false;
    },
  };
}

describe("shared incognito window lifecycle", () => {
  test("exits after the last incognito content window is hidden instead of destroyed", () => {
    const fixture = createWindow();
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];

    const lifecycle = createIncognitoWindowLifecycle(
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );
    lifecycle.observe(fixture.window);

    fixture.emit("close", { defaultPrevented: true });
    fixture.hide();
    scheduled.shift()?.();

    expect(exits).toEqual([0]);
  });

  test("minimizing without a close event preserves the session", () => {
    const fixture = createWindow();
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];

    const lifecycle = createIncognitoWindowLifecycle(
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );
    lifecycle.observe(fixture.window);

    fixture.hide();

    expect(scheduled).toEqual([]);
    expect(exits).toEqual([]);
  });

  test("a close probe and closed event can finish the session only once", () => {
    const fixture = createWindow();
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];

    const lifecycle = createIncognitoWindowLifecycle(
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );
    lifecycle.observe(fixture.window);

    fixture.emit("close");
    fixture.emit("closed");
    scheduled.shift()?.();

    expect(exits).toEqual([0]);
  });

  test("a hidden non-final content window stops probing instead of exiting later", () => {
    const fixture = createWindow();
    const sibling = createWindow();
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];

    const lifecycle = createIncognitoWindowLifecycle(
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );
    lifecycle.observe(fixture.window);
    lifecycle.observe(sibling.window);

    fixture.emit("close");
    fixture.hide();
    scheduled.shift()?.();

    expect(exits).toEqual([]);
    expect(scheduled).toEqual([]);

    sibling.hide();
    expect(exits).toEqual([]);
  });

  test("keeps probing a close until the host actually becomes hidden", () => {
    const fixture = createWindow();
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];
    const lifecycle = createIncognitoWindowLifecycle(
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );
    lifecycle.observe(fixture.window);

    fixture.emit("close");
    for (let attempt = 0; attempt < 50; attempt++) scheduled.shift()?.();

    expect(exits).toEqual([]);
    expect(scheduled).toHaveLength(1);

    fixture.hide();
    scheduled.shift()?.();
    expect(exits).toEqual([0]);
  });

  test("abandons a close probe when the host cancels that close request", () => {
    const fixture = createWindow();
    const scheduled: Array<() => void> = [];
    const exits: number[] = [];
    const lifecycle = createIncognitoWindowLifecycle(
      (code: number) => exits.push(code),
      (callback: () => void) => scheduled.push(callback),
    );
    lifecycle.observe(fixture.window);

    fixture.emit("close", { defaultPrevented: true });
    scheduled.shift()?.();

    expect(scheduled).toEqual([]);
    fixture.hide();
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
