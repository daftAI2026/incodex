import { withinOfficialRail, type OfficialSidebarCapabilities } from "./official-sidebar.ts";

type Root = { render(element: unknown): void; unmount(): void };
type ReactModules = {
  createElement(type: unknown, props: Record<string, unknown>): unknown;
  createRoot(host: HTMLElement): Root;
};
export type SidebarEntryCopy = {
  label: string;
  icon: string;
  hoverIcon?: string;
  pressed: boolean;
  onActivate(): void;
};

function equalProps(a: Record<string, unknown>, b: Record<string, unknown>): boolean {
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every(key =>
    key === "style" && a[key] && b[key]
      ? equalProps(a[key] as Record<string, unknown>, b[key] as Record<string, unknown>)
      : a[key] === b[key],
  );
}

export function createOfficialSidebarEntry(doc: Document, loadReact: () => Promise<ReactModules>) {
  let desired: { caps: OfficialSidebarCapabilities; copy: SidebarEntryCopy } | null = null;
  let mounted: {
    host: HTMLElement; root: Root; caps: OfficialSidebarCapabilities; copy: SidebarEntryCopy;
  } | null = null;
  let modules: ReactModules | null = null;
  let pending: Promise<ReactModules> | null = null;
  let hovered = false;

  function remove() {
    const old = mounted; mounted = null; hovered = false;
    old?.root.unmount(); old?.host.remove();
  }

  function render() {
    if (!mounted || !modules) return;
    const { caps, copy, root } = mounted, { createElement } = modules;
    // The same whole SVG used by the toolbar/banner. The native Button supplies
    // its icon layout props; no sidebar dimensions, colors or CSS are frozen.
    const Icon = (props: Record<string, unknown>) => {
      const wrap = doc.createElement("span");
      wrap.innerHTML = hovered && copy.hoverIcon ? copy.hoverIcon : copy.icon;
      const svg = wrap.firstElementChild;
      const node = (el: Element, top = false): unknown => createElement(el.tagName.toLowerCase(), {
        ...Object.fromEntries([...el.attributes].map(({ name, value }) => [
          name === "class" ? "className" : /^(aria|data)-/u.test(name) ? name : name.replace(/-([a-z])/gu, (_, c: string) => c.toUpperCase()), value,
        ])),
        ...(top ? props : {}), "aria-hidden": true,
        ...(top ? { "data-incodex-icon": hovered && copy.hoverIcon ? "circle-x" : "hat-glasses" } : {}),
        children: [...el.children].map(child => node(child)),
      });
      return svg ? node(svg, true) : null;
    };
    const button = createElement(caps.button.type, {
      ...caps.button.props, type: "button", "data-incodex-sidebar-entry": "true",
      "aria-label": copy.label, "aria-pressed": copy.pressed, onClick: copy.onActivate,
      onPointerEnter: () => { if (!hovered && copy.hoverIcon) { hovered = true; render(); } },
      onPointerLeave: () => { if (hovered) { hovered = false; render(); } },
      children: createElement(Icon, {}),
    });
    const tooltip = createElement(caps.tooltip.type, {
      ...caps.tooltip.props, tooltipContent: copy.label, children: button,
    });
    root.render(caps.slot.empty
      ? createElement(caps.group.type, { ...caps.group.props, children: tooltip })
      : tooltip);
  }

  async function ensure(caps: OfficialSidebarCapabilities | null, copy: SidebarEntryCopy): Promise<void> {
    desired = caps ? { caps, copy } : null;
    if (!desired) { remove(); return; }
    if (!modules) {
      pending ??= loadReact().catch((error: unknown) => { pending = null; throw error; });
      modules = await pending;
    }
    const current = desired;
    if (!current?.caps.rail.isConnected || !current.caps.slot.parent.isConnected ||
      !withinOfficialRail(current.caps.slot.parent, current.caps.rail)) { remove(); return; }
    const { caps: next, copy: content } = current;
    const before = next.slot.before;
    if (before && before.parentNode !== undefined && before.parentNode !== next.slot.parent) return;
    if (!mounted || !mounted.host.isConnected || mounted.host.parentElement !== next.slot.parent) {
      remove();
      const host = doc.createElement("div"); host.setAttribute("data-incodex-sidebar-host", "true");
      next.slot.parent.insertBefore(host, before);
      mounted = { host, root: modules.createRoot(host), caps: next, copy: content };
      render();
      return;
    }
    const old = mounted.caps, prev = mounted.copy;
    if (mounted.host.nextSibling !== before && before !== mounted.host) next.slot.parent.insertBefore(mounted.host, before);
    const changed = old.slot.empty !== next.slot.empty ||
      ["button", "tooltip", "group"].some(key => {
        const a = old[key as "button"], b = next[key as "button"];
        return a.type !== b.type || !equalProps(a.props, b.props);
      }) || prev.label !== content.label || prev.icon !== content.icon || prev.hoverIcon !== content.hoverIcon ||
      prev.pressed !== content.pressed || prev.onActivate !== content.onActivate;
    mounted.caps = next; mounted.copy = content;
    if (changed) render();
  }

  return { ensure, dispose() { desired = null; remove(); } };
}
