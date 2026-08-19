import { describe, expect, test } from "bun:test";
import { formatStatus } from "./status";

describe("formatStatus", () => {
  test("human status is labeled, not a raw key dump of hashes", () => {
    const text = formatStatus({
      appPath: "/Applications/ChatGPT.app",
      exists: true,
      patched: true,
      loaderOnly: true,
      runtime: "0.1.0 releases/0.1.0",
      main: ".vite/build/early-bootstrap.js",
      installId: "abc",
      targetId: "official-404f3389062b",
      appVersion: "26.814.41407 6720",
    });
    expect(text).toContain("App");
    expect(text).toContain("/Applications/ChatGPT.app");
    expect(text).toContain("Installed");
    expect(text).toContain("yes");
    expect(text).toContain("Runtime");
    expect(text).not.toContain("stored original asar file hash:");
    expect(text).not.toContain("asar file hash:");
  });
});
