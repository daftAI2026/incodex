/**
 * [INPUT]: 依赖 Portal Root 与 Sidebar、Banner 的同内容调和入口。
 * [OUTPUT]: 验证 owner 丢失后两类 React island 均恢复挂载。
 * [POS]: Portal owner 恢复回归；不启动官方 App。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { createOfficialTooltipPortalRoot } from "./official-tooltip-portal.ts";
import { createOfficialSidebarEntry } from "./official-sidebar-entry.ts";
import { createOfficialNotifications } from "./official-notifications.ts";

function portalHarness() {
  const context: { Provider?: unknown } = {};
  context.Provider = context;
  const container = { nodeType: 1, isConnected: true };
  const provider: { type: object; memoizedProps: { value: Record<string, unknown> }; return: unknown } = {
    type: context, memoizedProps: { value: { radixPortalContainer: container } }, return: null,
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
    ownerAvailable(available: boolean) {
      provider.memoizedProps = { value: available ? { radixPortalContainer: container } : {} };
    },
  };
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
