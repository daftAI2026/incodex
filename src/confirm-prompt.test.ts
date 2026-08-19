import { describe, expect, test } from "bun:test";
import { CONFIRM_PROMPT, interpretConfirmKey } from "./confirm-prompt";

describe("interpretConfirmKey", () => {
  test("Enter confirms without typing y", () => {
    expect(interpretConfirmKey("\r")).toBe("yes");
    expect(interpretConfirmKey("\n")).toBe("yes");
    expect(interpretConfirmKey("")).toBe("yes");
  });

  test("ESC and any other key cancel", () => {
    expect(interpretConfirmKey("\u001b")).toBe("no");
    expect(interpretConfirmKey("y")).toBe("no");
    expect(interpretConfirmKey("n")).toBe("no");
    expect(interpretConfirmKey(" ")).toBe("no");
  });

  test("Ctrl+C is an interrupt", () => {
    expect(interpretConfirmKey("\u0003")).toBe("interrupt");
  });

  test("prompt asks for Enter and ESC, not y/N", () => {
    expect(CONFIRM_PROMPT).toContain("Enter");
    expect(CONFIRM_PROMPT).toContain("ESC");
    expect(CONFIRM_PROMPT).not.toContain("y/N");
  });
});
