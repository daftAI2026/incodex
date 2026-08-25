import { describe, expect, test } from "bun:test";
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
