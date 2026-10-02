type Props = Record<string, unknown>;
type Fiber = {
  type?: unknown; elementType?: unknown; stateNode?: unknown;
  memoizedProps?: Props; pendingProps?: Props;
  child?: Fiber | null; sibling?: Fiber | null; return?: Fiber | null; alternate?: Fiber | null;
};
type Component = { type: unknown; props: Props };
export type OfficialSidebarCapabilities = {
  rail: HTMLElement;
  slot: { parent: HTMLElement; before: ChildNode | null; empty?: boolean };
  button: Component; tooltip: Component; group: Component;
};

// Discovery inspects only the mounted rail, never the account/chat tree. Bounds
// protect against unknown structures and cycles, not against slow human input.
const MAX_FIBERS = 2048;
function props(f: Fiber): Props { return f.memoizedProps ?? f.pendingProps ?? {}; }
function component(f: Fiber): unknown {
  const type = f.elementType ?? f.type;
  return typeof type === "function" || (type !== null && typeof type === "object") ? type : null;
}
function element(f: Fiber): HTMLElement | null {
  const node = f.stateNode as HTMLElement | null;
  return typeof f.type === "string" && node?.nodeType === 1 ? node : null;
}
function fiberOf(el: HTMLElement): Fiber | null {
  const key = Object.keys(el).find(name => name.startsWith("__reactFiber$"));
  const fiber = key ? (el as unknown as Record<string, Fiber>)[key] : null;
  if (!fiber) return null;
  // A DOM node can retain its original Fiber while the alternate is current.
  let top = fiber;
  const visited = new Set<Fiber>();
  while (top.return && !visited.has(top) && visited.size < MAX_FIBERS) { visited.add(top); top = top.return; }
  if (top.return) return null;
  const root = top.stateNode as { current?: Fiber } | null;
  if (!root?.current || root.current.stateNode !== root || root.current.return) return null;
  // Bailouts can share an uncloned host child across root generations. Its DOM
  // ownership and bounded return chain are validated again below.
  return root.current !== top ? fiber.alternate ?? fiber : fiber;
}
function subtree(root: Fiber): Fiber[] | null {
  const result: Fiber[] = [], stack = [root], visited = new Set<Fiber>();
  while (stack.length) {
    const f = stack.pop()!;
    if (visited.has(f) || result.length >= MAX_FIBERS) return null;
    visited.add(f); result.push(f);
    const children: Fiber[] = [];
    for (let child = f.child; child; child = child.sibling) {
      if (children.includes(child) || children.length >= MAX_FIBERS) return null;
      children.push(child);
    }
    stack.push(...children.reverse());
  }
  return result;
}
function chain(f: Fiber, stop: Fiber): Fiber[] | null {
  const result: Fiber[] = [], visited = new Set<Fiber>();
  let current: Fiber | null = f;
  const boundary = (value: Fiber | null) => value === stop || (value === stop.alternate && element(value!) === element(stop));
  for (; current && !boundary(current) && !visited.has(current) && result.length < MAX_FIBERS; current = current.return ?? null) {
    visited.add(current); result.push(current);
  }
  return current && boundary(current) ? result : null;
}
export function withinOfficialRail(node: HTMLElement, rail: HTMLElement): boolean {
  const visited = new Set<HTMLElement>();
  for (let current: HTMLElement | null = node; current && !visited.has(current) && visited.size < MAX_FIBERS; current = current.parentElement) {
    if (current === rail) return true;
    visited.add(current);
  }
  return false;
}
function pick(p: Props, keys: string[]): Props {
  return Object.fromEntries(keys.filter(k => Object.hasOwn(p, k) && p[k] !== undefined).map(k => [k, p[k]]));
}
function buttonProps(p: Props): Props {
  const copied = pick(p, ["color", "variant", "pill", "size", "iconSize", "uniform", "className"]);
  if (p.style && typeof p.style === "object") {
    const style = Object.fromEntries(Object.entries(p.style).filter(([key, value]) =>
      !["transform", "translate", "rotate", "scale", "transition"].includes(key) &&
      (typeof value === "string" || typeof value === "number"),
    ));
    if (Object.keys(style).length) copied.style = style;
  }
  return copied;
}

function firstHost(f: Fiber): HTMLElement | null {
  return subtree(f)?.map(element).find(node => node !== null) ?? null;
}

export function findOfficialSidebarCapabilities(doc: Document): OfficialSidebarCapabilities | null {
  const rails = [...doc.querySelectorAll<HTMLElement>("nav[data-app-navigation-rail]")].filter(el => el.isConnected);
  if (rails.length !== 1) return null;
  const rail = rails[0]!, root = fiberOf(rail);
  const nodes = root ? subtree(root) : null;
  if (!nodes || !root) return null;
  const contexts = nodes.filter(f => component(f) && Array.isArray(props(f).items) && typeof props(f).strategy === "function");
  if (contexts.length !== 1) return null;
  const context = contexts[0]!, pins = subtree(context);
  if (!pins) return null;
  const isGroup = (f: Fiber) => component(f) && typeof props(f).itemSpacing === "string";
  const groups = pins.filter(isGroup);
  if (groups.length > 1) return null;
  let group: Fiber | undefined = groups[0];
  let slot: OfficialSidebarCapabilities["slot"];
  if (group) {
    const parent = firstHost(group);
    if (!parent?.isConnected || !withinOfficialRail(parent, rail)) return null;
    // The managed root is not part of the host Fiber tree. Never treat it as
    // an official destination, nor return it as its own insertion boundary.
    const before = [...parent.childNodes].find(node =>
      !(node.nodeType === 1 && (node as Element).hasAttribute("data-incodex-sidebar-host")),
    ) ?? null;
    slot = { parent, before };
  } else {
    if (pins.some(f => element(f))) return null;
    const ancestors = chain(context, root);
    if (!ancestors) return null;
    group = ancestors.find(isGroup);
    if (!group) return null;
    const parentFiber = ancestors.find(f => element(f));
    const parent = parentFiber ? element(parentFiber) : null;
    if (!parent?.isConnected || !withinOfficialRail(parent, rail)) return null;
    let before: HTMLElement | null = null;
    for (const f of ancestors) {
      if (f === parentFiber) break;
      for (let next = f.sibling, count = 0; next && count < MAX_FIBERS; next = next.sibling, count++) {
        const node = firstHost(next);
        if (node?.parentElement === parent) { before = node; break; }
      }
      if (before) break;
    }
    slot = { parent, before, empty: true };
  }
  const rows = (groups.length ? pins : nodes).filter(f => element(f)?.hasAttribute("data-sidebar-destination"));
  const pinNodes = new Set(pins);
  const visualKeys = ["variant", "size", "iconSize", "uniform", "color", "pill"];
  const baseButtons = nodes.filter(f => !pinNodes.has(f) && element(f)?.hasAttribute("data-sidebar-destination"))
    .flatMap(row => {
      const native = chain(row, root)?.find(f => component(f) &&
        visualKeys.slice(0, -1).every(k => Object.hasOwn(props(f), k)));
      return native ? [native] : [];
    });
  for (const row of rows) {
    const ancestors = chain(row, root);
    if (!ancestors) return null;
    const button = ancestors.find(f => component(f) && ["variant", "size", "iconSize", "uniform", "color"].every(k => Object.hasOwn(props(f), k)));
    const tooltip = ancestors.filter(f => component(f) && Object.hasOwn(props(f), "tooltipContent") && props(f).cloneCustomTrigger === true).at(-1);
    if (!button || !tooltip) continue;
    const visual = buttonProps(props(button));
    if (typeof visual.className === "string") {
      // A sortable row adds drag-state classes. Read the same native Button's
      // non-sortable base classes instead of hardcoding or filtering host CSS.
      const bases = baseButtons.filter(f => component(f) === component(button) &&
        visualKeys.every(k => props(f)[k] === props(button)[k]));
      const classes = new Set(bases.map(f => props(f).className).filter(v => typeof v === "string"));
      if (classes.size !== 1) return null;
      visual.className = [...classes][0];
    }
    // Empty slots reuse the native Group's spacing/appearance, omitting the
    // outer scroll container's className so a second scroller is not created.
    const groupProps = pick(props(group), slot.empty ? ["itemSpacing", "appearance"] : ["className", "itemSpacing", "appearance"]);
    return {
      rail, slot,
      button: { type: component(button), props: visual },
      tooltip: { type: component(tooltip), props: pick(props(tooltip), ["side", "align", "sideOffset", "alignOffset", "cloneCustomTrigger", "closeOnTriggerClick", "delayDuration", "disableHoverOpen", "hoverGroupKey"]) },
      group: { type: component(group), props: groupProps },
    };
  }
  return null;
}
