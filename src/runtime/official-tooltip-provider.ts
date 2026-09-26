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

  function currentTarget(): { trigger: HTMLElement; provider: OfficialTooltipProvider } | null {
    const trigger = currentTrigger();
    if (!trigger) return null;
    const provider = findOfficialTooltipProvider(trigger);
    return provider ? { trigger, provider } : null;
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
        const target = currentTarget();
        if (!target) return fallbackMs;
        const invocation = actualTooltipInvocation(target.trigger, target.provider);
        const delayMs =
          target.provider.getOpenDelay(
            invocation?.groupKey ?? PROVIDER_KEY,
            invocation?.delayMs ?? fallbackMs,
          ) ?? fallbackMs;
        if (!Number.isFinite(delayMs) || delayMs < 0) return fallbackMs;
        return delayMs;
      } catch {
        return fallbackMs;
      }
    },
    activate(close) {
      deactivate();
      const target = currentTarget();
      if (!target) return;
      try {
        const invocation = actualTooltipInvocation(target.trigger, target.provider);
        target.provider.activateTooltip(
          PROVIDER_ID,
          invocation?.groupKey ?? PROVIDER_KEY,
          invocation?.variant ?? PROVIDER_VARIANT,
          close,
        );
        activeProvider = target.provider;
      } catch {
        try {
          target.provider.deactivateTooltip(PROVIDER_ID);
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
  type?: unknown;
  elementType?: unknown;
  memoizedProps?: unknown;
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
const OFFICIAL_TOOLTIP_SOURCE_MARKERS = [
  "activateTooltip",
  "delayDuration",
  "delayOpen",
  "getDelayDuration",
  "getOpenDelay",
  "skipDelayKey",
  "tooltipContent",
] as const;

type OfficialTooltipInvocation = {
  groupKey: string;
  delayMs: number;
  variant: string;
};

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

function isOfficialTooltipTimingFiber(fiber: ReactFiber): boolean {
  for (const componentType of [fiber.type, fiber.elementType]) {
    if (typeof componentType !== "function") continue;
    try {
      const source = Function.prototype.toString.call(componentType);
      if (OFFICIAL_TOOLTIP_SOURCE_MARKERS.every((marker) => source.includes(marker))) return true;
    } catch {
      /* 无法核实当前组件实现时，使用本地延迟降级。 */
    }
  }
  return false;
}

function consumesProvider(fiber: ReactFiber, provider: OfficialTooltipProvider): boolean {
  let context = fiber.dependencies?.firstContext;
  for (let contextIndex = 0; context && contextIndex < MAX_CONTEXTS_PER_FIBER; contextIndex += 1) {
    if (context.memoizedValue === provider) return true;
    context = context.next;
  }
  return false;
}

function isValidDelay(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function invocationFromTooltipFiber(fiber: ReactFiber): OfficialTooltipInvocation | null {
  if (typeof fiber.memoizedProps !== "object" || fiber.memoizedProps === null) return null;
  const props = fiber.memoizedProps as Record<string, unknown>;
  if (!("children" in props) || props.tooltipContent == null) return null;

  const delayDuration = props.delayDuration;
  const groupKey = props.skipDelayKey === undefined ? PROVIDER_KEY : props.skipDelayKey;
  const variant = props.variant === undefined ? PROVIDER_VARIANT : props.variant;
  if (
    !isValidDelay(delayDuration) ||
    typeof groupKey !== "string" ||
    typeof variant !== "string" ||
    variant.length === 0 ||
    props.delayOpen ||
    (props.getDelayDuration != null)
  ) {
    return null;
  }

  return { groupKey, delayMs: delayDuration, variant };
}

function actualTooltipInvocation(
  trigger: HTMLElement,
  provider: OfficialTooltipProvider,
): OfficialTooltipInvocation | null {
  let fiber = reactFiber(trigger);
  let matchingTooltip: ReactFiber | null = null;
  for (let fiberDepth = 0; fiber && fiberDepth < MAX_FIBER_DEPTH; fiberDepth += 1) {
    if (isOfficialTooltipTimingFiber(fiber) && consumesProvider(fiber, provider)) {
      if (matchingTooltip) return null;
      matchingTooltip = fiber;
    }
    fiber = fiber.return ?? null;
  }
  return matchingTooltip ? invocationFromTooltipFiber(matchingTooltip) : null;
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
