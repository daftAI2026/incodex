import type { TooltipLifecycle } from "./tooltip-lifecycle.ts";

type SharedTooltipState = {
  lifecycle: TooltipLifecycle | null;
  renderer: ReturnType<typeof createOfficialTooltipRenderer> | null;
};

export function sharedTooltipState(scope: { __incodexTooltipState?: SharedTooltipState }): SharedTooltipState {
  return scope.__incodexTooltipState ??= { lifecycle: null, renderer: null };
}

type ModulePaths = { react: string; client: string; tooltip: string };
type Root = { render: (element: unknown) => void; unmount: () => void };
type RendererModules = {
  createElement: (type: unknown, props: Record<string, unknown>) => unknown;
  createRoot: (host: HTMLElement) => Root;
  Tooltip: unknown;
};

// Read the entry's dependency graph instead of assuming every release keeps
// React and Tooltip in separately named chunks. Rolldown/Vite may merge them.
export function discoverOfficialTooltipModuleGraph(entry: string, source: string): string[] {
  const entryUrl = new URL(entry);
  if (!["app:", "file:"].includes(entryUrl.protocol)) throw new Error("Not a packaged renderer");
  const assetsDirectory = new URL("./", entryUrl);
  const modules = new Set<string>();
  const specifierPattern = /["'`](\.\/[^"'`]+\.js)["'`]/g;
  for (const match of source.matchAll(specifierPattern)) {
    const specifier = match[1]!;
    if (!/^\.\/[A-Za-z0-9_-][A-Za-z0-9._-]*\.js$/u.test(specifier)) continue;
    const resolved = new URL(specifier, entryUrl);
    if (resolved.protocol !== entryUrl.protocol || resolved.host !== entryUrl.host ||
        !resolved.pathname.startsWith(assetsDirectory.pathname)) {
      throw new Error("Official module graph escapes the packaged assets directory");
    }
    modules.add(resolved.href);
    if (modules.size > 256) throw new Error("Official module graph is unexpectedly large");
  }
  return [...modules];
}

// Read the current packaged entry's dependency map, not fixed asset hashes or
// copied CSS. Only direct siblings in that entry's assets directory may load.
export function discoverOfficialTooltipModules(entry: string, source: string): ModulePaths {
  const graph = discoverOfficialTooltipModuleGraph(entry, source);
  const result = {} as ModulePaths;
  for (const name of ["react", "client", "tooltip"] as const) {
    const moduleName = new RegExp(`^${name}-[A-Za-z0-9_]+\\.js$`, "u");
    const paths = graph.filter((url) => moduleName.test(new URL(url).pathname.split("/").at(-1) ?? ""));
    if (paths.length !== 1) throw new Error(`Official ${name} module is unavailable or ambiguous`);
    result[name] = paths[0]!;
  }
  return result;
}

export async function loadOfficialTooltipModules(doc: Document): Promise<RendererModules> {
  const page = new URL(doc.URL);
  if (!["app:", "file:"].includes(page.protocol)) throw new Error("Not a packaged renderer");
  const entries = [...doc.querySelectorAll<HTMLScriptElement>('script[type="module"][src]')]
    .map((script) => new URL(script.src, doc.URL))
    .filter((url) => url.protocol === page.protocol && url.host === page.host &&
      url.pathname.startsWith(new URL("./assets/", doc.URL).pathname) && /\/index-[A-Za-z0-9_-]+\.js$/.test(url.pathname));
  if (entries.length !== 1) throw new Error("Official renderer entry is unavailable or ambiguous");
  const entry = entries[0]!.href;
  const response = await fetch(entry, { signal: AbortSignal.timeout(5000), redirect: "error" });
  if (!response.ok) throw new Error("Cannot read official renderer entry");
  const source = await response.text();
  if (source.length > 2_000_000) throw new Error("Unexpected official entry size");
  const paths = discoverOfficialTooltipModules(entry, source);
  const [reactModule, clientModule, tooltipModule] = await Promise.all([
    import(paths.react), import(paths.client), import(paths.tooltip),
  ]);
  // Rolldown's verified packaged adapter: lazy module exports initialize the
  // same React singleton already used by the app. Do not bundle another React.
  if (typeof reactModule.t !== "function" || typeof clientModule.t !== "function" ||
      typeof tooltipModule.r !== "function" || typeof tooltipModule.t !== "function") {
    throw new Error("Unsupported official Tooltip exports");
  }
  const react = reactModule.t();
  const client = clientModule.t();
  if (typeof react?.createElement !== "function" || typeof client?.createRoot !== "function") {
    throw new Error("Unsupported official React renderer");
  }
  tooltipModule.r();
  return { createElement: react.createElement, createRoot: client.createRoot, Tooltip: tooltipModule.t };
}

const TOOLTIP_ID = "incodex-official-tooltip";

export function createOfficialTooltipRenderer(
  doc: Document,
  load: () => Promise<RendererModules> = () => loadOfficialTooltipModules(doc),
) {
  let modules: RendererModules | null = null;
  let root: Root | null = null;
  let host: HTMLElement | null = null;
  let pending: Promise<void> | null = null;
  let disposed = false;
  let button: HTMLElement | null = null;

  function hide() {
    if (button) {
      const ids = (button.getAttribute("aria-describedby") ?? "").split(/\s+/)
        .filter((id) => id && id !== TOOLTIP_ID);
      if (ids.length) button.setAttribute("aria-describedby", ids.join(" "));
      else button.removeAttribute("aria-describedby");
      button = null;
      root?.render(null);
    }
  }

  return {
    ready: () => !disposed && root !== null && host?.isConnected !== false,
    needsRemount: () => root !== null && host?.isConnected === false,
    prepare(): Promise<void> {
      if (pending) return pending;
      pending = load().then((loaded) => {
        if (disposed) return;
        modules = loaded;
        host = doc.createElement("div");
        host.setAttribute("data-incodex-official-tooltip-root", "true");
        doc.body.append(host);
        root = modules.createRoot(host);
      });
      return pending;
    },
    show(target: HTMLElement, label: string, shortcut: string) {
      if (disposed || !root || !modules || !target.isConnected) return;
      hide();
      button = target;
      target.removeAttribute("title");
      const ids = new Set((target.getAttribute("aria-describedby") ?? "").split(/\s+/).filter(Boolean));
      ids.add(TOOLTIP_ID);
      target.setAttribute("aria-describedby", [...ids].join(" "));
      root.render(modules.createElement(modules.Tooltip, {
        open: true, disableHoverOpen: true, tooltipId: TOOLTIP_ID,
        tooltipContent: label, shortcut, positioningElement: target,
        // Input/dismissal stays in our existing official-provider timing bridge.
        // The official component owns all tooltip DOM, styling and positioning.
        children: modules.createElement("span", { "aria-hidden": true }),
      }));
    },
    hide,
    dispose() {
      if (disposed) return;
      hide();
      disposed = true;
      root?.unmount();
      host?.remove();
      root = null;
      host = null;
    },
  };
}
