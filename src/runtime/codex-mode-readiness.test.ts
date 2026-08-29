import { describe, expect, test } from "bun:test";
import {
  decideCodexModeAction,
  deriveCodexModePageState,
} from "./incodex-codex-mode.cts";

describe("Codex mode readiness", () => {
  test("confirms the primary codex route without invoking its keyboard fallback", () => {
    const page = deriveCodexModePageState({
      modeAvailable: true,
      modeLabel: "Codex",
      officialBlockerVisible: false,
    });

    expect(page).toBe("codex");
    expect(decideCodexModeAction(page, false, 0)).toBe("confirmed");
  });

  test("waits while official onboarding keeps the final mode unavailable", () => {
    const page = deriveCodexModePageState({
      modeAvailable: false,
      modeLabel: "",
      officialBlockerVisible: true,
    });

    expect(page).toBe("pending");
    expect(decideCodexModeAction(page, false, 0)).toBe("wait");
  });

  test("uses Control+3 only after the primary route settles outside Codex", () => {
    const page = deriveCodexModePageState({
      modeAvailable: true,
      modeLabel: "ChatGPT",
      officialBlockerVisible: false,
    });

    expect(page).toBe("other");
    expect(decideCodexModeAction(page, false, 0)).toBe("select-fallback");
  });

  test("never repeats the fallback after bounded confirmation fails", () => {
    expect(decideCodexModeAction("other", true, 0)).toBe("wait");
    expect(decideCodexModeAction("other", true, 2)).toBe("unresolved");
  });
});
