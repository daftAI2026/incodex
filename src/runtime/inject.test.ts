import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { iconFor } from "./incognito-icon";

const inject = readFileSync(join(import.meta.dir, "inject.ts"), "utf8");
const hatGlasses = readFileSync(join(import.meta.dir, "../../assets/hat-glasses.svg"), "utf8");
const circleX = readFileSync(join(import.meta.dir, "../../assets/circle-x.svg"), "utf8");

describe("hat-glasses stays after header remount", () => {
  test("does not disconnect the observer once the button exists", () => {
    expect(inject).not.toContain("observer.disconnect");
    expect(inject).not.toMatch(/if \(uiReady\(\)\) return;/);
  });

  test("parks the button before the Search tooltip trigger boundary", () => {
    expect(inject).toContain("searchButtonPlacement(search)");
    expect(inject).toContain("placement.parent.insertBefore(btn, placement.before)");
    expect(inject).not.toContain("search.parentElement.insertBefore(btn, search)");
    expect(inject).not.toContain("cluster.insertBefore(btn, cluster.firstElementChild)");
  });

  test("watches documentElement so a replaced sidebar cluster is still seen", () => {
    expect(inject).toContain("document.documentElement");
    expect(inject).not.toContain("observer.observe(observeRoot()");
  });

  test("skips ensureButton while the hat is still beside the Search trigger", () => {
    expect(inject).toContain("buttonStillBesideSearch");
    expect(inject).toContain("if (!needsInject()) return");
  });
});

describe("incognito button exit affordance", () => {
  test("keeps both Lucide icons on the same 1.5px stroke", () => {
    const strokeWidth = (svg: string): string => svg.match(/stroke-width="([^"]+)"/)?.[1] ?? "";
    expect(strokeWidth(hatGlasses)).toBe("1.5");
    expect(strokeWidth(circleX)).toBe(strokeWidth(hatGlasses));
  });

  test("shows circle-x only while an incognito button is hovered", () => {
    expect(iconFor({ incognito: true, hovered: false })).toBe("hat-glasses");
    expect(iconFor({ incognito: true, hovered: true })).toBe("circle-x");
    expect(iconFor({ incognito: false, hovered: true })).toBe("hat-glasses");
  });

  test("routes pointer enter and leave through icon switching without changing click semantics", () => {
    expect(inject).toMatch(/function setButtonHover\(btn: HTMLElement, hovered: boolean\)[\s\S]*setButtonIcon\(btn\);/);
    expect(inject).toContain("setButtonHover(btn, true)");
    expect(inject).toContain("setButtonHover(btn, false)");

    const clickStart = inject.indexOf('btn.addEventListener(\n    "click"');
    const clickEnd = inject.indexOf("  );", clickStart);
    const clickHandler = inject.slice(clickStart, clickEnd);
    expect(clickHandler).toContain("event.preventDefault()");
    expect(clickHandler).toContain("event.stopImmediatePropagation()");
    expect(clickHandler).toContain("void activate()");
  });
});

describe("incodex tooltip lifecycle", () => {
  test("keeps a stable fallback delay without reading private provider state", () => {
    expect(inject).toContain("const TOOLTIP_FALLBACK_DELAY_MS = 700");
  });

  test("listens to the app-wide dismissal signal without dispatching the private event", () => {
    expect(inject).toContain(
      'window.addEventListener(TOOLTIP_DISMISS_EVENT, () => activeTooltipLifecycle?.dismiss())',
    );
    expect(inject).not.toContain("dispatchEvent(new Event(TOOLTIP_DISMISS_EVENT))");
  });

  test("does not override the official fit-content width utility", () => {
    expect(inject).not.toContain("width: max-content");
  });

  test("clears an open tooltip without losing a connected button lifecycle", () => {
    expect(inject).toMatch(
      /function ensureButton\(\): void \{[\s\S]*if \(!search \|\| !placement\) \{[\s\S]*if \(btn\?\.isConnected\) dismissActiveTooltip\(\);[\s\S]*else disposeActiveTooltip\(\);[\s\S]*return;/,
    );
  });

  test("cancels pending and open tooltips on window blur and Escape", () => {
    expect(inject).toContain('window.addEventListener("blur", dismissActiveTooltip)');
    expect(inject).toMatch(
      /function onKeydown\(event: KeyboardEvent\): void \{[\s\S]*event\.key === "Escape"[\s\S]*dismissActiveTooltip\(\);/,
    );
  });

  test("does not open while the official Search tooltip remains visible", () => {
    expect(inject).toMatch(
      /function injectedTooltipCanShow[\s\S]*!\(search && searchTooltipOpen\(search\)\)/,
    );
  });
});
