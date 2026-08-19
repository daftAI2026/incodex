import { describe, expect, test } from "bun:test";
import { formatKv, formatOk, formatSection, formatStep, formatWarn } from "./cli-print";

describe("cli print", () => {
  test("kv lines align labels", () => {
    const text = [formatKv("App", "/Applications/ChatGPT.app"), formatKv("Installed", "yes")].join("\n");
    expect(text).toContain("App");
    expect(text).toContain("/Applications/ChatGPT.app");
    expect(text).toContain("Installed");
    expect(text).not.toMatch(/^app:/m);
  });

  test("ok and warn marks are stable", () => {
    expect(formatOk("Closed. Isolated session removed.", { color: false })).toContain("Closed. Isolated session removed.");
    expect(formatWarn("unknown Codex build", { color: false })).toContain("unknown Codex build");
    expect(formatSection("Signing", { color: false })).toBe("➤ Signing");
  });

  test("headers sit at column 0; body rows indent two spaces", () => {
    expect(formatKv("App", "/Applications/ChatGPT.app", { color: false }).startsWith("  ")).toBe(true);
    expect(formatOk("done", { color: false }).startsWith("  ✓ ")).toBe(true);
    expect(formatWarn("careful", { color: false }).startsWith("  ! ")).toBe(true);
    expect(formatStep("Install", { color: false }).startsWith("➤ ")).toBe(true);
    expect(formatSection("Runtime", { color: false }).startsWith("➤ ")).toBe(true);
  });

  test("tty color uses a purple arrow header, green check, yellow warn", () => {
    expect(formatSection("Signing", { color: true })).toContain("\x1b[1;35m");
    expect(formatStep("Install", { color: true })).toContain("\x1b[1;35m");
    expect(formatOk("done", { color: true })).toContain("\x1b[0;32m");
    expect(formatWarn("careful", { color: true })).toContain("\x1b[0;33m");
  });
});

