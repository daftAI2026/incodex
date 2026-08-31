import { describe, expect, test } from "bun:test";
import {
  captureColorPopoverTemplate,
  captureHexFromHsv,
  captureHsvFromHex,
  isCaptureHexColor,
  sanitizeCaptureHex,
} from "./color-popover.ts";

describe("capture color popover", () => {
  test("matches the reference picker and prefixed hex-input structure", () => {
    const markup = captureColorPopoverTemplate("background", "#2B3440", "Color");

    expect(markup).toContain("data-capture-hide");
    expect(markup).toContain('data-color-popover="background"');
    expect(markup).toContain('data-color-saturation="background"');
    expect(markup).toContain('data-color-hue="background"');
    expect(markup).toContain('data-color-hex="background"');
    expect(markup).toContain('value="#2B3440"');
  });

  test("filters and validates hex input like the reference control", () => {
    expect(sanitizeCaptureHex(" #12g3-45zz6")).toBe("123456");
    expect(isCaptureHexColor("abc")).toBe(true);
    expect(isCaptureHexColor("ABCDEF")).toBe(true);
    expect(isCaptureHexColor("abcd")).toBe(false);
    expect(isCaptureHexColor("12")).toBe(false);
  });

  test("round-trips the selected color through the reference HSV model", () => {
    expect(captureHexFromHsv(captureHsvFromHex("#2B3440"))).toBe("#2b3440");
  });
});
