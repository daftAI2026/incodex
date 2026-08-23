export type OfficialTooltipProvider = {
  getOpenDelay: (key: string, fallbackMs: number) => number;
  activateTooltip: (
    id: string,
    key: string,
    variant: string,
    close: () => void,
  ) => void;
  deactivateTooltip: (id: string) => void;
};

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

function isOfficialTooltipProvider(value: unknown): value is OfficialTooltipProvider {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Partial<OfficialTooltipProvider>;
  return (
    typeof candidate.getOpenDelay === "function" &&
    typeof candidate.activateTooltip === "function" &&
    typeof candidate.deactivateTooltip === "function"
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
