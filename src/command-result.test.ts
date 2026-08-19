import { describe, expect, test } from "bun:test";
import { formatCommandResult } from "./command-result";

describe("formatCommandResult", () => {
  test("prints install id, runtime version, and path after a destructive command", () => {
    const text = formatCommandResult({
      action: "install",
      installId: "abc-123",
      runtimeVersion: "0.1.0",
      app: "/Applications/ChatGPT.app",
    });
    expect(text).toContain("install id: abc-123");
    expect(text).toContain("runtime version: 0.1.0");
    expect(text).toContain("app: /Applications/ChatGPT.app");
    expect(text).not.toMatch(/[✓🎉]/u);
  });

  test("omits missing fields and marks a skip", () => {
    const text = formatCommandResult({
      action: "runtime",
      skipped: true,
      runtimeVersion: "0.1.0",
      app: undefined,
    });
    expect(text).toContain("already current");
    expect(text).toContain("runtime version: 0.1.0");
    expect(text).not.toContain("install id:");
    expect(text).not.toContain("app:");
  });
});
