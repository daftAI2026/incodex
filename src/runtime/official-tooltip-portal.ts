/**
 * [INPUT]: 依赖 Search Fiber 的官方 Portal Context 与共享 React Root。
 * [OUTPUT]: 导出 Portal Context 发现器和 owner 变更信号。
 * [POS]: Runtime Tooltip Portal 边界；owner 变更后只请求一次 Root 重渲染。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
type Fiber = {
  type?: unknown;
  elementType?: unknown;
  memoizedProps?: unknown;
  pendingProps?: unknown;
  return?: Fiber | null;
};
type ContextProvider = { Provider?: unknown; _context?: unknown };
type ProviderOwner = { context: object; provider: unknown; value: Record<string, unknown> };
type Root = { render(element: unknown): void; unmount(): void };
type ElementFactory = (type: unknown, props: Record<string, unknown>) => unknown;
type ProviderSearch = () => HTMLElement | null;

const MAX_FIBER_DEPTH = 2048;
const PORTAL_CONTAINER_PROP = "radixPortalContainer";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function contextOfProvider(type: unknown): object | null {
  if (!isRecord(type)) return null;
  const candidate = type as ContextProvider;
  const context = isRecord(candidate._context) ? candidate._context : type;
  return isRecord(context) && (context as ContextProvider).Provider === type ? context : null;
}

function fiberOf(element: HTMLElement): Fiber | null {
  const key = Object.keys(element).find((name) => name.startsWith("__reactFiber$"));
  return key ? (element as unknown as Record<string, Fiber | undefined>)[key] ?? null : null;
}

function providerFibers(search: HTMLElement): Array<ProviderOwner | { context: object; provider: unknown }> {
  const providers: Array<ProviderOwner | { context: object; provider: unknown }> = [];
  const seen = new Set<Fiber>();
  for (let fiber = fiberOf(search), depth = 0;
    fiber && !seen.has(fiber) && depth < MAX_FIBER_DEPTH;
    fiber = fiber.return ?? null, depth += 1) {
    seen.add(fiber);
    const provider = fiber.elementType ?? fiber.type;
    const context = contextOfProvider(provider);
    if (!context) continue;
    const props = fiber.memoizedProps ?? fiber.pendingProps;
    if (!isRecord(props) || !Object.hasOwn(props, "value")) continue;
    const value = props.value;
    providers.push(isRecord(value) && Object.hasOwn(value, PORTAL_CONTAINER_PROP)
      ? { context, provider, value }
      : { context, provider });
  }
  return providers;
}

function validOwner(provider: ProviderOwner | { context: object; provider: unknown }): provider is ProviderOwner {
  if (!("value" in provider)) return false;
  const container = provider.value[PORTAL_CONTAINER_PROP];
  return isRecord(container) && container.nodeType === 1 && container.isConnected === true;
}

function findNearestPortalProvider(search: HTMLElement, context: object): ProviderOwner | null {
  const nearest = providerFibers(search).find((provider) => provider.context === context);
  return nearest && validOwner(nearest) ? nearest : null;
}

export function discoverOfficialTooltipPortalContext(search: HTMLElement): object {
  const contexts = new Set(providerFibers(search)
    .filter((provider): provider is ProviderOwner => "value" in provider)
    .map((provider) => provider.context));
  if (contexts.size !== 1) throw new Error("Official Tooltip portal context is unavailable or ambiguous");
  const context = [...contexts][0]!;
  if (!findNearestPortalProvider(search, context)) {
    throw new Error("Official Tooltip portal context is unavailable or ambiguous");
  }
  return context;
}

export function createOfficialTooltipPortalRoot(
  root: Root,
  createElement: ElementFactory,
  currentSearch: ProviderSearch,
  context: object,
) {
  const currentOwner = () => {
    const search = currentSearch();
    return search ? findNearestPortalProvider(search, context) : null;
  };
  let hasRenderIntent = false;
  let renderedOwner: Pick<ProviderOwner, "provider" | "value"> | null = null;
  return {
    ready: () => currentOwner() !== null,
    needsRender() {
      if (!hasRenderIntent) return false;
      const owner = currentOwner();
      return owner
        ? renderedOwner === null || renderedOwner.provider !== owner.provider || renderedOwner.value !== owner.value
        : renderedOwner !== null;
    },
    render(element: unknown) {
      if (element === null) {
        hasRenderIntent = false;
        renderedOwner = null;
        root.render(null);
        return;
      }
      hasRenderIntent = true;
      const owner = currentOwner();
      if (!owner) {
        root.render(null);
        renderedOwner = null;
        return;
      }
      root.render(createElement(owner.provider, { value: owner.value, children: element }));
      renderedOwner = { provider: owner.provider, value: owner.value };
    },
    unmount() {
      hasRenderIntent = false;
      renderedOwner = null;
      root.unmount();
    },
  };
}
