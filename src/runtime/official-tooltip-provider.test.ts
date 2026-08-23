import { describe, expect, test } from "bun:test";
import {
  findOfficialTooltipProvider,
  type OfficialTooltipProvider,
} from "./official-tooltip-provider";

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
    deactivateTooltip: () => {},
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

  test("rejects a context that cannot participate in both sides of provider timing", () => {
    const trigger = triggerWithFiber({
      dependencies: {
        firstContext: {
          memoizedValue: {
            getOpenDelay: () => 0,
            activateTooltip: () => {},
          },
        },
      },
    });

    expect(findOfficialTooltipProvider(trigger)).toBeNull();
  });
});
