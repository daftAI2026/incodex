import { describe, expect, test } from "bun:test";
import { liveSupportNote } from "./compatibility/supported-builds";
import { ADAPTER, probeUi } from "./runtime/compatibility/build-26.810.52044";
import { adapterForBuild } from "./runtime/compatibility/registry";
import { isSearchLabel } from "./runtime/compatibility/search-labels";

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

describe("build adapter 26.810.52044", () => {
  test("has a feature probe and known selectors", () => {
    expect(ADAPTER.selectors.headerCluster).toContain("ms-auto");
    expect(ADAPTER.stripCloneAttrs).toContain("data-testid");
    const search = { getAttribute: (name: string) => (name === "aria-label" ? "搜尋" : null) };
    const root = {
      querySelectorAll: (sel: string) => (sel === "button" ? [search] : []),
      querySelector: (sel: string) => (sel === ADAPTER.selectors.homeBanners ? {} : null),
    } as unknown as ParentNode;
    expect(probeUi(root)).toEqual({ search: true, banners: true });
  });

  test("is registered for the observed build and unknown builds have no adapter", () => {
    expect(adapterForBuild("26.810.52044", "6662")?.id).toBe("build-26.810.52044");
    expect(adapterForBuild("0.0.0", "0")).toBeUndefined();
  });
});

describe("live support", () => {
  test("unknown builds warn instead of blocking a confirmed live install", () => {
    const note = liveSupportNote({
      bundleIdentifier: "com.openai.codex",
      appVersion: "99.0.0",
      appBuild: "1",
    });
    expect(note).toMatch(/best-effort/);
    expect(
      liveSupportNote({
        bundleIdentifier: "com.openai.codex",
        appVersion: "26.810.52044",
        appBuild: "6662",
      }),
    ).toBeNull();
  });
});


