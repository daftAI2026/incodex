import { describe, expect, test } from "bun:test";
import {
  SHARED_MODULE_SOURCE_BUDGET,
  discoverCreateRootFactoryExport,
  discoverOfficialDynamicModuleGraph,
  discoverOfficialStaticModuleGraph,
  discoverCalledFactoryExports,
  assertOfficialModuleSourceSize,
  discoverOfficialJsxFactoryExport,
  discoverOfficialTooltipJsxReceivers,
  discoverOfficialJsxRuntime,
  discoverOfficialTooltipModuleGraph,
  discoverOfficialTooltipModules,
  findOfficialSearchButton,
  findOfficialTooltipComponent,
  createOfficialTooltipRenderer,
  sharedTooltipState,
  createOfficialModuleSourceReader,
  createOfficialTooltipModuleLoader,
} from "./official-tooltip-renderer.ts";

describe("official tooltip renderer", () => {
  test("reads bounded literal root-consumer sources during entry preparation rather than after Search appears", async () => {
    const reads: Array<{ url: string; budget?: number }> = [];
    let searchQueries = 0;
    const doc = {
      URL: "app://-/index.html",
      querySelectorAll(selector: string) {
        if (selector.startsWith("script")) return [{ src: "app://-/assets/index-current.js" }];
        searchQueries++;
        return [];
      },
    } as unknown as Document;
    const loader = createOfficialTooltipModuleLoader(doc, async (url, budget) => {
      reads.push({ url, budget });
      return url.endsWith("index-current.js")
        ? 'import{t}from"./shared-current.js";import("./consumer-one.js");import("./consumer-two.js");'
        : "current consumer source";
    }, async () => ({}));
    await loader.prepare();
    expect(reads).toEqual([
      { url: "app://-/assets/index-current.js", budget: undefined },
      { url: "app://-/assets/consumer-one.js", budget: 512_000 },
      { url: "app://-/assets/consumer-two.js", budget: 512_000 },
    ]);
    expect(searchQueries).toBe(0);
  });
  test("ambiguous Tooltip capability hints do not pre-read or choose a module", async () => {
    const reads: string[] = [];
    const doc = { URL: "app://-/index.html", querySelectorAll: () => [{ src: "app://-/assets/index-renamed.js" }] } as unknown as Document;
    const loader = createOfficialTooltipModuleLoader(doc, async (url) => {
      reads.push(url);
      return 'import{t}from"./first.js";import{t as second}from"./second.js";';
    }, async () => ({ arbitraryExport: (props: { tooltipContent: string }) => props.tooltipContent }));
    await loader.prepare();
    expect(reads).toEqual(["app://-/assets/index-renamed.js"]);
  });
  test("pre-reads a unique static Tooltip capability owner before Search mounts, without selecting its component", async () => {
    const reads: Array<{ url: string; budget?: number }> = [];
    let searchQueries = 0;
    const doc = {
      URL: "app://-/index.html",
      querySelectorAll(selector: string) {
        if (selector.startsWith("script")) return [{ src: "app://-/assets/index-renamed.js" }];
        searchQueries++;
        return [];
      },
    } as unknown as Document;
    const loader = createOfficialTooltipModuleLoader(doc, async (url, budget) => {
      reads.push({ url, budget });
      return url.endsWith("index-renamed.js") ? 'import{t}from"./any-name.js";' : "current shared source";
    }, async () => ({ arbitraryExport: (props: { tooltipContent: string }) => props.tooltipContent }));
    await loader.prepare();
    expect(reads).toEqual([
      { url: "app://-/assets/index-renamed.js", budget: undefined },
      { url: "app://-/assets/any-name.js", budget: SHARED_MODULE_SOURCE_BUDGET },
    ]);
    expect(searchQueries).toBe(0);
    await expect(loader.load()).rejects.toThrow("Official Search trigger is unavailable or ambiguous");
  });
  test("repeated injections reuse the current window's prepared loader instead of reading its entry again", async () => {
    const scope = {};
    let reads = 0;
    const doc = { URL: "app://-/index.html", querySelectorAll: () => [{ src: "app://-/assets/index-current.js" }] } as unknown as Document;
    const acquire = () => createOfficialTooltipModuleLoader(doc, async () => {
      reads++;
      return 'import{t}from"./shared-current.js";';
    }, async () => ({}));
    await sharedTooltipState(scope, acquire).moduleLoader!.prepare();
    await sharedTooltipState(scope, acquire).moduleLoader!.prepare();
    expect(reads).toBe(1);
  });
  test("prepares the live entry and its static imports before Search exists, releases the snapshot after consumption", async () => {
    let reads = 0;
    let imports = 0;
    let searchQueries = 0;
    const doc = {
      URL: "app://-/index.html",
      querySelectorAll(selector: string) {
        if (selector.startsWith("script")) return [{ src: "app://-/assets/index-current.js" }];
        searchQueries++;
        return [];
      },
    } as unknown as Document;
    const loader = createOfficialTooltipModuleLoader(doc, async () => {
      reads++;
      return 'import{t}from"./shared-current.js";';
    }, async () => { imports++; return {}; });
    await loader.prepare();
    await loader.prepare();
    expect(reads).toBe(1);
    expect(imports).toBe(1);
    expect(searchQueries).toBe(0);
    await expect(loader.load()).rejects.toThrow("Official Search trigger is unavailable or ambiguous");
    expect(searchQueries).toBe(1);
    await loader.prepare();
    expect(reads).toBe(2);
    expect(imports).toBe(2);
  });
  test("a failed early entry read releases preparation and a later attempt reads the current entry again", async () => {
    let reads = 0;
    const doc = { URL: "app://-/index.html", querySelectorAll: () => [{ src: "app://-/assets/index-current.js" }] } as unknown as Document;
    const loader = createOfficialTooltipModuleLoader(doc, async () => {
      if (++reads === 1) throw Error("official entry temporarily unavailable");
      return 'import{t}from"./shared-current.js";';
    }, async () => ({}));
    await expect(loader.prepare()).rejects.toThrow("official entry temporarily unavailable");
    await loader.prepare();
    expect(reads).toBe(2);
  });
  test("coalesces only in-flight module reads with the same source budget, never caches completed source", async () => {
    const reads: Array<{ url: string; budget?: number; resolve: (source: string) => void; reject: (cause: Error) => void }> = [];
    const read = createOfficialModuleSourceReader((url, budget) => new Promise((resolve, reject) => { reads.push({ url, budget, resolve, reject }); }));
    const first = read("app://-/assets/current.js", 16000);
    const second = read("app://-/assets/current.js", 16000);
    expect(reads).toHaveLength(1);
    expect(first).toBe(second);
    const smaller = read("app://-/assets/current.js", 512);
    expect(reads).toHaveLength(2);
    expect(smaller).not.toBe(first);
    reads[0]!.resolve("same current source");
    reads[1]!.resolve("small source");
    expect(await second).toBe("same current source");
    await smaller;
    const next = read("app://-/assets/current.js", 16000);
    expect(reads).toHaveLength(3);
    reads[2]!.reject(Error("read failed"));
    await expect(next).rejects.toThrow("read failed");
    const retry = read("app://-/assets/current.js", 16000);
    expect(reads).toHaveLength(4);
    reads[3]!.resolve("new read after failure");
    expect(await retry).toBe("new read after failure");
  });
  test("shares current-window renderer capabilities with Banner while preparation is in flight", async () => {
    let loads = 0;
    let resolve!: (value: any) => void;
    const host = { setAttribute() {}, remove() {} };
    const doc = { createElement: () => host, body: { append() {} } } as unknown as Document;
    const modules = { Tooltip: () => {}, createElement: () => ({}), createRoot: () => ({ render() {}, unmount() {} }) };
    const renderer = createOfficialTooltipRenderer(doc, () => { loads += 1; return new Promise((done) => { resolve = done; }); });
    const preparation = renderer.prepare();
    const bannerCapabilities = renderer.preparedModules();
    expect(loads).toBe(1);
    resolve(modules);
    await preparation;
    expect(await bannerCapabilities).toBe(modules);
    expect(await renderer.preparedModules()).toBe(modules);
    expect(loads).toBe(1);
    renderer.dispose();
    await expect(renderer.preparedModules()).rejects.toThrow("disposed");
  });
  test("repeated injections share the lifecycle seen by old dismissal listeners", () => {
    const scope = {};
    const first = sharedTooltipState(scope);
    const dismiss = () => first.lifecycle?.dismiss();
    const second = sharedTooltipState(scope);
    let dismissed = false;
    second.lifecycle = { dismiss: () => { dismissed = true; } } as NonNullable<typeof second.lifecycle>;
    dismiss();
    expect(first).toBe(second);
    expect(dismissed).toBe(true);
  });
  test("discovers hashed packaged modules before any tooltip DOM exists", () => {
    expect(discoverOfficialTooltipModules("app://-/assets/index-123.js", 'const deps=["./react-abc.js","./client-def.js","./tooltip-dismiss-ghi.js","./tooltip-jkl.js"]')).toEqual({
      react: "app://-/assets/react-abc.js", client: "app://-/assets/client-def.js", tooltip: "app://-/assets/tooltip-jkl.js",
    });
  });
  test("walks the current shared-chunk graph without depending on its content hash", () => {
    const entry = "app://-/assets/index-current.js";
    const source = [
      'const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["./rpc-current.js","./app-initial-current.js","./rolldown-runtime-current.js","./app-shared-current.js","./app-main-current.js"])))=>i.map(i=>d[i]);',
      'import{n as e}from"./rolldown-runtime-current.js";',
      'import{H3 as n,V3 as r,W3 as i}from"./app-shared-current.js";',
      'await import("./app-main-current.js");',
    ].join("");

    expect(discoverOfficialTooltipModuleGraph(entry, source)).toEqual([
      "app://-/assets/rpc-current.js",
      "app://-/assets/app-initial-current.js",
      "app://-/assets/rolldown-runtime-current.js",
      "app://-/assets/app-shared-current.js",
      "app://-/assets/app-main-current.js",
    ]);
  });
  test("limits shared runtime discovery to static modules reachable from the active entry", () => {
    expect(discoverOfficialStaticModuleGraph(
      "app://-/assets/index-current.js",
      'import{n as runtime}from"./runtime-build.js";import{a as app}from"./shared-build.js";import"./styles-build.js";export{x}from"./reexport-build.js";const preload="./unrelated-build.js";const expression=/import{bad}from".\\/regex-build.js"/;/* import"./comment-build.js" */await import("./lazy-build.js");',
    )).toEqual([
      "app://-/assets/runtime-build.js",
      "app://-/assets/shared-build.js",
      "app://-/assets/styles-build.js",
      "app://-/assets/reexport-build.js",
    ]);
  });
  test("finds direct dynamic imports without treating preload-only URLs as dependencies", () => {
    expect(discoverOfficialDynamicModuleGraph(
      "app://-/assets/index-current.js",
      'const deps=(m.f||(m.f=["./preload-only.js"]));import{shared}from"./shared.js";await load(()=>import(`./main-current.js`),preload([0]));',
    )).toEqual([
      "app://-/assets/shared.js",
      "app://-/assets/main-current.js",
    ]);
  });
  test("keeps the observed shared chunk under a dedicated source budget", () => {
    const observedSharedChunk = "x".repeat(7_071_636);
    expect(() => assertOfficialModuleSourceSize(observedSharedChunk, SHARED_MODULE_SOURCE_BUDGET)).not.toThrow();
    expect(SHARED_MODULE_SOURCE_BUDGET).toBeGreaterThanOrEqual(2 * observedSharedChunk.length);
    expect(() => assertOfficialModuleSourceSize(observedSharedChunk, 2_000_000)).toThrow();
  });
  test("requires a unique localized official Search trigger", () => {
    const search = { getAttribute: () => "搜索" } as unknown as HTMLElement;
    expect(findOfficialSearchButton({ querySelectorAll: () => [search] } as unknown as Document)).toBe(search);
    expect(() => findOfficialSearchButton({ querySelectorAll: () => [] } as unknown as Document)).toThrow();
    expect(() => findOfficialSearchButton({ querySelectorAll: () => [search, search] } as unknown as Document)).toThrow();
  });
  test("fails closed on a static import that escapes the packaged asset directory", () => {
    expect(() => discoverOfficialStaticModuleGraph(
      "app://-/assets/index-current.js",
      'import{runtime}from"../outside.js";',
    )).toThrow();
  });
  test("keeps the old direct-module layout in the same local graph", () => {
    expect(discoverOfficialTooltipModuleGraph(
      "app://-/assets/index-old.js",
      'import{t}from"./react-a1.js";import{t as e}from"./client-b2.js";import{r,t}from"./tooltip-c3.js";',
    )).toEqual([
      "app://-/assets/react-a1.js",
      "app://-/assets/client-b2.js",
      "app://-/assets/tooltip-c3.js",
    ]);
  });
  test("resolves the root factory through the current consumer import and use", () => {
    const importer = "app://-/assets/current-main.js";
    const shared = "app://-/assets/current-shared.js";
    const source = [
      'import{Provider as setup,Module as loader}from"./current-shared.js";',
      'let root;function mount(){root=loader();window.root??=(0,root.createRoot)(element)}',
    ].join("");

    expect(discoverCreateRootFactoryExport(importer, source, shared)).toBe("Module");
  });
  test("finds only no-argument factories actually called by the current consumer", () => {
    expect(discoverCalledFactoryExports(
      "app://-/assets/current-main.js",
      'import{Jsx as warm,Root as createRoot}from"./shared.js";const root=createRoot();warm();window.root=root;',
      "app://-/assets/shared.js",
    )).toEqual(["Jsx", "Root"]);
  });
  test("maps the live Tooltip JSX receiver through its initializer and consumer export", () => {
    const runtimeVar = { jsx: (type: unknown, props: Record<string, unknown>) => ({ type, props }) };
    function TooltipMock() { return runtimeVar.jsx("span", {}); }
    const sharedSource = [
      Function.prototype.toString.call(TooltipMock),
      "var runtimeVar;function initialize(){runtimeVar=lazyFactory();}",
      "export{lazyFactory as opaqueGetter};",
    ].join("");
    const importerSource = 'import{opaqueGetter as warm}from"./shared.js";warm();';
    expect(discoverOfficialJsxFactoryExport(
      TooltipMock, sharedSource, "app://-/assets/main.js", importerSource, "app://-/assets/shared.js",
    )).toBe("opaqueGetter");
  });
  test("recognizes the official bundler's unbound JSX call form", () => {
    expect(discoverOfficialTooltipJsxReceivers("function T(e){return(0,R3.jsx)(R3.Fragment,e)}"))
      .toEqual(["R3"]);
  });
  test("invokes only the statically selected getter and validates official JSX capabilities", () => {
    const expected = { Fragment: Symbol.for("react.fragment"), jsx() {}, jsxs() {} };
    let invoked = 0;
    const namespace = {
      officialFactory: () => { invoked++; return expected; },
      unrelated: () => { throw new Error("must not be called"); },
    };
    expect(discoverOfficialJsxRuntime(namespace, "officialFactory")).toBe(expected);
    expect(invoked).toBe(1);
    expect(() => discoverOfficialJsxRuntime({ officialFactory: () => ({ jsx() {} }) }, "officialFactory")).toThrow();
  });
  test("reuses the exported Tooltip type present in the live Search fiber", () => {
    function OfficialTooltip() {}
    const trigger = {
      "__reactFiber$runtime": {
        return: {
          type: OfficialTooltip,
          memoizedProps: { tooltipContent: "Search", children: {} },
        },
      },
    } as unknown as HTMLElement;

    expect(findOfficialTooltipComponent(trigger, { opaqueExport: OfficialTooltip })).toBe(OfficialTooltip);
    expect(() => findOfficialTooltipComponent(trigger, { unrelated: function Other() {} })).toThrow();
  });
  test("rejects external, traversing, ambiguous, or incomplete module sources", () => {
    for (const source of [
      '"https://evil.test/react-a.js","./client-b.js","./tooltip-c.js"',
      '"../react-a.js","./client-b.js","./tooltip-c.js"',
      '"./react-a.js","./react-b.js","./client-c.js","./tooltip-d.js"',
      '"./react-a.js","./client-b.js","./tooltip-dismiss-c.js"',
    ]) expect(() => discoverOfficialTooltipModules("app://-/assets/index-a.js", source)).toThrow();
  });
  test("renders the actual official component on first show without Search sampling", async () => {
    const renders: unknown[] = [];
    let unmounted = 0;
    let removed = 0;
    const host = { setAttribute() {}, remove() { removed++; } };
    const doc = { createElement: () => host, body: { append() {} } } as unknown as Document;
    const component = () => {};
    const renderer = createOfficialTooltipRenderer(doc, async () => ({
      createElement: (type: unknown, props: unknown) => ({ type, props }),
      createRoot: () => ({ render: (value: unknown) => renders.push(value), unmount: () => { unmounted++; } }),
      Tooltip: component,
    }));
    await renderer.prepare();
    const attrs = new Map<string, string>([["aria-describedby", "existing"]]);
    const button = { isConnected: true, getAttribute: (k: string) => attrs.get(k) ?? null, setAttribute: (k: string,v: string) => attrs.set(k,v), removeAttribute: (k: string) => attrs.delete(k) } as unknown as HTMLElement;
    renderer.show(button, "Open incognito", "Ctrl+Shift+N");
    expect(renders.at(-1)).toMatchObject({ type: component, props: { open: true, tooltipContent: "Open incognito", shortcut: "Ctrl+Shift+N", positioningElement: button } });
    expect(attrs.get("aria-describedby")).toContain("incodex-official-tooltip");
    renderer.hide();
    expect(attrs.get("aria-describedby")).toBe("existing");
    expect(renders.at(-1)).toBeNull();
    renderer.dispose();
    expect(unmounted).toBe(1);
    expect(removed).toBe(1);
  });
  test("passes the current Search direction and offset to the hat Tooltip on each show", async () => {
    const renders: Array<{ props: Record<string, unknown> }> = [];
    function OfficialTooltip() {}
    let direction = "bottom";
    const searchProps = { tooltipContent: "Search", children: {}, sideOffset: 6 };
    const search = {
      "__reactFiber$runtime": {
        return: { type: OfficialTooltip, memoizedProps: searchProps },
      },
    } as unknown as HTMLElement;
    const host = { setAttribute() {}, isConnected: true, remove() {} };
    const renderer = createOfficialTooltipRenderer(
      {
        createElement: () => host, body: { append() {} },
        defaultView: { getComputedStyle: () => ({ getPropertyValue: (name: string) => name === "--side-tooltip" ? direction : "" }) },
      } as unknown as Document,
      async () => ({
        createElement: (_type: unknown, props: Record<string, unknown>) => ({ props }),
        createRoot: () => ({ render: (value: unknown) => { if (value) renders.push(value as { props: Record<string, unknown> }); }, unmount() {} }),
        Tooltip: OfficialTooltip,
      }),
    );
    await renderer.prepare();
    const button = {
      isConnected: true, getAttribute: () => null, setAttribute() {}, removeAttribute() {},
    } as unknown as HTMLElement;
    renderer.show(button, "Open incognito", "Shift+Command+N", search);
    expect(renders.at(-1)?.props.side).toBe("bottom");
    expect(renders.at(-1)?.props.sideOffset).toBe(6);
    direction = "top";
    searchProps.sideOffset = 9;
    renderer.show(button, "Open incognito", "Shift+Command+N", search);
    expect(renders.at(-1)?.props.side).toBe("top");
    expect(renders.at(-1)?.props.sideOffset).toBe(9);
    renderer.dispose();
  });
  test("inherits the live Search zoom context instead of scaling tooltip pixels", async () => {
    let zoom = 1.25;
    const zoomContext = { Provider: {}, _currentValue: 1 };
    const providerFiber = { type: zoomContext, memoizedProps: { value: zoom }, return: null };
    const search = { "__reactFiber$live": { return: providerFiber } } as unknown as HTMLElement;
    const renders: Array<{ type: unknown; props: Record<string, unknown> }> = [];
    const host = { setAttribute() {}, isConnected: true, remove() {} };
    const doc = {
      createElement: () => host,
      body: { append() {} },
      defaultView: { getComputedStyle: () => ({ getPropertyValue: (name: string) => name === "--codex-window-zoom" ? String(zoom) : "" }) },
    } as unknown as Document;
    const tooltip = () => {};
    const renderer = createOfficialTooltipRenderer(doc, async () => ({
      createElement: (type: unknown, props: Record<string, unknown>) => ({ type, props }),
      createRoot: () => ({ render(value: unknown) { if (value) renders.push(value as typeof renders[number]); }, unmount() {} }),
      Tooltip: tooltip,
    }));
    await renderer.prepare();
    const button = { isConnected: true, getAttribute: () => null, setAttribute() {}, removeAttribute() {} } as unknown as HTMLElement;
    renderer.show(button, "Open incognito", "Ctrl+Shift+N", search);
    expect(renders.at(-1)).toMatchObject({ type: zoomContext.Provider, props: { value: zoom, children: { type: tooltip } } });
    zoom = 1.4;
    providerFiber.memoizedProps.value = zoom;
    renderer.show(button, "Open incognito", "Ctrl+Shift+N", search);
    expect(renders.at(-1)).toMatchObject({ type: zoomContext.Provider, props: { value: zoom, children: { type: tooltip } } });
    zoom = 1;
    providerFiber.memoizedProps.value = zoom;
    renderer.show(button, "Open incognito", "Ctrl+Shift+N", search);
    expect(renders.at(-1)?.type).toBe(tooltip);
    renderer.dispose();
  });
  test("does not attach a late module result after disposal", async () => {
    let resolve!: (value: never) => void;
    let hosts = 0;
    const doc = { createElement: () => { hosts++; } } as unknown as Document;
    const renderer = createOfficialTooltipRenderer(doc, () => new Promise((done) => { resolve = done; }));
    const ready = renderer.prepare();
    renderer.dispose();
    resolve({} as never);
    await ready;
    expect(hosts).toBe(0);
    expect(renderer.ready()).toBe(false);
  });
  test("reports initialization failure without adding any fallback DOM", async () => {
    const renderer = createOfficialTooltipRenderer({} as Document, async () => { throw new Error("unsupported build"); });
    await expect(renderer.prepare()).rejects.toThrow("unsupported build");
    expect(renderer.ready()).toBe(false);
    renderer.hide();
    renderer.dispose();
  });
  test("allows a later Search remount to retry a transient module-load failure", async () => {
    let attempts = 0;
    const host = { setAttribute() {}, isConnected: true, remove() {} };
    const doc = { createElement: () => host, body: { append() {} } } as unknown as Document;
    const renderer = createOfficialTooltipRenderer(doc, async () => {
      if (++attempts === 1) throw new Error("Search is remounting");
      return {
        createElement: () => ({}),
        createRoot: () => ({ render() {}, unmount() {} }),
        Tooltip: () => {},
      };
    });
    expect(renderer.needsPreparation()).toBe(true);
    await expect(renderer.prepare()).rejects.toThrow("Search is remounting");
    expect(renderer.needsPreparation()).toBe(true);
    await renderer.prepare();
    expect(attempts).toBe(2);
    expect(renderer.ready()).toBe(true);
    renderer.dispose();
  });
  test("removes an orphan tooltip host before retrying failed React root creation", async () => {
    let attempts = 0;
    let removed = 0;
    const doc = {
      createElement: () => ({ setAttribute() {}, isConnected: true, remove() { removed++; } }),
      body: { append() {} },
    } as unknown as Document;
    const renderer = createOfficialTooltipRenderer(doc, async () => ({
      createElement: () => ({}),
      createRoot: () => {
        if (++attempts === 1) throw new Error("root unavailable");
        return { render() {}, unmount() {} };
      },
      Tooltip: () => {},
    }));
    await expect(renderer.prepare()).rejects.toThrow("root unavailable");
    expect(removed).toBe(1);
    await renderer.prepare();
    expect(renderer.ready()).toBe(true);
    renderer.dispose();
    expect(removed).toBe(2);
  });
});
