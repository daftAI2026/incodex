import { describe, expect, test } from "bun:test";
import {
  createOfficialTooltipTimingBridge,
  findOfficialTooltipProvider,
  type OfficialTooltipProvider,
} from "./official-tooltip-provider.ts";

type TestFiber = {
  type?: unknown;
  memoizedProps?: Record<string, unknown> | null;
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

function officialTooltipComponentFromFirstBuild(props: Record<string, unknown>): unknown {
  const { delayDuration, delayOpen, getDelayDuration, skipDelayKey, tooltipContent, variant } = props;
  const provider = {
    getOpenDelay: (_key: unknown, delay: unknown) => delay,
    activateTooltip: (...args: unknown[]) => args,
  };
  const delay = delayOpen ? 250 : delayDuration;
  return [
    provider.getOpenDelay(skipDelayKey, delay),
    provider.activateTooltip(tooltipContent, skipDelayKey, variant, getDelayDuration),
  ];
}

function differentlyNamedTooltipComponentFromAnotherBuild(props: Record<string, unknown>): unknown {
  const { delayDuration, delayOpen, getDelayDuration, skipDelayKey, tooltipContent, variant } = props;
  const provider = {
    getOpenDelay: (_key: unknown, delay: unknown) => delay,
    activateTooltip: (...args: unknown[]) => args,
  };
  const delay = delayOpen ? 250 : delayDuration;
  return [
    provider.getOpenDelay(skipDelayKey, delay),
    provider.activateTooltip(tooltipContent, skipDelayKey, variant, getDelayDuration),
  ];
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

  test("reuses the current tooltip's delay and group key for delay lookup and activation", () => {
    const events: string[] = [];
    let warmGroup: string | null = null;
    const expected = {
      ...provider(),
      getOpenDelay: (key: string, fallbackMs: number) => {
        events.push(`delay:${key}:${fallbackMs}`);
        return warmGroup === key ? 0 : fallbackMs;
      },
      activateTooltip: (id: string, key: string, variant: string) => {
        events.push(`activate:${id}:${key}:${variant}`);
        warmGroup = key;
      },
    } satisfies OfficialTooltipProvider;
    const searchTooltip: TestFiber = {
      type: officialTooltipComponentFromFirstBuild,
      memoizedProps: {
        children: {},
        delayDuration: 200,
        skipDelayKey: "search",
        tooltipContent: "Search",
        variant: "tooltip",
      },
      dependencies: { firstContext: { memoizedValue: expected } },
    };
    const trigger = triggerWithFiber({ return: searchTooltip });
    const bridge = createOfficialTooltipTimingBridge(() => trigger);

    expect(bridge.resolveDelay(700)).toBe(200);
    bridge.activate(() => {});
    expect(bridge.resolveDelay(700)).toBe(0);

    expect(events).toEqual([
      "delay:search:200",
      "activate:incodex-privacy-toggle:search:tooltip",
      "delay:search:200",
    ]);
  });

  test("ignores delayDuration on unrelated ancestors and uses the local fallback", () => {
    const calls: Array<[string, number]> = [];
    const expected = {
      ...provider(),
      getOpenDelay: (key: string, fallbackMs: number) => {
        calls.push([key, fallbackMs]);
        return fallbackMs;
      },
    } satisfies OfficialTooltipProvider;
    const trigger = triggerWithFiber({
      return: {
        type: function searchPanel(props: Record<string, unknown>) {
          return props.delayDuration;
        },
        memoizedProps: { delayDuration: 200 },
        dependencies: { firstContext: { memoizedValue: expected } },
      },
    });
    const bridge = createOfficialTooltipTimingBridge(() => trigger);

    expect(bridge.resolveDelay(700)).toBe(700);
    expect(calls).toEqual([["default", 700]]);
  });

  test("falls back when the trigger fiber has ambiguous tooltip parameters", () => {
    const calls: Array<[string, number]> = [];
    const expected = {
      ...provider(),
      getOpenDelay: (key: string, fallbackMs: number) => {
        calls.push([key, fallbackMs]);
        return fallbackMs;
      },
    } satisfies OfficialTooltipProvider;
    const tooltipProps = (delayDuration: number) => ({
      children: {},
      delayDuration,
      tooltipContent: "Search",
    });
    const trigger = triggerWithFiber({
      return: {
        type: officialTooltipComponentFromFirstBuild,
        memoizedProps: tooltipProps(200),
        dependencies: { firstContext: { memoizedValue: expected } },
        return: {
          type: officialTooltipComponentFromFirstBuild,
          memoizedProps: tooltipProps(250),
          dependencies: { firstContext: { memoizedValue: expected } },
        },
      },
    });
    const bridge = createOfficialTooltipTimingBridge(() => trigger);

    expect(bridge.resolveDelay(700)).toBe(700);
    expect(calls).toEqual([["default", 700]]);
  });

  test("falls back when delayOpen could override the actual provider argument", () => {
    const calls: Array<[string, number]> = [];
    const expected = {
      ...provider(),
      getOpenDelay: (key: string, fallbackMs: number) => {
        calls.push([key, fallbackMs]);
        return fallbackMs;
      },
    } satisfies OfficialTooltipProvider;
    const trigger = triggerWithFiber({
      return: {
        type: officialTooltipComponentFromFirstBuild,
        memoizedProps: {
          children: {},
          delayDuration: 200,
          delayOpen: true,
          tooltipContent: "Search",
        },
        dependencies: { firstContext: { memoizedValue: expected } },
      },
    });
    const bridge = createOfficialTooltipTimingBridge(() => trigger);

    expect(bridge.resolveDelay(700)).toBe(700);
    expect(calls).toEqual([["default", 700]]);
  });

  test("recognizes tooltip semantics without relying on the bundled function name", () => {
    for (const componentType of [
      officialTooltipComponentFromFirstBuild,
      differentlyNamedTooltipComponentFromAnotherBuild,
    ]) {
      const calls: Array<[string, number]> = [];
      const expected = {
        ...provider(),
        getOpenDelay: (key: string, fallbackMs: number) => {
          calls.push([key, fallbackMs]);
          return fallbackMs;
        },
      } satisfies OfficialTooltipProvider;
      const trigger = triggerWithFiber({
        return: {
          type: componentType,
          memoizedProps: {
            children: {},
            delayDuration: 200,
            skipDelayKey: "search",
            tooltipContent: "Search",
          },
          dependencies: { firstContext: { memoizedValue: expected } },
        },
      });
      const bridge = createOfficialTooltipTimingBridge(() => trigger);

      expect(bridge.resolveDelay(700)).toBe(200);
      expect(calls).toEqual([["search", 200]]);
    }
  });

  test("uses the official default group when the tooltip omits skipDelayKey", () => {
    const calls: Array<[string, number]> = [];
    const expected = {
      ...provider(),
      getOpenDelay: (key: string, fallbackMs: number) => {
        calls.push([key, fallbackMs]);
        return fallbackMs;
      },
    } satisfies OfficialTooltipProvider;
    const trigger = triggerWithFiber({
      return: {
        type: differentlyNamedTooltipComponentFromAnotherBuild,
        memoizedProps: {
          children: {},
          delayDuration: 200,
          tooltipContent: "Search",
        },
        dependencies: { firstContext: { memoizedValue: expected } },
      },
    });
    const bridge = createOfficialTooltipTimingBridge(() => trigger);

    expect(bridge.resolveDelay(700)).toBe(200);
    expect(calls).toEqual([["default", 200]]);
  });

  test("falls back when a tooltip adds an event-dependent delay callback", () => {
    const calls: Array<[string, number]> = [];
    const expected = {
      ...provider(),
      getOpenDelay: (key: string, fallbackMs: number) => {
        calls.push([key, fallbackMs]);
        return fallbackMs;
      },
    } satisfies OfficialTooltipProvider;
    const trigger = triggerWithFiber({
      return: {
        type: officialTooltipComponentFromFirstBuild,
        memoizedProps: {
          children: {},
          delayDuration: 200,
          getDelayDuration: () => 125,
          skipDelayKey: "search",
          tooltipContent: "Search",
        },
        dependencies: { firstContext: { memoizedValue: expected } },
      },
    });
    const bridge = createOfficialTooltipTimingBridge(() => trigger);

    expect(bridge.resolveDelay(700)).toBe(700);
    expect(calls).toEqual([["default", 700]]);
  });
});
