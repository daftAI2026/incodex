import { describe, expect, test } from "bun:test";
import { findSupportedBuild, SUPPORTED_CODEX_BUILDS } from "../src/compatibility/supported-builds";

describe("supported Codex builds", () => {
  test("lists at least the observed official build", () => {
    expect(SUPPORTED_CODEX_BUILDS.length).toBeGreaterThan(0);
    const known = findSupportedBuild({
      bundleIdentifier: "com.openai.codex",
      appVersion: "26.810.52044",
      appBuild: "6662",
    });
    expect(known?.asarMain).toBe(".vite/build/early-bootstrap.js");
  });

  test("unknown builds are not listed", () => {
    expect(
      findSupportedBuild({
        bundleIdentifier: "com.openai.codex",
        appVersion: "0.0.0",
        appBuild: "0",
      }),
    ).toBeUndefined();
  });

  test("known builds carry the compatibility manifest fields", () => {
    const known = findSupportedBuild({
      bundleIdentifier: "com.openai.codex",
      appVersion: "26.810.52044",
      appBuild: "6662",
    });
    expect(known?.adapterId).toBe("build-26.810.52044");
    expect(known?.expectedAsarFiles).toContain("package.json");
    expect(known?.selectors.length).toBeGreaterThan(0);
  });
});
