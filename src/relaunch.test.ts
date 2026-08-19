import { describe, expect, test } from "bun:test";
import { relaunchDecision } from "./relaunch";

describe("relaunchDecision", () => {
  test("opens ChatGPT when install already quit it", () => {
    expect(relaunchDecision({ before: [18665], after: [] })).toBe("open");
  });

  test("asks when ChatGPT is still running after a skip", () => {
    expect(relaunchDecision({ before: [18665], after: [18665] })).toBe("ask");
  });

  test("does nothing when ChatGPT was not running", () => {
    expect(relaunchDecision({ before: [], after: [] })).toBe("none");
  });
});
