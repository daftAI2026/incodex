export type OfficialTooltipProvider = {
  getOpenDelay: (key: string, fallbackMs: number) => number;
  activateTooltip: (
    id: string,
    key: string,
    variant: string,
    close: () => void,
  ) => void;
  clearHoverHandoffLock: (id: string) => void;
  deactivateTooltip: (id: string) => void;
  isHoverOpenBlocked: (id: string) => boolean;
  registerOpenTooltip: (id: string, variant: string, close: () => void) => () => void;
  registerTooltipDismissHandler: (id: string, close: () => void) => () => void;
  setHoverHandoffLockTooltipId: (id: string) => void;
};

export type OfficialTooltipTimingBridge = {
  resolveDelay: (fallbackMs: number) => number;
  activate: (close: () => void) => void;
  deactivate: () => void;
};

export function createOfficialTooltipTimingBridge(
  currentTrigger: () => HTMLElement | null,
): OfficialTooltipTimingBridge {
  let activeProvider: OfficialTooltipProvider | null = null;

  function currentProvider(): OfficialTooltipProvider | null {
    const trigger = currentTrigger();
    if (!trigger) return null;
    return findOfficialTooltipProvider(trigger);
  }

  function deactivate(): void {
    const provider = activeProvider;
    activeProvider = null;
    if (!provider) return;
    try {
      provider.deactivateTooltip(PROVIDER_ID);
    } catch {
      /* 官方内部结构变化时保持本地降级，不传播异常。 */
    }
  }

  return {
    resolveDelay(fallbackMs) {
      try {
        const delayMs = currentProvider()?.getOpenDelay(PROVIDER_KEY, fallbackMs) ?? fallbackMs;
        if (!Number.isFinite(delayMs) || delayMs < 0) return fallbackMs;
        return delayMs;
      } catch {
        return fallbackMs;
      }
    },
    activate(close) {
      deactivate();
      const provider = currentProvider();
      if (!provider) return;
      try {
        provider.activateTooltip(PROVIDER_ID, PROVIDER_KEY, PROVIDER_VARIANT, close);
        activeProvider = provider;
      } catch {
        try {
          provider.deactivateTooltip(PROVIDER_ID);
        } catch {
          /* 官方内部结构变化时保持本地降级，不传播异常。 */
        }
      }
    },
    deactivate,
  };
}

type ReactContextDependency = {
  memoizedValue?: unknown;
  next?: ReactContextDependency | null;
};

type ReactFiber = {
  return?: ReactFiber | null;
  dependencies?: {
    firstContext?: ReactContextDependency | null;
  } | null;
};

const REACT_FIBER_PREFIX = "__reactFiber$";
const MAX_FIBER_DEPTH = 64;
const MAX_CONTEXTS_PER_FIBER = 64;
const PROVIDER_ID = "incodex-privacy-toggle";
const PROVIDER_KEY = "default";
const PROVIDER_VARIANT = "tooltip";

function isOfficialTooltipProvider(value: unknown): value is OfficialTooltipProvider {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Partial<OfficialTooltipProvider>;
  return (
    typeof candidate.getOpenDelay === "function" &&
    typeof candidate.activateTooltip === "function" &&
    typeof candidate.clearHoverHandoffLock === "function" &&
    typeof candidate.deactivateTooltip === "function" &&
    typeof candidate.isHoverOpenBlocked === "function" &&
    typeof candidate.registerOpenTooltip === "function" &&
    typeof candidate.registerTooltipDismissHandler === "function" &&
    typeof candidate.setHoverHandoffLockTooltipId === "function"
  );
}

function reactFiber(trigger: HTMLElement): ReactFiber | null {
  const key = Object.keys(trigger).find((name) => name.startsWith(REACT_FIBER_PREFIX));
  if (!key) return null;
  return (trigger as unknown as Record<string, ReactFiber | undefined>)[key] ?? null;
}

export function findOfficialTooltipProvider(trigger: HTMLElement): OfficialTooltipProvider | null {
  let fiber = reactFiber(trigger);
  for (let fiberDepth = 0; fiber && fiberDepth < MAX_FIBER_DEPTH; fiberDepth += 1) {
    let context = fiber.dependencies?.firstContext;
    for (
      let contextIndex = 0;
      context && contextIndex < MAX_CONTEXTS_PER_FIBER;
      contextIndex += 1
    ) {
      if (isOfficialTooltipProvider(context.memoizedValue)) return context.memoizedValue;
      context = context.next;
    }
    fiber = fiber.return ?? null;
  }
  return null;
}
