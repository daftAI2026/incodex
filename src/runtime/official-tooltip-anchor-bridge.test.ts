/**
 * [INPUT]: 依赖官方 Slot 注入的 ref、事件与 ARIA 属性。
 * [OUTPUT]: 验证现代 Tooltip 到外置按钮的提交和销毁契约。
 * [POS]: src/runtime 的锚点桥接测试；不启动 App。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";

type BridgeModule = {
  createOfficialTooltipAnchorBridge: (
    createElement: (type: unknown, props: Record<string, unknown>) => unknown,
    target: HTMLElement,
  ) => { type: unknown; props: Record<string, unknown> };
};

class FakeTarget {
  attributes = new Map<string, string>([
    ["aria-describedby", "existing-description other-description"],
    ["aria-label", "Open private window"],
    ["aria-pressed", "false"],
    ["data-state", "original-state"],
  ]);
  listeners = new Map<string, Set<(event: unknown) => void>>();

  getAttribute(name: string): string | null { return this.attributes.get(name) ?? null; }
  setAttribute(name: string, value: string): void { this.attributes.set(name, value); }
  removeAttribute(name: string): void { this.attributes.delete(name); }
  addEventListener(type: string, listener: EventListenerOrEventListenerObject, options?: boolean | AddEventListenerOptions): void {
    const key = `${type}:${typeof options === "boolean" ? options : options?.capture === true}`;
    const listeners = this.listeners.get(key) ?? new Set();
    listeners.add(listener as (event: unknown) => void);
    this.listeners.set(key, listeners);
  }
  removeEventListener(type: string, listener: EventListenerOrEventListenerObject, options?: boolean | EventListenerOptions): void {
    const key = `${type}:${typeof options === "boolean" ? options : options?.capture === true}`;
    this.listeners.get(key)?.delete(listener as (event: unknown) => void);
  }
  dispatch(type: string, event: unknown, capture = false): void {
    for (const listener of this.listeners.get(`${type}:${capture}`) ?? []) listener(event);
  }
}

describe("official tooltip anchor bridge", () => {
  test("installs the Slot ref and generated ARIA token at commit, then runs ref cleanup", async () => {
    const loaded = await import("./official-tooltip-anchor-bridge.ts").catch(() => null);
    expect(loaded).not.toBeNull();
    if (!loaded) return;
    const { createOfficialTooltipAnchorBridge } = loaded as BridgeModule;
    const target = new FakeTarget();
    const targetElement = target as unknown as HTMLElement;
    const refEvents: Array<HTMLElement | null> = [];
    let officialRefCleanup = 0;
    let pointerDowns = 0;
    const bridge = createOfficialTooltipAnchorBridge((type, props) => ({ type, props }), targetElement);
    const component = bridge.type as (props: Record<string, unknown>) => { type: unknown; props: Record<string, unknown> };
    const slotRef = (node: HTMLElement | null) => {
      refEvents.push(node);
      if (node) return () => { officialRefCleanup++; };
    };
    const sentinel = component({
      ...bridge.props,
      ref: slotRef,
      onPointerDownCapture: () => { pointerDowns++; },
      "aria-describedby": "radix-generated-tooltip",
      "data-state": "open",
    });

    expect(sentinel.type).toBe("span");
    expect(sentinel.props.hidden).toBe(true);
    expect(sentinel.props["aria-hidden"]).toBe(true);
    const commitRef = sentinel.props.ref as (node: HTMLElement | null) => (() => void) | undefined;
    const cleanup = commitRef({} as HTMLElement);
    expect(refEvents).toEqual([targetElement]);
    expect(target.getAttribute("aria-describedby")).toBe(
      "existing-description other-description radix-generated-tooltip",
    );
    expect(target.getAttribute("aria-label")).toBe("Open private window");
    expect(target.getAttribute("aria-pressed")).toBe("false");
    expect(target.getAttribute("data-state")).toBe("open");

    target.dispatch("pointerdown", { currentTarget: target }, true);
    expect(pointerDowns).toBe(1);

    // A third party can add a token while the bridge is mounted. Cleanup owns
    // only the Tooltip token and must preserve both prior and live tokens.
    target.setAttribute("aria-describedby", `${target.getAttribute("aria-describedby")} live-description`);
    (cleanup as () => void)();
    expect(officialRefCleanup).toBe(1);
    expect(refEvents).toEqual([targetElement]);
    expect(target.getAttribute("aria-describedby")).toBe("existing-description other-description live-description");
    expect(target.getAttribute("data-state")).toBe("original-state");
    target.dispatch("pointerdown", { currentTarget: target }, true);
    expect(pointerDowns).toBe(1);
    commitRef(null);
    expect(officialRefCleanup).toBe(1);
  });

  test("calls ref(null) when the official callback ref has no cleanup", async () => {
    const loaded = await import("./official-tooltip-anchor-bridge.ts").catch(() => null);
    expect(loaded).not.toBeNull();
    if (!loaded) return;
    const { createOfficialTooltipAnchorBridge } = loaded as BridgeModule;
    const target = new FakeTarget();
    const targetElement = target as unknown as HTMLElement;
    const refEvents: Array<HTMLElement | null> = [];
    const bridge = createOfficialTooltipAnchorBridge((type, props) => ({ type, props }), targetElement);
    const component = bridge.type as (props: Record<string, unknown>) => { type: unknown; props: Record<string, unknown> };
    const sentinel = component({ ...bridge.props, ref: (node: HTMLElement | null) => { refEvents.push(node); } });
    const commitRef = sentinel.props.ref as (node: HTMLElement | null) => (() => void) | undefined;
    const cleanup = commitRef({} as HTMLElement);

    (cleanup as () => void)();
    expect(refEvents).toEqual([targetElement, null]);
  });
});
