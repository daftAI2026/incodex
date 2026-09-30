import {
  discoverOfficialTooltipJsxReceivers,
  loadOfficialTooltipModules,
  readOfficialModuleSource,
  SHARED_MODULE_SOURCE_BUDGET,
} from "./official-tooltip-renderer.ts";

type Fiber = { return?: Fiber | null; memoizedProps?: Record<string, unknown>; pendingProps?: Record<string, unknown> };
type ToastHandle = { close: () => void };
type Toaster = { warning: (title: string, options: Record<string, unknown>) => ToastHandle; registerViewport: () => unknown; custom: (...args: unknown[]) => unknown };
type ToastViewport = { host: HTMLElement; toaster: Toaster };
type Root = { render: (element: unknown) => void; unmount: () => void };
type BannerModules = {
  Banner: unknown;
  CloseIcon: unknown;
  createElement: (type: unknown, props: Record<string, unknown>) => unknown;
  createRoot: (host: HTMLElement) => Root;
};
export type LaunchErrorCopy = { title: string; body: string; retryLabel: string; onRetry: () => void };
export type PrivacyBannerCopy = { title: string; body: string; closeLabel: string; icon: string; onClose: () => void };

function fiberOf(element: HTMLElement): Fiber | null {
  const key = Object.keys(element).find((name) => name.startsWith("__reactFiber$"));
  return key ? (element as unknown as Record<string, Fiber>)[key] ?? null : null;
}

// Read the mounted provider's API. Calling it preserves its React context,
// native warning component, viewport bounds and button styling on every route.
export function findOfficialToaster(doc: Document): ToastViewport | null {
  const matches: ToastViewport[] = [];
  for (const host of doc.querySelectorAll<HTMLElement>(".codex-toast-area")) {
    if (!host.isConnected) continue;
    const candidates = new Set<Toaster>();
    const visited = new Set<Fiber>();
    for (let fiber = fiberOf(host); fiber && !visited.has(fiber); fiber = fiber.return ?? null) {
      visited.add(fiber);
      const value = (fiber.memoizedProps ?? fiber.pendingProps)?.toaster as Partial<Toaster> | undefined;
      if (value && typeof value.warning === "function" && typeof value.registerViewport === "function" && typeof value.custom === "function") {
        candidates.add(value as Toaster);
      }
    }
    if (candidates.size === 1) matches.push({ host, toaster: [...candidates][0]! });
  }
  return matches.length === 1 ? matches[0]! : null;
}

export function discoverOfficialBannerComponent(namespace: Record<string, unknown>): unknown {
  const props = ["actionsPlacement", "attachedToComposer", "description", "dismissAction", "leadingVisual", "title"];
  const candidates = [...new Set(Object.values(namespace))].filter((value) => {
    if (typeof value !== "function") return false;
    const source = Function.prototype.toString.call(value);
    return props.every((prop) => new RegExp(`\\b${prop}\\b`, "u").test(source));
  });
  if (candidates.length !== 1) throw new Error("Official home Banner is unavailable or ambiguous");
  return candidates[0];
}

function escaped(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}

// Current packaged components have lazy initializers. Discover the factory
// which initializes this component's own JSX receiver, rather than relying on
// a neighbouring announcement having happened to initialize it first.
export function initializeOfficialBanner(namespace: Record<string, unknown>, component: unknown, source?: string): void {
  if (typeof component !== "function") throw new Error("Official Banner is unavailable");
  const receivers = discoverOfficialTooltipJsxReceivers(Function.prototype.toString.call(component));
  if (receivers.length !== 1) throw new Error("Official Banner JSX receiver is unavailable or ambiguous");
  const initializer = new RegExp(`\\b${escaped(receivers[0]!)}\\s*=`, "u");
  const candidates: unknown[] = [];
  if (source) {
    for (const { local, exported } of exportBindings(source)) {
      const start = source.indexOf(`function ${local}(`);
      if (start < 0) continue;
      const end = source.indexOf("function ", start + 9);
      const body = source.slice(start, end < 0 ? undefined : end);
      if (initializer.test(body) && new RegExp(`return\\s*\\(\\s*${escaped(local)}\\s*=`, "u").test(body)) {
        candidates.push(namespace[exported]);
      }
    }
  } else {
    candidates.push(...[...new Set(Object.values(namespace))].filter((value) =>
      typeof value === "function" && value !== component &&
        new RegExp(`return\\s*\\(\\s*${escaped(value.name)}\\s*=`, "u").test(Function.prototype.toString.call(value)) &&
        initializer.test(Function.prototype.toString.call(value)),
    ));
  }
  if (candidates.length !== 1) throw new Error("Official Banner initializer is unavailable or ambiguous");
  Reflect.apply(candidates[0] as (...args: unknown[]) => unknown, undefined, []);
}

// Bounds on packaged-source inspection, unrelated to UI size or animation.
const MAX_COMPONENT_HEADER_CHARACTERS = 1024;
const MAX_DISCOVERY_MODULES = 24;
const MAX_DISCOVERY_SOURCE_CHARACTERS = 32_000_000;

export function discoverOfficialBannerCloseIconImport(componentSource: string, moduleSource: string): { specifier: string; imported: string } {
  const action = /(?:\.jsx|\bjsx)\s*\)?\s*\(\s*([\w$]+)\s*,\s*\{action:[^}]*?kind:\s*[`"']dismiss[`"']/u.exec(componentSource)?.[1];
  if (!action) throw new Error("Official Banner dismiss action is unavailable");
  const start = moduleSource.indexOf(`function ${action}(`);
  if (start < 0) throw new Error("Official Banner action implementation is unavailable");
  const end = moduleSource.indexOf("function ", start + "function ".length);
  const actionSource = moduleSource.slice(start, end < 0 ? undefined : end);
  const icon = /(?:\.jsx|\bjsx)\s*\)?\s*\(\s*([\w$]+)\s*,\s*\{className:\s*[\w$]+\.desktopDismiss\b/u.exec(actionSource)?.[1];
  if (!icon) throw new Error("Official Banner dismiss glyph is unavailable");
  const matches: Array<{ specifier: string; imported: string }> = [];
  for (const match of moduleSource.matchAll(/\bimport\s*\{([^}]+)\}\s*from\s*["'`]([^"'`]+)["'`]/gu)) {
    for (const binding of match[1]!.split(",")) {
      const [imported = "", local] = binding.trim().split(/\s+as\s+/u);
      if ((local ?? imported) === icon) matches.push({ specifier: match[2]!, imported });
    }
  }
  if (matches.length !== 1) throw new Error("Official dismiss glyph import is unavailable or ambiguous");
  return matches[0]!;
}

function exportBindings(source: string): Array<{ local: string; exported: string }> {
  const clause = /\bexport\s*\{([^}]+)\}\s*;?\s*(?:\/\/[#@]\s*sourceMappingURL=[^\r\n]*)?\s*$/u.exec(source);
  return (clause?.[1] ?? "").split(",").map((binding) => {
    const [local = "", alias] = binding.trim().split(/\s+as\s+/u);
    return { local, exported: alias ?? local };
  }).filter(({ local, exported }) => /^[\w$]+$/u.test(local) && /^[\w$]+$/u.test(exported));
}

export function hasExportedBanner(source: string): boolean {
  return exportBindings(source).some(({ local }) => {
    const start = source.indexOf(`function ${local}(`);
    if (start < 0) return false;
    const header = source.slice(start, start + MAX_COMPONENT_HEADER_CHARACTERS);
    const parameter = /^function [\w$]+\(([\w$]+)\)/u.exec(header)?.[1];
    if (!parameter) return false;
    const end = new RegExp(`\\}\\s*=\\s*${escaped(parameter)}\\b`, "u").exec(header)?.index;
    if (end === undefined) return false;
    const props = header.slice(0, end);
    return ["actionsPlacement", "attachedToComposer", "description", "dismissAction", "leadingVisual", "title"]
      .every((prop) => new RegExp(`\\b${prop}\\b`, "u").test(props));
  });
}

async function loadOfficialBannerModules(doc: Document): Promise<BannerModules> {
  const react = await loadOfficialTooltipModules(doc);
  const page = new URL(doc.URL);
  if (!["app:", "file:"].includes(page.protocol)) throw new Error("Not a packaged renderer");
  const assets = new URL("./assets/", doc.URL);
  // These are the actual active renderer's packaged modules, including lazy
  // route dependencies. Discover ownership from exported prop capabilities,
  // without storing file prefixes, content hashes or minified export names.
  const loaded = [...new Set([...doc.querySelectorAll<HTMLLinkElement>('link[rel="modulepreload"][href]')]
    .map((link) => new URL(link.href, doc.URL))
    .filter((url) => url.protocol === page.protocol && url.host === page.host && url.pathname.startsWith(assets.pathname) && /\.js$/u.test(url.pathname))
    .map((url) => url.href))];
  let budget = MAX_DISCOVERY_SOURCE_CHARACTERS;
  for (const url of loaded.slice(0, MAX_DISCOVERY_MODULES)) {
    const source = await readOfficialModuleSource(url, Math.min(budget, SHARED_MODULE_SOURCE_BUDGET));
    budget -= source.length;
    if (budget <= 0) throw new Error("Official component discovery source budget exhausted");
    if (!hasExportedBanner(source)) continue;
    const namespace = await import(url) as Record<string, unknown>;
    const Banner = discoverOfficialBannerComponent(namespace);
    initializeOfficialBanner(namespace, Banner, source);
    const glyph = discoverOfficialBannerCloseIconImport(Function.prototype.toString.call(Banner), source);
    const dependency = new URL(glyph.specifier, url);
    if (dependency.protocol !== page.protocol || dependency.host !== page.host || !dependency.pathname.startsWith(assets.pathname)) {
      throw new Error("Official dismiss glyph import escapes packaged assets");
    }
    const icons = await import(dependency.href) as Record<string, unknown>;
    const CloseIcon = icons[glyph.imported];
    if (typeof CloseIcon !== "function" && (typeof CloseIcon !== "object" || CloseIcon === null)) {
      throw new Error("Official dismiss glyph component is unavailable");
    }
    return { ...react, Banner, CloseIcon };
  }
  throw new Error("Official home Banner module is unavailable");
}

const ERROR_TEST_ID = "incodex-launch-error";

export function createOfficialNotifications(
  doc: Document,
  loadBanner: () => Promise<BannerModules> = () => loadOfficialBannerModules(doc),
) {
  let error: LaunchErrorCopy | null = null;
  let toast: { viewport: ToastViewport; handle: ToastHandle; generation: number } | null = null;
  let generation = 0;
  let desired: { slot: HTMLElement; copy: PrivacyBannerCopy } | null = null;
  let mounted: { slot: HTMLElement; copy: PrivacyBannerCopy; host: HTMLElement; root: Root } | null = null;
  let modules: BannerModules | null = null;
  let pending: Promise<BannerModules> | null = null;

  function clearToast() {
    const old = toast;
    toast = null;
    generation += 1;
    old?.handle.close();
  }
  function hideError() { error = null; clearToast(); }
  function ensureError() {
    if (!error) return;
    const viewport = findOfficialToaster(doc);
    if (toast && viewport?.host === toast.viewport.host && viewport.toaster === toast.viewport.toaster) return;
    clearToast();
    if (!viewport) return;
    const ownGeneration = generation;
    const copy = error;
    const handle = viewport.toaster.warning(copy.title, {
      description: copy.body,
      actionPlacement: "inline",
      duration: 0,
      testId: ERROR_TEST_ID,
      primaryAction: { label: copy.retryLabel, onClick: () => {
        if (generation !== ownGeneration || error !== copy) return;
        hideError(); copy.onRetry();
      } },
      onRemove: () => {
        if (generation !== ownGeneration) return;
        if (!viewport.host.isConnected) { toast = null; return; }
        // Keep the removed handle until settlement so DOM reconciliation cannot
        // recreate a warning which the user has just dismissed.
        // A removed provider is a remount, not an acknowledgement of the error.
        queueMicrotask(() => {
          if (generation !== ownGeneration) return;
          toast = null;
          const current = findOfficialToaster(doc);
          if (current?.host === viewport.host && current.toaster === viewport.toaster) error = null;
        });
      },
    });
    toast = { viewport, handle, generation: ownGeneration };
  }
  function removeBanner() {
    const old = mounted; mounted = null;
    old?.root.unmount(); old?.host.remove();
  }
  function sameCopy(a: PrivacyBannerCopy, b: PrivacyBannerCopy) {
    return a.title === b.title && a.body === b.body && a.closeLabel === b.closeLabel && a.icon === b.icon;
  }
  async function ensure(slot: HTMLElement | null, copy: PrivacyBannerCopy | null): Promise<void> {
    ensureError();
    desired = slot && copy ? { slot, copy } : null;
    if (!desired) { removeBanner(); return; }
    if (mounted && mounted.slot === slot && mounted.host.isConnected && sameCopy(mounted.copy, copy!)) return;
    if (!modules) {
      pending ??= loadBanner().catch((cause: unknown) => { pending = null; throw cause; });
      modules = await pending;
    }
    // Always use the latest request after preparation, including dismissal or
    // navigation which happened while the official module was loading.
    const current = desired;
    if (!current?.slot.isConnected) return;
    if (mounted && mounted.slot === current.slot && mounted.host.isConnected && sameCopy(mounted.copy, current.copy)) return;
    if (!mounted || mounted.slot !== current.slot || !mounted.host.isConnected) {
      removeBanner();
      const host = doc.createElement("div");
      host.setAttribute("data-incodex-banner-host", "true");
      current.slot.insertBefore(host, current.slot.firstChild);
      mounted = { slot: current.slot, copy: current.copy, host, root: modules.createRoot(host) };
    }
    const { createElement } = modules;
    const text = (attribute: string, value: string) => createElement("span", {
      [attribute]: "true", children: value,
      ...(attribute === "data-incodex-banner-title" ? { "data-incodex-landing": "true" } : {}),
    });
    // Content is ours; layout props are left to the current original component.
    // Its own leading visual wrapper and icon class determine the glyph size.
    const icon = (source: string) => (props: Record<string, unknown>) => {
      const container = doc.createElement("span");
      container.innerHTML = source;
      const svg = container.firstElementChild;
      const node = (element: Element, root = false): unknown => {
        const attributes = Object.fromEntries([...element.attributes].map(({ name, value }) => [
          name === "class" ? "className" : /^(aria|data)-/u.test(name) ? name : name.replace(/-([a-z])/gu, (_, char: string) => char.toUpperCase()), value,
        ]));
        return createElement(element.tagName.toLowerCase(), {
          ...attributes, ...(root ? props : {}), "aria-hidden": true,
          children: [...element.children].map((child) => node(child)),
        });
      };
      return svg ? node(svg, true) : null;
    };
    mounted.copy = current.copy;
    mounted.root.render(createElement(modules.Banner, {
      title: text("data-incodex-banner-title", current.copy.title),
      description: text("data-incodex-banner-body", current.copy.body),
      leadingVisual: createElement(icon(current.copy.icon), {}),
      dismissAction: { ariaLabel: current.copy.closeLabel, icon: modules.CloseIcon, onClick: current.copy.onClose },
    }));
  }
  return {
    document: doc,
    ensure,
    showError(copy: LaunchErrorCopy) { clearToast(); error = copy; ensureError(); },
    hideError,
    errorPending: () => error !== null,
    bannerNeedsReconcile: (slot: HTMLElement | null) => slot
      ? !mounted || mounted.slot !== slot || !mounted.host.isConnected
      : mounted !== null || desired !== null,
    errorNeedsMount: () => error !== null && !toast?.viewport.host.isConnected,
    dispose() { hideError(); desired = null; removeBanner(); },
  };
}
