import { describe, expect, test } from "bun:test";
import { relaunchDecision } from "./relaunch";

describe("relaunchDecision", () => {
  test("opens ChatGPT when install already quit it", () => {
    expect(relaunchDecision({ before: [18665], after: [] })).toBe("open");
  });

  test("does not relaunch when install was already current", () => {
    expect(relaunchDecision({ before: [52038], after: [52038], skipped: true })).toBe("none");
  });

  test("does nothing when ChatGPT was not running", () => {
    expect(relaunchDecision({ before: [], after: [] })).toBe("none");
  });
});
