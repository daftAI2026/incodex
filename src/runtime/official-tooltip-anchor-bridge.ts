/**
 * [INPUT]: 依赖官方 Tooltip Fiber、JSX 工厂及外置按钮。
 * [OUTPUT]: 导出官方定位能力、窗口缩放与 Slot 锚点桥。
 * [POS]: Tooltip 锚点适配器；sentinel 不参与定位和布局。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { parseOfficialWindowZoom } from "./tooltip-presentation.ts";

type ReactFiber = {
  return?: ReactFiber | null;
  type?: unknown;
  elementType?: unknown;
  memoizedProps?: unknown;
  pendingProps?: unknown;
};
type Ref = ((value: HTMLElement | null) => unknown) | { current?: unknown } | null | undefined;
type ElementFactory = (type: unknown, props: Record<string, unknown>) => unknown;

const BRIDGED_ATTRIBUTES = new Set(["aria-describedby", "data-state", "data-disabled"]);

export function readOfficialReactFiber(element: HTMLElement): ReactFiber | null {
  const key = Object.keys(element).find((name) => name.startsWith("__reactFiber$"));
  return key ? (element as unknown as Record<string, ReactFiber | undefined>)[key] ?? null : null;
}

export function officialSearchTooltipPositioning(
  search: HTMLElement | null,
  tooltipType: unknown,
  doc: Document,
): Record<string, unknown> {
  if (!search) return {};
  let props: Record<string, unknown> | null = null;
  for (let fiber = readOfficialReactFiber(search), depth = 0; fiber && depth < 64; fiber = fiber.return ?? null, depth += 1) {
    if (fiber.type !== tooltipType && fiber.elementType !== tooltipType) continue;
    const candidate = fiber.memoizedProps ?? fiber.pendingProps;
    if (typeof candidate !== "object" || candidate === null ||
        (!Object.hasOwn(candidate, "tooltipContent") && !Object.hasOwn(candidate, "content"))) continue;
    if (props) return {};
    props = candidate as Record<string, unknown>;
  }
  if (!props) return {};

  const positioning: Record<string, unknown> = {};
  const side = props.side ?? doc.defaultView?.getComputedStyle(search).getPropertyValue("--side-tooltip").trim();
  if (["top", "bottom", "left", "right"].includes(String(side))) positioning.side = side;
  const sideOffset = props.sideOffset;
  if (typeof sideOffset === "number" && Number.isFinite(sideOffset)) positioning.sideOffset = sideOffset;
  const align = props.align;
  if (["start", "center", "end"].includes(String(align))) positioning.align = align;
  const alignOffset = props.alignOffset;
  if (typeof alignOffset === "number" && Number.isFinite(alignOffset)) positioning.alignOffset = alignOffset;
  return positioning;
}

export function officialSearchZoomProvider(
  search: HTMLElement | null,
  doc: Document,
): { type: unknown; value: number } | null {
  if (!search) return null;
  const zoom = parseOfficialWindowZoom(
    doc.defaultView?.getComputedStyle(search).getPropertyValue("--codex-window-zoom") ?? "",
  );
  if (zoom === 1) return null;

  const candidates: Array<{ type: unknown; value: number }> = [];
  const visited = new Set<ReactFiber>();
  for (let fiber = readOfficialReactFiber(search); fiber && !visited.has(fiber); fiber = fiber.return ?? null) {
    visited.add(fiber);
    const value = (fiber.memoizedProps as { value?: unknown } | undefined)?.value;
    const context = fiber.type as { Provider?: unknown; _currentValue?: unknown } | null;
    if (value !== zoom || !context || context._currentValue !== 1 || !context.Provider) continue;
    candidates.push({ type: context.Provider, value: zoom });
  }
  return candidates.length === 1 ? candidates[0]! : null;
}

function reactEventName(name: string): { type: string; capture: boolean } | null {
  const match = /^on([A-Z][A-Za-z0-9]*?)(Capture)?$/u.exec(name);
  if (!match) return null;
  const reactName = match[1]!.toLowerCase();
  return { type: reactName === "doubleclick" ? "dblclick" : reactName, capture: match[2] === "Capture" };
}

function attributeValue(name: string, value: unknown): string | null {
  if (value === null || value === undefined) return null;
  if (name.startsWith("aria-") && typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "boolean") return value ? "" : null;
  return String(value);
}

function tokens(value: string | null): string[] {
  return value?.trim().split(/\s+/u).filter(Boolean) ?? [];
}

function bridgeAttributes(target: HTMLElement, props: Record<string, unknown>): Array<() => void> {
  const cleanup: Array<() => void> = [];
  for (const name of BRIDGED_ATTRIBUTES) {
    if (!Object.hasOwn(props, name)) continue;
    const value = attributeValue(name, props[name]);
    if (name === "aria-describedby") {
      const before = tokens(target.getAttribute(name));
      const added = [...new Set(tokens(value).filter((token) => !before.includes(token)))];
      if (added.length === 0) continue;
      target.setAttribute(name, [...before, ...added].join(" "));
      const owned = new Set(added);
      cleanup.push(() => {
        const remaining = tokens(target.getAttribute(name)).filter((token) => !owned.has(token));
        if (remaining.length) target.setAttribute(name, remaining.join(" "));
        else target.removeAttribute(name);
      });
      continue;
    }

    const before = target.getAttribute(name);
    if (value === null) target.removeAttribute(name);
    else target.setAttribute(name, value);
    cleanup.push(() => {
      if (target.getAttribute(name) !== value) return;
      if (before === null) target.removeAttribute(name);
      else target.setAttribute(name, before);
    });
  }
  return cleanup;
}

function bridgeEvents(target: HTMLElement, props: Record<string, unknown>): Array<() => void> {
  const cleanup: Array<() => void> = [];
  for (const [name, value] of Object.entries(props)) {
    const event = reactEventName(name);
    if (!event || typeof value !== "function") continue;
    const handler = value as (event: Event) => unknown;
    const listener: EventListener = (nativeEvent) => { Reflect.apply(handler, undefined, [nativeEvent]); };
    target.addEventListener(event.type, listener, event.capture);
    cleanup.push(() => target.removeEventListener(event.type, listener, event.capture));
  }
  return cleanup;
}

function bridgeRef(ref: Ref, target: HTMLElement): () => void {
  if (typeof ref === "function") {
    const returned = ref(target);
    let active = true;
    return () => {
      if (!active) return;
      active = false;
      if (typeof returned === "function") Reflect.apply(returned, undefined, []);
      else Reflect.apply(ref, undefined, [null]);
    };
  }
  if (ref && typeof ref === "object") {
    const previous = ref.current;
    ref.current = target;
    return () => { if (ref.current === target) ref.current = previous; };
  }
  return () => {};
}

function attachOfficialTrigger(target: HTMLElement, props: Record<string, unknown>, ref: Ref): () => void {
  const cleanups = [...bridgeAttributes(target, props), ...bridgeEvents(target, props)];
  const cleanRef = bridgeRef(ref, target);
  let active = true;
  return () => {
    if (!active) return;
    active = false;
    cleanRef();
    for (const cleanup of cleanups.reverse()) cleanup();
  };
}

export function createOfficialTooltipAnchorBridge(createElement: ElementFactory, target: HTMLElement): unknown {
  function ExternalAnchorBridge(props: Record<string, unknown>): unknown {
    // React 19 passes ref as a function-component prop. The Slot injects this
    // ref and its trigger props; only the committed sentinel callback touches DOM.
    const slotRef = props.ref as Ref;
    const commitRef = (() => {
      let cleanup: (() => void) | null = null;
      const detach = () => {
        cleanup?.();
        cleanup = null;
      };
      return (sentinel: HTMLElement | null) => {
        if (!sentinel) { detach(); return; }
        detach();
        cleanup = attachOfficialTrigger(target, props, slotRef);
        return detach;
      };
    })();
    return createElement("span", { hidden: true, "aria-hidden": true, ref: commitRef });
  }
  return createElement(ExternalAnchorBridge, { target });
}
