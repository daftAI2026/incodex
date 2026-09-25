import { describe, expect, test } from "bun:test";
import {
  discoverOfficialTooltipModuleGraph,
  discoverOfficialTooltipModules,
  createOfficialTooltipRenderer,
  sharedTooltipState,
} from "./official-tooltip-renderer.ts";

describe("official tooltip renderer", () => {
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
});
