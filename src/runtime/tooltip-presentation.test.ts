/**
 * [INPUT]: 依赖 tooltip-presentation 的官方关联取样与原生提示文案
 * [OUTPUT]: 约束无样本降级、动态 class、触发器替换与截图快捷键边界
 * [POS]: Runtime tooltip 的纯行为回归，不以固定颜色作为预期
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { nativeTooltipTitle, parseOfficialWindowZoom } from "./tooltip-presentation.ts";

describe("official tooltip presentation", () => {
  test("uses the live Codex window zoom with a safe default", () => {
    expect(parseOfficialWindowZoom("1.2")).toBe(1.2);
    expect(parseOfficialWindowZoom(" 0.8 ")).toBe(0.8);
    expect(parseOfficialWindowZoom("")).toBe(1);
    expect(parseOfficialWindowZoom("0")).toBe(1);
    expect(parseOfficialWindowZoom("not-a-number")).toBe(1);
  });
});

// Official selectors are discovered from the live trigger relationship, not
// from a hardcoded palette or an unrelated tooltip elsewhere in the page.
import { createOfficialTooltipPresentation } from "./tooltip-presentation.ts";

function fixture() {
  const elements = new Map<string, unknown>();
  const attrs = new Map<string, string>();
  const parent = { getAttribute: (key: string) => attrs.get(key) ?? null };
  const trigger = {
    isConnected: true,
    parentElement: parent,
    ownerDocument: { getElementById: (id: string) => elements.get(id) ?? null },
    getAttribute: () => null,
    getBoundingClientRect: () => ({ top: 56, bottom: 84, left: 300, right: 328 }),
  };
  const tooltip = (className: string) => ({
    isConnected: true,
    className,
    getAttribute: (key: string) => (key === "role" ? "tooltip" : null),
    hasAttribute: () => false,
    querySelector: () => ({ className: "official-shortcut-token" }),
  });
  return { elements, attrs, trigger, tooltip };
}

describe("live official tooltip token selection", () => {
  test("keeps the Search tooltip's observed bottom side and gap for fallback placement", () => {
    const f = fixture();
    f.attrs.set("aria-describedby", "search-tip");
    const tip = {
      ...f.tooltip("official"),
      getAttribute: (key: string) => key === "role" ? "tooltip" : key === "data-side" ? "bottom" : null,
      getBoundingClientRect: () => ({ top: 90, bottom: 120, left: 271, right: 356 }),
    };
    f.elements.set("search-tip", tip);
    const bridge = createOfficialTooltipPresentation();
    expect(bridge.read(f.trigger as unknown as HTMLElement)).toMatchObject({ side: "bottom", gap: 6 });
    f.elements.clear();
    expect(bridge.read(f.trigger as unknown as HTMLElement)).toMatchObject({ side: "bottom", gap: 6 });
  });
  test("does not guess a palette before the official tooltip exists", () => {
    const f = fixture();
    const bridge = createOfficialTooltipPresentation();
    expect(bridge.read(f.trigger as unknown as HTMLElement)).toBeNull();
    f.elements.set("unrelated", f.tooltip("bg-unrelated"));
    expect(bridge.read(f.trigger as unknown as HTMLElement)).toBeNull();
  });

  test("learns current official classes and follows a later token change", () => {
    const f = fixture();
    const bridge = createOfficialTooltipPresentation();
    f.attrs.set("aria-describedby", "search-tip");
    const tip = f.tooltip("rounded-future bg-future-surface text-future-label border-future");
    f.elements.set("search-tip", tip);
    expect(bridge.read(f.trigger as unknown as HTMLElement)).toEqual({
      className: tip.className,
      shortcutClassName: "official-shortcut-token",
    });
    tip.className = "rounded-next bg-next-surface text-next-label";
    expect(bridge.read(f.trigger as unknown as HTMLElement)?.className).toBe(tip.className);
  });

  test("retains semantic classes after portal unmount, not resolved RGB", () => {
    const f = fixture();
    const bridge = createOfficialTooltipPresentation();
    f.attrs.set("aria-describedby", "search-tip");
    f.elements.set("search-tip", f.tooltip("bg-live-token dark:bg-other-live-token"));
    bridge.read(f.trigger as unknown as HTMLElement);
    f.attrs.delete("aria-describedby");
    f.elements.clear();
    expect(bridge.read(f.trigger as unknown as HTMLElement)?.className).toBe(
      "bg-live-token dark:bg-other-live-token",
    );
  });

  test("does not inherit a sample across replacement of the official trigger", () => {
    const f = fixture();
    const bridge = createOfficialTooltipPresentation();
    f.attrs.set("aria-describedby", "search-tip");
    f.elements.set("search-tip", f.tooltip("bg-old"));
    bridge.read(f.trigger as unknown as HTMLElement);
    const replacement = fixture();
    expect(bridge.read(replacement.trigger as unknown as HTMLElement)).toBeNull();
    f.trigger.isConnected = false;
    expect(bridge.read(f.trigger as unknown as HTMLElement)).toBeNull();
  });

  test("rejects non-tooltip and injected self-samples", () => {
    const f = fixture();
    const bridge = createOfficialTooltipPresentation();
    f.attrs.set("aria-describedby", "description injected good");
    f.elements.set("description", { ...f.tooltip("bg-not-tooltip"), getAttribute: () => "note" });
    f.elements.set("injected", { ...f.tooltip("bg-self"), hasAttribute: () => true });
    expect(bridge.read(f.trigger as unknown as HTMLElement)).toBeNull();
    f.elements.set("good", f.tooltip("bg-official"));
    expect(bridge.read(f.trigger as unknown as HTMLElement)?.className).toBe("bg-official");
  });
});

// 相机入口不得冒用无痕快捷键。
describe("native tooltip fallback", () => {
  test("keeps capture hints free of the incognito shortcut", () => {
    expect(nativeTooltipTitle("截取窗口", "")).toBe("截取窗口");
    expect(nativeTooltipTitle("打开无痕窗口", "⇧⌘N")).toBe("打开无痕窗口 (⇧⌘N)");
  });
});
