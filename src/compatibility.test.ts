import { describe, expect, test } from "bun:test";
import { ADAPTER, probeUi } from "./runtime/compatibility/default-adapter.ts";
import { activeAdapter } from "./runtime/compatibility/registry.ts";
import { isSearchLabel } from "./runtime/compatibility/search-labels.ts";

describe("search labels", () => {
  test("covers English, simplified, and traditional Chinese", () => {
    expect(isSearchLabel("Search")).toBe(true);
    expect(isSearchLabel("搜索")).toBe(true);
    expect(isSearchLabel("搜尋")).toBe(true);
  });

  test("rejects unrelated labels", () => {
    expect(isSearchLabel("Settings")).toBe(false);
    expect(isSearchLabel("")).toBe(false);
  });
});

describe("default UI adapter", () => {
  test("finds Search by label and parks against the header cluster", () => {
    expect(ADAPTER.id).toBe("default");
    expect(ADAPTER).not.toHaveProperty("appVersion");
    expect(ADAPTER).not.toHaveProperty("appBuild");
    expect(ADAPTER.selectors.headerCluster).toContain("ms-auto");
    expect(ADAPTER.stripCloneAttrs).toContain("data-testid");
    const search = { getAttribute: (name: string) => (name === "aria-label" ? "搜尋" : null) };
    const root = {
      querySelectorAll: (sel: string) => (sel === "button" ? [search] : []),
      querySelector: (sel: string) => (sel === ADAPTER.selectors.homeBanners ? {} : null),
    } as unknown as ParentNode;
    expect(probeUi(root)).toEqual({ search: true, banners: true });
  });

  test("is the adapter for every Codex build", () => {
    expect(activeAdapter()).toBe(ADAPTER);
  });
});

