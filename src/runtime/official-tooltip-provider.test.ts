import { describe, expect, test } from "bun:test";
import {
  createOfficialTooltipTimingBridge,
  findOfficialTooltipProvider,
  type OfficialTooltipProvider,
} from "./official-tooltip-provider.ts";

type TestFiber = {
  return?: TestFiber | null;
  dependencies?: {
    firstContext?: TestContext | null;
  } | null;
};

type TestContext = {
  memoizedValue?: unknown;
  next?: TestContext | null;
};

function triggerWithFiber(fiber: TestFiber): HTMLElement {
  return { "__reactFiber$build-specific": fiber } as unknown as HTMLElement;
}

function provider(): OfficialTooltipProvider {
  return {
    getOpenDelay: (_key, fallbackMs) => fallbackMs,
    activateTooltip: () => {},
    clearHoverHandoffLock: () => {},
    deactivateTooltip: () => {},
    isHoverOpenBlocked: () => false,
    registerOpenTooltip: () => () => {},
    registerTooltipDismissHandler: () => () => {},
    setHoverHandoffLockTooltipId: () => {},
  };
}

describe("official tooltip provider discovery", () => {
  test("discovers the provider by capability through the trigger fiber context", () => {
    const expected = provider();
    const trigger = triggerWithFiber({
      return: {
        dependencies: {
          firstContext: {
            memoizedValue: { locale: "zh-CN" },
            next: { memoizedValue: expected },
          },
        },
      },
    });

    expect(findOfficialTooltipProvider(trigger)).toBe(expected);
  });

  test("rejects a same-shaped context without the provider's complete timing capabilities", () => {
    const trigger = triggerWithFiber({
      dependencies: {
        firstContext: {
          memoizedValue: {
            getOpenDelay: () => 0,
            activateTooltip: () => {},
            deactivateTooltip: () => {},
          },
        },
      },
    });

    expect(findOfficialTooltipProvider(trigger)).toBeNull();
  });

  test("rediscovers the current provider but deactivates the instance it activated", () => {
    const events: string[] = [];
    const first = {
      ...provider(),
      getOpenDelay: () => 0,
      activateTooltip: () => events.push("activate:first"),
      deactivateTooltip: () => events.push("deactivate:first"),
    } satisfies OfficialTooltipProvider;
    const second = {
      ...provider(),
      getOpenDelay: () => 25,
      activateTooltip: () => events.push("activate:second"),
      deactivateTooltip: () => events.push("deactivate:second"),
    } satisfies OfficialTooltipProvider;
    let trigger = triggerWithFiber({ dependencies: { firstContext: { memoizedValue: first } } });
    const bridge = createOfficialTooltipTimingBridge(() => trigger);

    expect(bridge.resolveDelay(700)).toBe(0);
    bridge.activate(() => {});
    trigger = triggerWithFiber({ dependencies: { firstContext: { memoizedValue: second } } });
    bridge.deactivate();
    expect(bridge.resolveDelay(700)).toBe(25);
    bridge.activate(() => {});
    bridge.deactivate();

    expect(events).toEqual([
      "activate:first",
      "deactivate:first",
      "activate:second",
      "deactivate:second",
    ]);
  });
});
