import { describe, expect, test } from "bun:test";
import { createTooltipLifecycle } from "./tooltip-lifecycle.ts";

type ScheduledTask = {
  callback: () => void;
  cancelled: boolean;
};

function createHarness(
  canShow = true,
  timing: {
    resolveDelay?: (fallbackMs: number) => number;
    onOpen?: (close: () => void) => void;
    onClose?: () => void;
  } = {},
) {
  const tasks = new Map<number, ScheduledTask>();
  const events: string[] = [];
  const delays: number[] = [];
  let nextId = 1;
  const lifecycle = createTooltipLifecycle({
    delayMs: 700,
    schedule(callback, delayMs) {
      const id = nextId++;
      tasks.set(id, { callback, cancelled: false });
      delays.push(delayMs);
      return id;
    },
    cancel(id) {
      const task = tasks.get(id);
      if (task) task.cancelled = true;
    },
    canShow: () => canShow,
    show: () => events.push("show"),
    hide: () => events.push("hide"),
    ...timing,
  });

  return {
    lifecycle,
    events,
    delays,
    runScheduled() {
      for (const task of tasks.values()) {
        if (!task.cancelled) {
          task.cancelled = true;
          task.callback();
        }
      }
    },
  };
}

describe("tooltip lifecycle", () => {
  test("等待官方延迟后才显示", () => {
    const harness = createHarness();

    harness.lifecycle.pointerEnter();
    expect(harness.events).toEqual([]);

    harness.runScheduled();
    expect(harness.events).toEqual(["show"]);
  });

  test("指针在延迟结束前离开时取消显示", () => {
    const harness = createHarness();

    harness.lifecycle.pointerEnter();
    harness.lifecycle.pointerLeave();
    harness.runScheduled();

    expect(harness.events).toEqual(["hide"]);
  });

  test("按钮仍有焦点时 pointer leave 也必须取消显示", () => {
    const harness = createHarness();

    harness.lifecycle.pointerEnter();
    harness.lifecycle.focus();
    harness.lifecycle.pointerLeave();
    harness.runScheduled();

    expect(harness.events).toEqual(["hide"]);
  });

  test("指针仍在按钮上时 blur 也必须关闭已经显示的 tooltip", () => {
    const harness = createHarness();

    harness.lifecycle.pointerEnter();
    harness.runScheduled();
    harness.lifecycle.focus();
    harness.lifecycle.blur();

    expect(harness.events).toEqual(["show", "hide"]);
  });

  test("窗口回焦不得重放失焦前的按钮 focus", () => {
    const harness = createHarness();

    harness.lifecycle.pointerEnter();
    harness.lifecycle.focus();
    harness.runScheduled();
    harness.lifecycle.pointerLeave();
    harness.lifecycle.windowBlur();

    harness.lifecycle.windowFocus();
    harness.lifecycle.focus();
    harness.runScheduled();
    expect(harness.events).toEqual(["show", "hide", "hide"]);

    harness.lifecycle.pointerEnter();
    harness.runScheduled();
    expect(harness.events).toEqual(["show", "hide", "hide", "show"]);
  });

  test("窗口内产生新的键盘焦点后允许再次显示", () => {
    const harness = createHarness();

    harness.lifecycle.focus();
    harness.lifecycle.windowBlur();
    harness.lifecycle.blur();
    harness.lifecycle.windowFocus();
    harness.lifecycle.focus();
    harness.runScheduled();
    expect(harness.events).toEqual(["hide", "hide"]);

    harness.lifecycle.blur();
    harness.lifecycle.focus();
    harness.runScheduled();
    expect(harness.events).toEqual(["hide", "hide", "hide", "show"]);
  });

  test("延迟结束时重新确认按钮仍可显示", () => {
    const harness = createHarness(false);

    harness.lifecycle.focus();
    harness.runScheduled();

    expect(harness.events).toEqual([]);
  });

  test("收到同组 tooltip 的关闭事件时取消待显示并隐藏", () => {
    const harness = createHarness();

    harness.lifecycle.pointerEnter();
    harness.lifecycle.dismiss();
    harness.runScheduled();

    expect(harness.events).toEqual(["hide"]);
  });

  test("共享官方 provider 的快速切换延迟并登记打开与关闭", () => {
    const providerEvents: string[] = [];
    const harness = createHarness(true, {
      resolveDelay: () => 0,
      onOpen: () => providerEvents.push("activate"),
      onClose: () => providerEvents.push("deactivate"),
    });

    harness.lifecycle.pointerEnter();
    expect(harness.delays).toEqual([0]);

    harness.runScheduled();
    expect(harness.events).toEqual(["show"]);
    expect(providerEvents).toEqual(["activate"]);

    harness.lifecycle.pointerLeave();
    expect(harness.events).toEqual(["show", "hide"]);
    expect(providerEvents).toEqual(["activate", "deactivate"]);
  });

  test("trigger dismissal stays closed until pointer and focus both exit", () => {
    const harness = createHarness();

    harness.lifecycle.pointerEnter();
    harness.runScheduled();
    harness.lifecycle.trigger();
    harness.lifecycle.focus();
    harness.runScheduled();

    expect(harness.events).toEqual(["show", "hide"]);

    harness.lifecycle.pointerLeave();
    harness.lifecycle.blur();
    harness.lifecycle.pointerEnter();
    harness.runScheduled();

    expect(harness.events).toEqual(["show", "hide", "hide", "hide", "show"]);
  });
});
