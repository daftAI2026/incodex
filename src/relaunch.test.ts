import { describe, expect, test } from "bun:test";
import { relaunchDecision } from "./relaunch";

describe("relaunchDecision", () => {
  test("relaunches when ChatGPT was running and install quit it", () => {
    expect(relaunchDecision({ before: [18665] })).toBe("open");
  });

  test("relaunches when ChatGPT is still running after a skip", () => {
    expect(relaunchDecision({ before: [18665] })).toBe("open");
  });

  test("does nothing when ChatGPT was not running", () => {
    expect(relaunchDecision({ before: [] })).toBe("none");
  });
});
