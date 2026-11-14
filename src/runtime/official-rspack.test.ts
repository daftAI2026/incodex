/**
 * [INPUT]: 依赖官方 Rspack 只读缓存适配和多入口 Tooltip 加载器。
 * [OUTPUT]: 验证无文件顺序假设、原 React 实例复用、不可枚举 API 与 proxy 排除。
 * [POS]: 新旧打包结构的隔离回归；不执行官方模块工厂或启动宿主。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { createOfficialTooltipModuleLoader } from "./official-tooltip-renderer.ts";
import { loadOfficialRspackRuntime, discoverOfficialRspackReact } from "./official-rspack.ts";

describe("official cached Rspack capabilities", () => {
  test("keeps the original single-entry route without importing an entry", async () => {
    const doc = { URL: "file:///app/index.html", querySelectorAll: () => [{ src: "file:///app/assets/entry.js" }] } as unknown as Document;
    expect(await loadOfficialRspackRuntime(doc, async () => { throw Error("legacy entry must retain its loader"); })).toBeNull();
  });

  test("selects the loaded runtime by capability in any script position, without invoking it", async () => {
    const namespace = { Component() {} };
    const require = Object.assign(() => { throw Error("factory execution forbidden"); }, { c: { first: { exports: namespace } } });
    Object.defineProperty(require, "m", { get() { throw Error("factory enumeration forbidden"); } });
    for (const index of [0, 1, 2]) {
      const scripts = [0, 1, 2].map(i => ({ src: `app://-/assets/${i}.js` }));
      const doc = { URL: "app://-/index.html", querySelectorAll: () => scripts } as unknown as Document;
      const imports: string[] = [];
      const runtime = await loadOfficialRspackRuntime(doc, async url => { imports.push(url); return url.endsWith(`/${index}.js`) ? { __webpack_require__: require } : {}; });
      expect(runtime?.values).toContain(namespace.Component);
      expect(imports).toEqual(scripts.map(s => s.src));
    }
  });

  test("uses nonenumerable APIs and aliases by exact identity, ignoring unowned proxy capabilities", () => {
    const element = () => ({}), root = () => ({ render() {}, unmount() {} });
    const react = { createElement: element, useState() {} };
    const client = Object.defineProperty({}, "createRoot", { value: root });
    const proxy = new Proxy({}, { get() { return () => {}; } });
    const found = discoverOfficialRspackReact({ values: [react, { ...react }, client, proxy], namespaces: [] });
    expect(found.createElement("div", {})).toEqual({});
    expect(typeof found.createRoot({} as HTMLElement).unmount).toBe("function");
    expect(() => discoverOfficialRspackReact({ values: [react, { createElement() {}, useState() {} }, client], namespaces: [] })).toThrow("ambiguous");
  });

  test("reads only same-package active scripts and tolerates an unrelated unreadable export", async () => {
    const exports = Object.defineProperty({ Component() {} }, "unreadable", { get() { throw Error("unrelated"); } });
    const require = Object.assign(() => {}, { c: { existing: { exports } } });
    const scripts = ["app://-/assets/part.js", "app://-/assets/runtime.js", "app://foreign/assets/a.js", "app://-/outside.js"];
    const doc = { URL: "app://-/index.html", querySelectorAll: () => scripts.map(src => ({ src })) } as unknown as Document;
    const imports: string[] = [];
    const runtime = await loadOfficialRspackRuntime(doc, async url => { imports.push(url); return url.endsWith("runtime.js") ? { __webpack_require__: require } : {}; });
    expect(imports).toEqual(scripts.slice(0, 2));
    expect(runtime?.values).toContain(exports.Component);
  });
  test("reuses a capability-selected Rspack runtime among several active scripts without reading sources", async () => {
    function Tooltip() {}
    const search = {
      getAttribute: (name: string) => name === "aria-label" ? "Search" : null,
      __reactFiber$rspack: { type: Tooltip, elementType: Tooltip, memoizedProps: { tooltipContent: "Search", children: {}, side: "bottom" }, return: null },
    } as unknown as HTMLElement;
    const scripts = ["vendor", "ui", "runtime", "locale", "extra"].map(name => ({ src: `app://-/assets/${name}.js` }));
    const doc = { URL: "app://-/index.html", querySelectorAll: (selector: string) => selector.startsWith("script") ? scripts : selector === "button" ? [search] : [] } as unknown as Document;
    const createElement = (type: unknown, props: Record<string, unknown>) => ({ type, props });
    const root = { render() {}, unmount() {} };
    const client = Object.defineProperty({}, "createRoot", { value: () => root });
    const require = Object.assign(() => { throw Error("must not execute module factories"); }, { c: {
      react: { exports: { createElement, useState() {} } }, client: { exports: client }, tooltip: { exports: { Tooltip } },
    } });
    const reads: string[] = [];
    const loader = createOfficialTooltipModuleLoader(doc, async url => { reads.push(url); throw Error("source scan forbidden"); }, async url => url.endsWith("runtime.js") ? { __webpack_require__: require } : { __webpack_modules__: {} });
    await loader.prepare();
    const modules = await loader.load();
    expect(modules.Tooltip).toBe(Tooltip);
    expect(modules.createElement(Tooltip, {})).toEqual({ type: Tooltip, props: {} });
    expect(modules.createRoot({} as HTMLElement)).toBe(root);
    expect(reads).toEqual([]);
  });
});
