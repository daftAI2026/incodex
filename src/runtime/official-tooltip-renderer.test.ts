import { describe, expect, test } from "bun:test";
import { discoverOfficialTooltipModules, createOfficialTooltipRenderer } from "./official-tooltip-renderer.ts";

describe("official tooltip renderer", () => {
  test("discovers hashed packaged modules before any tooltip DOM exists", () => {
    expect(discoverOfficialTooltipModules("app://-/assets/index-123.js", 'const deps=["./react-abc.js","./client-def.js","./tooltip-dismiss-ghi.js","./tooltip-jkl.js"]')).toEqual({
      react: "app://-/assets/react-abc.js", client: "app://-/assets/client-def.js", tooltip: "app://-/assets/tooltip-jkl.js",
    });
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
});
