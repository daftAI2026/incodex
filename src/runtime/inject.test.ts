import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { iconFor } from "./incognito-icon";

const inject = readFileSync(join(import.meta.dir, "inject.ts"), "utf8");

describe("hat-glasses stays after header remount", () => {
  test("does not disconnect the observer once the button exists", () => {
    expect(inject).not.toContain("observer.disconnect");
    expect(inject).not.toMatch(/if \(uiReady\(\)\) return;/);
  });

  test("parks the button as the previous sibling of Search", () => {
    expect(inject).toContain("insertBefore(btn, search)");
    expect(inject).not.toContain("cluster.insertBefore(btn, cluster.firstElementChild)");
  });

  test("watches documentElement so a replaced sidebar cluster is still seen", () => {
    expect(inject).toContain("document.documentElement");
    expect(inject).not.toContain("observer.observe(observeRoot()");
  });

  test("skips ensureButton while the hat is still the previous sibling of Search", () => {
    expect(inject).toContain("buttonStillBesideSearch");
    expect(inject).toContain("if (!needsInject()) return");
  });
});

describe("incognito button exit affordance", () => {
  test("shows circle-x only while an incognito button is hovered", () => {
    expect(iconFor({ incognito: true, hovered: false })).toBe("hat-glasses");
    expect(iconFor({ incognito: true, hovered: true })).toBe("circle-x");
    expect(iconFor({ incognito: false, hovered: true })).toBe("hat-glasses");
  });
});
