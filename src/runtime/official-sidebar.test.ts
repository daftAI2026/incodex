import { describe, expect, test } from "bun:test";
import { findOfficialSidebarCapabilities } from "./official-sidebar.ts";

type Props = Record<string, unknown>;
type Fiber = {
  type: unknown;
  tag: number;
  memoizedProps: Props;
  pendingProps: Props;
  stateNode: unknown;
  child: Fiber | null;
  sibling: Fiber | null;
  return: Fiber | null;
  alternate?: Fiber;
};

const strategy = () => null;
const oldAction = () => undefined;
function RailComponent() {}
function HostRootComponent() {}
function NavListComponent() {}
function DndContextComponent() {}
function SortableContextComponent() {}
function DestinationComponent() {}
function ContextMenuComponent() {}
function TooltipComponent() {}
function StyledButtonComponent() {}
function MoreComponent() {}

class FixtureElement {
  readonly nodeType = 1;
  readonly tagName: string;
  readonly childNodes: FixtureElement[] = [];
  parentElement: FixtureElement | null = null;
  isConnected = true;
  private readonly attributes = new Map<string, string>();

  constructor(tagName: string, attributes: Record<string, string> = {}) {
    this.tagName = tagName.toUpperCase();
    for (const [name, value] of Object.entries(attributes)) this.attributes.set(name, value);
  }

  get firstChild(): FixtureElement | null { return this.childNodes[0] ?? null; }
  get parentNode(): FixtureElement | null { return this.parentElement; }
  getAttribute(name: string): string | null { return this.attributes.get(name) ?? null; }
  hasAttribute(name: string): boolean { return this.attributes.has(name); }
  appendChild(child: FixtureElement): FixtureElement {
    if (child.parentElement) {
      const oldChildren = child.parentElement.childNodes;
      const oldIndex = oldChildren.indexOf(child);
      if (oldIndex >= 0) oldChildren.splice(oldIndex, 1);
    }
    child.parentElement = this;
    this.childNodes.push(child);
    return child;
  }
}

function matchesSelector(node: FixtureElement, selector: string): boolean {
  const match = /^(?:([a-z][\w-]*))?(?:\[([\w-]+)(?:=(?:"([^"]*)"|'([^']*)'|([^\]]+)))?\])?$/iu.exec(selector.trim());
  if (!match || (!match[1] && !match[2])) return false;
  if (match[1] && node.tagName.toLowerCase() !== match[1].toLowerCase()) return false;
  if (!match[2]) return true;
  const value = node.getAttribute(match[2]);
  const expected = match[3] ?? match[4] ?? match[5];
  return value !== null && (expected === undefined || value === expected.trim());
}

class FixtureDocument {
  constructor(private readonly root: FixtureElement) {}

  querySelectorAll<T extends Element = Element>(selector: string): NodeListOf<T> {
    const found: FixtureElement[] = [];
    const visit = (node: FixtureElement) => {
      if (matchesSelector(node, selector)) found.push(node);
      for (const child of node.childNodes) visit(child);
    };
    visit(this.root);
    return found as unknown as NodeListOf<T>;
  }
}

function fiber(type: unknown, props: Props = {}, stateNode: unknown = null, tag = 0): Fiber {
  return { type, tag, memoizedProps: props, pendingProps: props, stateNode, child: null, sibling: null, return: null };
}

function children(parent: Fiber, ...nodes: Fiber[]): void {
  parent.child = nodes[0] ?? null;
  nodes.forEach((node, index) => {
    node.return = parent;
    node.sibling = nodes[index + 1] ?? null;
  });
}

function host(node: FixtureElement, props: Props = {}): Fiber {
  const result = fiber(node.tagName.toLowerCase(), props, node, 5);
  Object.defineProperty(node, "__reactFiber$fixture", { configurable: true, enumerable: true, value: result });
  return result;
}

function fiberOn(node: FixtureElement): Fiber {
  const key = Object.keys(node).find(name => name.startsWith("__reactFiber$"));
  const result = key ? (node as unknown as Record<string, Fiber>)[key] : null;
  if (!result) throw new Error("Fixture element has no host Fiber");
  return result;
}

function ancestorFiber(start: Fiber, type: unknown): Fiber | null {
  const visited = new Set<Fiber>();
  for (let current: Fiber | null = start; current && !visited.has(current); current = current.return ?? null) {
    visited.add(current);
    if (current.type === type) return current;
  }
  return null;
}

function installCurrentRootWithBailedOutNav(nav: FixtureElement): void {
  const oldNav = fiberOn(nav);
  const oldRail = oldNav.return;
  if (!oldRail) throw new Error("Fixture nav must have its rail parent");

  const rootState: { current: Fiber | null } = { current: null };
  const oldRoot = fiber(HostRootComponent, {}, rootState, 3);
  const newRoot = fiber(HostRootComponent, {}, rootState, 3);
  const newRail = fiber(oldRail.type, oldRail.memoizedProps, oldRail.stateNode, oldRail.tag);
  const newNav = fiber(oldNav.type, oldNav.memoizedProps, nav, oldNav.tag);

  oldRoot.child = oldRail;
  oldRail.return = oldRoot;
  newRoot.child = newRail;
  newRail.return = newRoot;
  newRail.child = newNav;
  newNav.return = newRail;

  // React can bail out of the nav subtree and reuse its old child Fibers.
  // Their return pointers still name oldNav even though current traversal starts
  // at newNav.
  newNav.child = oldNav.child;

  oldRoot.alternate = newRoot;
  newRoot.alternate = oldRoot;
  oldRail.alternate = newRail;
  newRail.alternate = oldRail;
  oldNav.alternate = newNav;
  newNav.alternate = oldNav;
  rootState.current = newRoot;
}

function installCurrentRootSharingMountedNav(nav: FixtureElement): void {
  const mountedNav = fiberOn(nav);
  const mountedRail = mountedNav.return;
  if (!mountedRail) throw new Error("Fixture nav must have its rail parent");

  const rootState: { current: Fiber | null } = { current: null };
  const oldRoot = fiber(HostRootComponent, {}, rootState, 3);
  const newRoot = fiber(HostRootComponent, {}, rootState, 3);
  oldRoot.child = mountedRail;
  mountedRail.return = oldRoot;
  // This commit reuses the mounted rail/nav Fibers directly, so the nav has no
  // alternate and its return ancestry still ends at oldRoot.
  newRoot.child = mountedRail;
  oldRoot.alternate = newRoot;
  newRoot.alternate = oldRoot;
  rootState.current = newRoot;
}

type BuildOptions = {
  pins?: string[];
  rails?: number;
  sortableContexts?: number;
  buttonVisual?: Props;
  tooltipPosition?: Props;
  groupLayout?: Props;
  pinButtonClass?: string;
  primaryButtonClass?: string;
  omit?: Array<"button" | "tooltip" | "group">;
  unrelatedSortable?: boolean;
};

function buildFixture(options: BuildOptions = {}) {
  const pins = options.pins ?? ["plugin:incodex", "builtin:agents"];
  const railCount = options.rails ?? 1;
  const root = new FixtureElement("html");
  const body = root.appendChild(new FixtureElement("body"));
  let first: {
    body: FixtureElement;
    nav: FixtureElement;
    outerList: FixtureElement;
    pinList: FixtureElement | null;
    firstPin: FixtureElement | null;
    more: FixtureElement;
    sortables: Fiber[];
    nativeButtonType: unknown;
    tooltipType: unknown;
    groupType: unknown;
    buttonVisual: Props;
    tooltipPosition: Props;
    groupLayout: Props;
  } | null = null;

  const buttonVisual = options.buttonVisual ?? {
    color: "secondary", variant: "ghost", pill: false, size: "xl", iconSize: "lg", uniform: true,
  };
  const tooltipPosition = options.tooltipPosition ?? { side: "right", sideOffset: 4, align: "center" };
  const groupLayout = options.groupLayout ?? { itemSpacing: "rail", className: "host-pin-layout-v1" };
  const omitted = new Set(options.omit ?? []);

  for (let railIndex = 0; railIndex < railCount; railIndex += 1) {
    const nav = new FixtureElement("nav", { "data-app-navigation-rail": "true" });
    const header = nav.appendChild(new FixtureElement("header"));
    const scroll = nav.appendChild(new FixtureElement("div", { "data-testid": "scroll-region" }));
    const outerList = scroll.appendChild(new FixtureElement("div", { "data-appearance": "plain" }));
    const home = outerList.appendChild(new FixtureElement("div", { "data-testid": "home-row" }));
    const primaryDestination = makeDestination("builtin:codex", false, options, omitted);
    outerList.appendChild(primaryDestination.button);
    const explore = outerList.appendChild(new FixtureElement("div", { "data-testid": "explore-row" }));
    const more = new FixtureElement("div", { "data-testid": "more-row" });

    const rail = fiber(RailComponent, { availableDestinations: [], primaryDestinations: ["builtin:codex"] });
    const navFiber = host(nav, { "data-app-navigation-rail": "true" });
    children(rail, navFiber);
    const headerFiber = host(header);
    const scrollFiber = host(scroll);
    const outerProps: Props = {
      ...(omitted.has("group") ? {} : { itemSpacing: "rail" }),
      className: "host-scroll-layout",
      children: undefined,
    };
    const outerGroupFiber = fiber(NavListComponent, outerProps);
    const outerListFiber = host(outerList, { "data-appearance": "plain" });
    children(navFiber, headerFiber, scrollFiber);
    children(scrollFiber, outerGroupFiber);
    children(outerGroupFiber, outerListFiber);

    const homeFiber = host(home);
    const exploreFiber = host(explore);
    const moreFiber = host(more);

    const sortables: Fiber[] = [];
    const pinLists: FixtureElement[] = [];
    const pinRows: ReturnType<typeof makeDestination>[] = [];
    const contextCount = options.sortableContexts ?? 1;
    const dndContexts: Fiber[] = [];
    for (let contextIndex = 0; contextIndex < contextCount; contextIndex += 1) {
      const ids = contextIndex === 0 ? pins : [`plugin:other-${contextIndex}`];
      const sortable = fiber(SortableContextComponent, { items: ids, strategy, children: ids.length > 0 ? null : false });
      const dnd = fiber(DndContextComponent, { children: sortable });
      children(dnd, sortable);
      if (ids.length > 0 && !omitted.has("group")) {
        const pinList = outerList.appendChild(new FixtureElement("div", { "data-appearance": "plain" }));
        const layout = contextIndex === 0 ? groupLayout : { itemSpacing: "rail", className: `other-pin-layout-${contextIndex}` };
        const groupProps = { ...layout, children: undefined };
        const group = fiber(NavListComponent, groupProps);
        const pinListFiber = host(pinList, { "data-appearance": "plain" });
        const groupRows = ids.map((id) => {
          const row = makeDestination(id, true, options, omitted);
          pinList.appendChild(row.button);
          pinRows.push(row);
          return row.fiber;
        });
        children(group, pinListFiber);
        children(pinListFiber, ...groupRows);
        sortable.memoizedProps.children = group;
        sortable.pendingProps.children = group;
        children(sortable, group);
        pinLists.push(pinList);
      }
      sortables.push(sortable);
      dndContexts.push(dnd);
    }

    outerList.appendChild(more);

    // Actual pin DOM follows the transparent DnD node and precedes More.
    // The React child/sibling chain, rather than these attributes, defines the empty-list boundary.
    const nextFiber = fiber(MoreComponent, {});
    children(outerListFiber, homeFiber, primaryDestination.fiber, exploreFiber, ...dndContexts, nextFiber);
    // The real More component is a host sibling with no useful label for discovery.
    children(nextFiber, moreFiber);

    if (options.unrelatedSortable) {
      const unrelated = body.appendChild(new FixtureElement("div", { "data-testid": "unrelated-sortable" }));
      const unrelatedFiber = fiber(SortableContextComponent, { items: ["unrelated"], strategy, children: null });
      const unrelatedHost = host(unrelated);
      children(unrelatedFiber, unrelatedHost);
      const unrelatedRoot = fiber(RailComponent, {});
      children(unrelatedRoot, unrelatedFiber);
    }

    body.appendChild(nav);
    if (railIndex === 0) {
      first = {
        body, nav, outerList, pinList: pinLists[0] ?? null, firstPin: pinRows[0]?.button ?? null, more,
        sortables, nativeButtonType: StyledButtonComponent, tooltipType: TooltipComponent,
        groupType: NavListComponent, buttonVisual, tooltipPosition, groupLayout,
      };
    }
  }

  if (!first) throw new Error("Fixture must contain at least one navigation rail");
  return { doc: new FixtureDocument(root), ...first };
}

function makeDestination(id: string, pinned: boolean, options: BuildOptions, omitted: Set<string>) {
  const button = new FixtureElement("button", {
    "data-sidebar-destination": id,
    "aria-current": id === "builtin:codex" ? "page" : "false",
  });
  const qxo = fiber(DestinationComponent, {
    item: { id, label: `old ${id}` },
    ...(pinned ? { pinContextMenuItem: { id: "unpin-from-sidebar" } } : {}),
  });
  let parent = qxo;
  const menu = fiber(ContextMenuComponent, { onClick: oldAction, children: null });
  children(parent, menu);
  parent = menu;
  if (!omitted.has("tooltip")) {
    const tooltipProps = {
      cloneCustomTrigger: true,
      closeOnTriggerClick: true,
      ...options.tooltipPosition ?? { side: "right", sideOffset: 4, align: "center" },
      tooltipContent: `old ${id}`,
      open: true,
      onClick: oldAction,
      onOpenChange: oldAction,
      children: null,
    };
    const tooltip = fiber(TooltipComponent, tooltipProps);
    children(parent, tooltip);
    parent = tooltip;
  }
  const wrapper = fiber(function MXoFixture() {}, { onClick: oldAction, ref: oldAction, children: null });
  children(parent, wrapper);
  parent = wrapper;
  const visual = options.buttonVisual ?? {
    color: "secondary", variant: "ghost", pill: false, size: "xl", iconSize: "lg", uniform: true,
  };
  const buttonProps = {
    ...(omitted.has("button") ? {} : visual),
    ...(pinned && options.pinButtonClass !== undefined ? { className: options.pinButtonClass } : {}),
    ...(!pinned && options.primaryButtonClass !== undefined ? { className: options.primaryButtonClass } : {}),
    selected: true,
    "aria-selected": "true",
    "aria-current": "page",
    "data-sidebar-destination": id,
    onClick: oldAction,
    ref: oldAction,
    children: { icon: "old icon" },
  };
  const buttonComponent = fiber(StyledButtonComponent, buttonProps);
  const buttonFiber = host(button, {
    "data-sidebar-destination": id,
    "aria-current": "page",
    "aria-selected": "true",
  });
  children(parent, buttonComponent);
  children(buttonComponent, buttonFiber);
  return { fiber: qxo, button, buttonFiber };
}

function requireCapabilities(doc: FixtureDocument) {
  const result = findOfficialSidebarCapabilities(doc as unknown as Document);
  if (!result) throw new Error("Expected a unique, complete Codex sidebar capability set");
  return result;
}

describe("official sidebar capabilities", () => {
  test("finds a host pin group and first item without relying on Code Review", () => {
    const f = buildFixture({ pins: ["plugin:incodex", "builtin:agents"] });
    const rails = f.doc.querySelectorAll('nav[data-app-navigation-rail="true"]');
    expect(rails).toHaveLength(1);
    const found = requireCapabilities(f.doc);

    expect(found.rail).toBe(f.nav as unknown as HTMLElement);
    expect(found.slot.parent).toBe(f.pinList as unknown as HTMLElement);
    expect(found.slot.before).toBe(f.firstPin as unknown as ChildNode);
    expect(found.button.type).toBe(f.nativeButtonType);
    expect(found.tooltip.type).toBe(f.tooltipType);
    expect(found.group.type).toBe(f.groupType);
  });

  test("uses the transparent SortableContext sibling boundary when there are no pins", () => {
    const f = buildFixture({ pins: [] });
    expect(f.pinList).toBeNull();
    expect(f.sortables[0]!.memoizedProps.items).toEqual([]);
    expect(f.sortables[0]!.memoizedProps.children).toBe(false);
    const found = requireCapabilities(f.doc);

    expect(found.slot.parent).toBe(f.outerList as unknown as HTMLElement);
    expect(found.slot.before).toBe(f.more as unknown as ChildNode);
    expect(found.button.type).toBe(f.nativeButtonType);
    expect(found.tooltip.type).toBe(f.tooltipType);
    expect(found.group.type).toBe(f.groupType);
    expect(found.group.props.itemSpacing).toBe("rail");
  });

  test("copies current host visual, tooltip, and group props while dropping row identity", () => {
    const buttonVisual = { color: "accent", variant: "outline", pill: true, size: "md", iconSize: "sm", uniform: false };
    const tooltipPosition = { side: "left", sideOffset: 9, align: "start" };
    const groupLayout = { itemSpacing: "rail", className: "host-pin-layout-v2" };
    const f = buildFixture({ buttonVisual, tooltipPosition, groupLayout, pins: ["plugin:incodex"] });
    const found = requireCapabilities(f.doc);

    expect(found.button.props).toEqual(buttonVisual);
    expect(found.tooltip.props).toEqual({
      cloneCustomTrigger: true, closeOnTriggerClick: true, ...tooltipPosition,
    });
    expect(found.group.props).toEqual(groupLayout);
    for (const props of [found.button.props, found.tooltip.props, found.group.props]) {
      expect(props).not.toHaveProperty("onClick");
      expect(props).not.toHaveProperty("ref");
      expect(props).not.toHaveProperty("children");
    }
    expect(found.button.props).not.toHaveProperty("selected");
    expect(found.button.props).not.toHaveProperty("aria-selected");
    expect(found.button.props).not.toHaveProperty("aria-current");
    expect(found.button.props).not.toHaveProperty("data-sidebar-destination");
    expect(found.tooltip.props).not.toHaveProperty("tooltipContent");
    expect(found.tooltip.props).not.toHaveProperty("open");
  });

  test("tracks host component changes instead of retaining hard-coded sidebar styling", () => {
    const first = requireCapabilities(buildFixture().doc);
    const changed = requireCapabilities(buildFixture({
      buttonVisual: { color: "warning", variant: "solid", pill: false, size: "lg", iconSize: "md", uniform: true },
      tooltipPosition: { side: "bottom", sideOffset: 6, align: "end" },
      groupLayout: { itemSpacing: "compact", className: "host-pin-layout-v3" },
    }).doc);

    expect(first.button.props.size).toBe("xl");
    expect(changed.button.props.size).toBe("lg");
    expect(changed.button.props.color).toBe("warning");
    expect(changed.tooltip.props.side).toBe("bottom");
    expect(changed.tooltip.props.sideOffset).toBe(6);
    expect(changed.group.props.itemSpacing).toBe("compact");
    expect(changed.group.props.className).toBe("host-pin-layout-v3");
  });

  test("ignores an unrelated sortable context outside the unique navigation rail", () => {
    const f = buildFixture({ unrelatedSortable: true });
    const found = requireCapabilities(f.doc);
    expect(found.rail).toBe(f.nav as unknown as HTMLElement);
    expect(found.slot.parent).toBe(f.pinList as unknown as HTMLElement);
  });

  test("fails closed when the rail selector finds multiple navigation rails", () => {
    const f = buildFixture({ rails: 2 });
    expect(findOfficialSidebarCapabilities(f.doc as unknown as Document)).toBeNull();
  });

  test("fails closed when multiple pin SortableContexts make the slot ambiguous", () => {
    const f = buildFixture({ sortableContexts: 2 });
    expect(findOfficialSidebarCapabilities(f.doc as unknown as Document)).toBeNull();
  });

  test("fails closed when a pinned destination return chain cycles before reaching the rail", () => {
    const f = buildFixture();
    const pinHostFiber = fiberOn(f.firstPin!);
    const destination = ancestorFiber(pinHostFiber, DestinationComponent);
    expect(destination).not.toBeNull();
    destination!.return = destination;

    expect(findOfficialSidebarCapabilities(f.doc as unknown as Document)).toBeNull();
  });

  test("fails closed when the mounted pin group DOM is outside the rail", () => {
    const f = buildFixture();
    f.body.appendChild(f.pinList!);
    expect(f.pinList!.parentElement).toBe(f.body);
    expect(f.pinList!.isConnected).toBe(true);

    expect(findOfficialSidebarCapabilities(f.doc as unknown as Document)).toBeNull();
  });

  test("fails closed when the native rail return ancestry cycles", () => {
    const f = buildFixture();
    const navFiber = fiberOn(f.nav);
    const nativeRailFiber = navFiber.return;
    expect(nativeRailFiber).not.toBeNull();
    nativeRailFiber!.return = navFiber;

    expect(findOfficialSidebarCapabilities(f.doc as unknown as Document)).toBeNull();
  });

  test("accepts a current nav alternate that reuses children returning to the mounted old nav", () => {
    const f = buildFixture();
    const oldNav = fiberOn(f.nav);
    const sharedChild = oldNav.child;
    installCurrentRootWithBailedOutNav(f.nav);

    expect(oldNav.alternate).not.toBeNull();
    expect(oldNav.alternate!.child).toBe(sharedChild);
    expect(sharedChild!.return).toBe(oldNav);
    expect(findOfficialSidebarCapabilities(f.doc as unknown as Document)).not.toBeNull();
  });

  test("accepts a current root that shares the mounted rail and nav fibers without a nav alternate", () => {
    const f = buildFixture();
    const mountedNav = fiberOn(f.nav);
    installCurrentRootSharingMountedNav(f.nav);

    expect(mountedNav.alternate).toBeUndefined();
    expect(findOfficialSidebarCapabilities(f.doc as unknown as Document)).not.toBeNull();
  });

  test("uses the non-sortable native button base class without pin drag interaction classes", () => {
    const primaryButtonClass = "host-button-base-token";
    const pinButtonClass = `${primaryButtonClass} pin-drag-gesture-token`;
    const f = buildFixture({ primaryButtonClass, pinButtonClass });
    const found = requireCapabilities(f.doc);

    expect(found.button.type).toBe(f.nativeButtonType);
    expect(found.button.props.className).toBe(primaryButtonClass);
    expect(found.button.props.className).not.toContain("pin-drag-gesture-token");
  });

  test("ignores unrelated native rail buttons when reading destination base classes", () => {
    const f = buildFixture({ primaryButtonClass: "host-base", pinButtonClass: "host-base pin-drag" });
    const header = f.nav.childNodes[0]!;
    const unrelated = header.appendChild(new FixtureElement("button"));
    const button = fiber(StyledButtonComponent, { ...f.buttonVisual, className: "unrelated-header-style" });
    children(fiberOn(header), button); children(button, host(unrelated));
    expect(requireCapabilities(f.doc).button.props.className).toBe("host-base");
  });

  test("fails closed when Button, Tooltip, or group host capability is missing", () => {
    for (const omit of ["button", "tooltip", "group"] as const) {
      const f = buildFixture({ omit: [omit] });
      expect(findOfficialSidebarCapabilities(f.doc as unknown as Document)).toBeNull();
    }
  });
});
