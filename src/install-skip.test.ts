import { describe, expect, test } from "bun:test";
import { officialInstallAlreadyCurrent } from "./install";

describe("officialInstallAlreadyCurrent", () => {
  test("skips only when patched, loader-only, and runtime already matches", () => {
    expect(officialInstallAlreadyCurrent({ patched: true, loaderOnly: true, runtimeCurrent: true })).toBe(true);
    expect(officialInstallAlreadyCurrent({ patched: false, loaderOnly: true, runtimeCurrent: true })).toBe(false);
    expect(officialInstallAlreadyCurrent({ patched: true, loaderOnly: false, runtimeCurrent: true })).toBe(false);
    expect(officialInstallAlreadyCurrent({ patched: true, loaderOnly: true, runtimeCurrent: false })).toBe(false);
  });
});
