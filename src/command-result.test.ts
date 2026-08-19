import { describe, expect, test } from "bun:test";
import { formatCommandResult } from "./command-result";
import { formatKv, formatOk } from "./cli-print";

describe("formatCommandResult", () => {
  test("prints install id, runtime version, and path as aligned kv lines", () => {
    const text = formatCommandResult({
      action: "install",
      installId: "abc-123",
      runtimeVersion: "0.1.0",
      app: "/Applications/ChatGPT.app",
    });
    expect(text).toContain(formatKv("Install id", "abc-123", { color: false }));
    expect(text).toContain(formatKv("Runtime", "0.1.0", { color: false }));
    expect(text).toContain(formatKv("App", "/Applications/ChatGPT.app", { color: false }));
    expect(text).not.toContain("install id:");
    expect(text).not.toMatch(/[✓🎉]/u);
  });

  test("omits missing fields and marks a skip with the same marks as other output", () => {
    const text = formatCommandResult({
      action: "runtime",
      skipped: true,
      runtimeVersion: "0.1.0",
      app: undefined,
    });
    expect(text).toContain(formatOk("Already current. Codex was not re-signed.", { color: false }));
    expect(text).toContain(formatKv("Runtime", "0.1.0", { color: false }));
    expect(text).not.toContain("install id:");
    expect(text).not.toContain("Install id");
    expect(text).not.toMatch(/^app:/m);
  });
});
