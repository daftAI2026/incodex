import { describe, expect, test } from "bun:test";
import { createTooltipLifecycle } from "./tooltip-lifecycle";

type ScheduledTask = {
  callback: () => void;
  cancelled: boolean;
};

function createHarness(canShow = true) {
  const tasks = new Map<number, ScheduledTask>();
  const events: string[] = [];
  let nextId = 1;
  const lifecycle = createTooltipLifecycle({
    delayMs: 700,
    schedule(callback) {
      const id = nextId++;
      tasks.set(id, { callback, cancelled: false });
      return id;
    },
    cancel(id) {
      const task = tasks.get(id);
      if (task) task.cancelled = true;
    },
    canShow: () => canShow,
    show: () => events.push("show"),
    hide: () => events.push("hide"),
  });

  return {
    lifecycle,
    events,
    runScheduled() {
      for (const task of tasks.values()) {
        if (!task.cancelled) task.callback();
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
});
