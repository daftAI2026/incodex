import { describe, expect, test } from "bun:test";
import { parseOfficialWindowZoom } from "./tooltip-presentation.ts";

describe("official tooltip presentation", () => {
  test("uses the live Codex window zoom with a safe default", () => {
    expect(parseOfficialWindowZoom("1.2")).toBe(1.2);
    expect(parseOfficialWindowZoom(" 0.8 ")).toBe(0.8);
    expect(parseOfficialWindowZoom("")).toBe(1);
    expect(parseOfficialWindowZoom("0")).toBe(1);
    expect(parseOfficialWindowZoom("not-a-number")).toBe(1);
  });
});
