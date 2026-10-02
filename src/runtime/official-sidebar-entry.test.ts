import { expect, test } from "bun:test";
import { createOfficialSidebarEntry } from "./official-sidebar-entry.ts";

function fixture() {
  function element() {
    const el: any = {
      isConnected: true, parentElement: null, children: [], attrs: {},
      setAttribute(k: string, v: string) { this.attrs[k] = v; },
      insertBefore(child: any, before: any) {
        child.remove();
        const i = before ? this.children.indexOf(before) : this.children.length;
        this.children.splice(i, 0, child); child.parentElement = this; child.isConnected = true;
      },
      remove() {
        if (this.parentElement) this.parentElement.children.splice(this.parentElement.children.indexOf(this), 1);
        this.parentElement = null; this.isConnected = false;
      },
    };
    Object.defineProperty(el, "firstChild", { get: () => el.children[0] ?? null });
    Object.defineProperty(el, "nextSibling", { get: () => el.parentElement?.children[el.parentElement.children.indexOf(el) + 1] ?? null });
    return el;
  }
  const rail = element(), parent = element(), official = element();
  rail.insertBefore(parent, null); parent.insertBefore(official, null);
  const renders: any[] = []; let unmounted = 0, roots = 0;
  const modules = {
    createElement: (type: unknown, props: Record<string, unknown>) => ({ type, props }),
    createRoot: () => { roots++; return { render(value: unknown) { renders.push(value); }, unmount() { unmounted++; } }; },
  };
  const doc = { createElement: element } as unknown as Document;
  const caps: any = {
    rail, slot: { parent, before: official },
    button: { type: function NativeButton() {}, props: { size: "host-size", className: "current-host" } },
    tooltip: { type: function NativeTooltip() {}, props: { side: "right" } },
    group: { type: function NativeGroup() {}, props: { itemSpacing: "rail" } },
  };
  const copy = { label: "私密窗口", icon: "<svg/>", hoverIcon: "<svg><path/></svg>", pressed: false, onActivate() {} };
  return { doc, modules, caps, copy, parent, official, renders, element, roots: () => roots, unmounted: () => unmounted };
}

test("uses current native Button/Tooltip props, shared content and the supplied action", async () => {
  const f = fixture(); const manager = createOfficialSidebarEntry(f.doc, async () => f.modules);
  await manager.ensure(f.caps, f.copy);
  const tip = f.renders.at(-1), button = tip.props.children;
  expect(tip.type).toBe(f.caps.tooltip.type);
  expect(tip.props).toMatchObject({ side: "right", tooltipContent: f.copy.label });
  expect(button.type).toBe(f.caps.button.type);
  expect(button.props).toMatchObject({ size: "host-size", className: "current-host", "aria-label": f.copy.label });
  expect(button.props.onClick).toBe(f.copy.onActivate);
  expect(f.parent.firstChild.attrs["data-incodex-sidebar-host"]).toBe("true");
});

test("reconcile is idempotent, repairs first position and adopts changed native props", async () => {
  const f = fixture(); const manager = createOfficialSidebarEntry(f.doc, async () => f.modules);
  await manager.ensure(f.caps, f.copy); const host = f.parent.firstChild;
  await manager.ensure({ ...f.caps }, { ...f.copy });
  expect(f.roots()).toBe(1); expect(f.renders).toHaveLength(1);
  f.parent.insertBefore(f.official, host);
  await manager.ensure(f.caps, f.copy); expect(f.parent.firstChild).toBe(host);
  await manager.ensure({ ...f.caps, button: { ...f.caps.button, props: { size: "new-host-size" } } }, f.copy);
  expect(f.renders.at(-1).props.children.props.size).toBe("new-host-size");
  expect(f.roots()).toBe(1);
});

test("renders a native group for an initially empty transparent Pin slot", async () => {
  const f = fixture(); const manager = createOfficialSidebarEntry(f.doc, async () => f.modules);
  await manager.ensure({ ...f.caps, slot: { ...f.caps.slot, empty: true } }, f.copy);
  expect(f.renders.at(-1).type).toBe(f.caps.group.type);
  expect(f.renders.at(-1).props.itemSpacing).toBe("rail");
  expect(f.renders.at(-1).props.children.type).toBe(f.caps.tooltip.type);
});

test("unmounts removed slots and fails closed when discovery disappears", async () => {
  const f = fixture(); const manager = createOfficialSidebarEntry(f.doc, async () => f.modules);
  await manager.ensure(f.caps, f.copy); const host = f.parent.firstChild;
  host.remove(); await manager.ensure(f.caps, f.copy);
  expect(f.roots()).toBe(2); expect(f.unmounted()).toBe(1);
  await manager.ensure(null, f.copy);
  expect(f.unmounted()).toBe(2); expect(f.parent.children).toEqual([f.official]);
});

test("does not mount a stale slot after React preparation completes", async () => {
  const f = fixture(); let ready!: (m: typeof f.modules) => void;
  const manager = createOfficialSidebarEntry(f.doc, () => new Promise(resolve => { ready = resolve; }));
  const pending = manager.ensure(f.caps, f.copy);
  await manager.ensure(null, f.copy); ready(f.modules); await pending;
  expect(f.roots()).toBe(0);
});

test("retries failed preparation and dispose blocks pending mounting", async () => {
  const f = fixture(); let attempts = 0;
  const manager = createOfficialSidebarEntry(f.doc, async () => { if (!attempts++) throw Error("not ready"); return f.modules; });
  await expect(manager.ensure(f.caps, f.copy)).rejects.toThrow("not ready");
  await manager.ensure(f.caps, f.copy); expect(f.roots()).toBe(1);
  manager.dispose(); expect(f.unmounted()).toBe(1);
});

test("does not mount a still-connected slot moved outside its rail during preparation", async () => {
  const f = fixture(); let ready!: (m: typeof f.modules) => void;
  const manager = createOfficialSidebarEntry(f.doc, () => new Promise(resolve => { ready = resolve; }));
  const pending = manager.ensure(f.caps, f.copy);
  const outside = f.element(); outside.insertBefore(f.parent, null);
  ready(f.modules); await pending;
  expect(f.roots()).toBe(0);
});
