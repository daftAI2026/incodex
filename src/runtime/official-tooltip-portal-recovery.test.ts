/**
 * [INPUT]: 依赖 Portal Root、Toolbar Renderer、Sidebar 与 Banner 调和入口。
 * [OUTPUT]: 验证 owner 恢复、Tooltip 重放及取消后的不复活。
 * [POS]: Portal owner 恢复回归；不启动官方 App。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { createOfficialTooltipPortalRoot } from "./official-tooltip-portal.ts";
import { createOfficialTooltipRenderer } from "./official-tooltip-renderer.ts";
import { createOfficialSidebarEntry } from "./official-sidebar-entry.ts";
import { createOfficialNotifications } from "./official-notifications.ts";
import { createTooltipLifecycle } from "./tooltip-lifecycle.ts";

function portalHarness() {
  const context: { Provider?: unknown } = {};
  context.Provider = context;
  const container = { nodeType: 1, isConnected: true };
  let value: Record<string, unknown> = { radixPortalContainer: container, route: "initial" };
  const provider: { type: object; memoizedProps: { value: Record<string, unknown> }; return: unknown } = {
    type: context, memoizedProps: { value }, return: null,
  };
  const search = { "__reactFiber$portal-test": { return: provider } } as unknown as HTMLElement;
  const rawRenders: unknown[] = [];
  const root = createOfficialTooltipPortalRoot(
    { render(value: unknown) { rawRenders.push(value); }, unmount() {} },
    (type, props) => ({ type, props }),
    () => search,
    context,
  );
  return {
    root,
    rawRenders,
    context,
    container,
    search,
    get value() { return value; },
    replaceValue(route: string) {
      value = { radixPortalContainer: container, route };
      provider.memoizedProps = { value };
    },
    ownerAvailable(available: boolean) {
      value = available
        ? { radixPortalContainer: container, route: "returned" }
        : { radixPortalContainer: container, route: "missing" };
      provider.memoizedProps = { value: available ? value : {} };
    },
  };
}

function toolbarRenderer(owner: ReturnType<typeof portalHarness>) {
  const host = { isConnected: true, setAttribute() {}, remove() { this.isConnected = false; } };
  const doc = {
    documentElement: {},
    createElement: () => host,
    body: { append(node: typeof host) { node.isConnected = true; } },
    defaultView: { getComputedStyle: () => ({ getPropertyValue: () => "" }) },
  } as unknown as Document;
  function Tooltip() {}
  const renderer = createOfficialTooltipRenderer(doc, async () => ({
    Tooltip,
    createElement: (type: unknown, props: Record<string, unknown>) => ({ type, props }),
    createRoot: () => owner.root,
    tooltipContract: { contentProp: "content", openProp: "forceOpen", anchorMode: "slot-child" },
  }));
  const button = {
    isConnected: true, getAttribute: () => null, setAttribute() {}, removeAttribute() {},
  } as unknown as HTMLElement;
  return { renderer, button, Tooltip };
}

function element() {
  const node: any = {
    isConnected: true, parentElement: null, parentNode: null, children: [], attrs: {},
    setAttribute(key: string, value: string) { this.attrs[key] = value; },
    insertBefore(child: any, before: any) {
      child.remove();
      const index = before ? this.children.indexOf(before) : this.children.length;
      this.children.splice(index, 0, child);
      child.parentElement = this; child.parentNode = this; child.isConnected = true;
    },
    remove() {
      if (this.parentElement) this.parentElement.children.splice(this.parentElement.children.indexOf(this), 1);
      this.parentElement = null; this.parentNode = null; this.isConnected = false;
    },
  };
  Object.defineProperty(node, "firstChild", { get: () => node.children[0] ?? null });
  Object.defineProperty(node, "nextSibling", { get: () => node.parentElement?.children[node.parentElement.children.indexOf(node) + 1] ?? null });
  return node;
}

test("Sidebar rerenders identical capabilities after the Portal owner returns", async () => {
  const portal = portalHarness();
  const rail = element(), parent = element(), official = element();
  rail.insertBefore(parent, null); parent.insertBefore(official, null);
  const doc = { createElement: element } as unknown as Document;
  const modules = {
    createElement: (type: unknown, props: Record<string, unknown>) => ({ type, props }),
    createRoot: () => portal.root,
  };
  const Button = () => {}, Tooltip = () => {}, Group = () => {};
  const caps: any = {
    rail, slot: { parent, before: official },
    button: { type: Button, props: { size: "current" } },
    tooltip: { type: Tooltip, props: { side: "right" }, contentProp: "content" },
    group: { type: Group, props: { itemSpacing: "rail" } },
  };
  const copy = { label: "Private", icon: "<svg/>", pressed: false, onActivate() {} };
  const manager = createOfficialSidebarEntry(doc, async () => modules);

  portal.ownerAvailable(false);
  await manager.ensure(caps, copy);
  expect(portal.rawRenders.at(-1)).toBeNull();
  portal.ownerAvailable(true);
  await manager.ensure(caps, copy);
  expect(portal.rawRenders.at(-1)).not.toBeNull();
  const rendersAfterRecovery = portal.rawRenders.length;
  await manager.ensure(caps, copy);
  expect(portal.rawRenders).toHaveLength(rendersAfterRecovery);
});

test("Banner reconcile notices and repairs an identical mounted Portal root", async () => {
  const portal = portalHarness();
  const slot: any = {
    isConnected: true, firstChild: null,
    insertBefore(host: any) { host.parentElement = this; host.isConnected = true; },
  };
  const doc = {
    querySelectorAll: () => [],
    createElement: () => ({ isConnected: false, setAttribute() {}, remove() { this.isConnected = false; } }),
  } as unknown as Document;
  function Banner() {}
  const modules = {
    Banner, CloseIcon: () => {},
    createElement: (type: unknown, props: Record<string, unknown>) => ({ type, props }),
    createRoot: () => portal.root,
  };
  const manager = createOfficialNotifications(doc, async () => modules);
  const copy = { title: "Private", body: "Body", closeLabel: "Dismiss", icon: "<svg/>", onClose() {} };

  portal.ownerAvailable(false);
  await manager.ensure(slot, copy);
  expect(portal.rawRenders.at(-1)).toBeNull();
  portal.ownerAvailable(true);
  expect(manager.bannerNeedsReconcile(slot)).toBe(true);
  await manager.ensure(slot, copy);
  expect(manager.bannerNeedsReconcile(slot)).toBe(false);
  expect(portal.rawRenders.at(-1)).not.toBeNull();
  const rendersAfterRecovery = portal.rawRenders.length;
  await manager.ensure(slot, copy);
  expect(portal.rawRenders).toHaveLength(rendersAfterRecovery);
});

test("Portal rerender intent ends on null render and unmount", () => {
  const portal = portalHarness();
  const root = portal.root as typeof portal.root & { needsRender(): boolean };
  const child = {};
  root.render(child);
  expect(root.needsRender()).toBe(false);
  portal.ownerAvailable(false);
  expect(root.needsRender()).toBe(true);
  root.render(child);
  expect(root.needsRender()).toBe(false);
  portal.ownerAvailable(true);
  expect(root.needsRender()).toBe(true);
  root.render(child);
  expect(root.needsRender()).toBe(false);
  root.render(null);
  portal.ownerAvailable(false);
  expect(root.needsRender()).toBe(false);
  root.render(child);
  root.unmount();
  portal.ownerAvailable(true);
  expect(root.needsRender()).toBe(false);
});

test("Toolbar Renderer replays a shown Tooltip when the official Portal value changes", async () => {
  const owner = portalHarness();
  const { renderer, button } = toolbarRenderer(owner);
  await renderer.prepare();
  renderer.show(button, "Open private", "Shift+Command+N", owner.search);
  const initialRenderCount = owner.rawRenders.length;
  owner.replaceValue("updated");
  expect(owner.root.needsRender()).toBe(true);
  expect(renderer.ready()).toBe(true);
  expect(renderer.needsRemount()).toBe(false);

  const reconcile = (renderer as unknown as { reconcile?: () => boolean }).reconcile;
  expect(reconcile?.call(renderer)).toBe(true);
  expect(owner.rawRenders).toHaveLength(initialRenderCount + 1);
  expect((owner.rawRenders.at(-1) as { props: { value: unknown } }).props.value).toBe(owner.value);
  expect(reconcile?.call(renderer)).toBe(false);
  renderer.dispose();
});

test("Toolbar keeps Tooltip intent through an owner gap without remount fallback", async () => {
  const owner = portalHarness();
  const { renderer, button } = toolbarRenderer(owner);
  await renderer.prepare();
  renderer.show(button, "Open private", "Shift+Command+N", owner.search);
  owner.ownerAvailable(false);
  expect(renderer.ready()).toBe(true);
  expect(renderer.needsRemount()).toBe(false);

  const reconcile = (renderer as unknown as { reconcile?: () => boolean }).reconcile;
  expect(reconcile?.call(renderer)).toBe(true);
  expect(owner.rawRenders.at(-1)).toBeNull();
  expect(reconcile?.call(renderer)).toBe(false);

  owner.ownerAvailable(true);
  expect(reconcile?.call(renderer)).toBe(true);
  expect((owner.rawRenders.at(-1) as { props: { value: unknown } }).props.value).toBe(owner.value);
  renderer.hide();
  const rendersAfterHide = owner.rawRenders.length;
  owner.ownerAvailable(false);
  expect(reconcile?.call(renderer)).toBe(false);
  owner.ownerAvailable(true);
  expect(reconcile?.call(renderer)).toBe(false);
  expect(owner.rawRenders).toHaveLength(rendersAfterHide);
  renderer.dispose();
});

test("Toolbar leave, blur and dismiss revoke Tooltip intent before owner recovery", async () => {
  for (const cancel of ["pointerLeave", "blur", "dismiss"] as const) {
    const owner = portalHarness();
    const { renderer, button } = toolbarRenderer(owner);
    await renderer.prepare();
    const scheduled: Array<() => void> = [];
    const lifecycle = createTooltipLifecycle({
      delayMs: 500,
      schedule(callback) { scheduled.push(callback); return scheduled.length; },
      cancel() {},
      canShow: () => renderer.ready(),
      show: () => renderer.show(button, "Open private", "Shift+Command+N", owner.search),
      hide: () => renderer.hide(),
    });
    lifecycle.pointerEnter();
    scheduled.shift()!();
    expect(owner.rawRenders.at(-1)).not.toBeNull();
    owner.ownerAvailable(false);
    const reconcile = (renderer as unknown as { reconcile?: () => boolean }).reconcile;
    expect(reconcile?.call(renderer)).toBe(true);
    expect(owner.rawRenders.at(-1)).toBeNull();

    lifecycle[cancel]();
    const rendersAfterCancel = owner.rawRenders.length;
    owner.ownerAvailable(true);
    expect(reconcile?.call(renderer)).toBe(false);
    expect(owner.rawRenders).toHaveLength(rendersAfterCancel);
    lifecycle.dispose();
    renderer.dispose();
  }
});

test("Toolbar ensure refresh reconciles its existing Portal Root", () => {
  const source = readFileSync(join(import.meta.dir, "inject.ts"), "utf8");
  const start = source.indexOf("function ensureButton(): void {");
  const end = source.indexOf("\nfunction observeSearchAppearance", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  expect(source.slice(start, end)).toContain("renderer.reconcile()");
});
