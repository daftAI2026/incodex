import { expect, test } from "bun:test";
import { createPermissionGraphics } from "./incodex-permission-graphics.cts";

function fixture(missing = false) {
  const stored = new Map<string, unknown>();
  const released: string[] = [];
  const bridge = {
    NobjcLibrary: class { NSString = { stringWithUTF8String$: (s: string) => s }; },
    callFunction(name: string, signature: { returns: string }, ...args: unknown[]) {
      if (name.endsWith("Release")) { released.push(name); return; }
      // The shipping objc-js bridge silently returns undefined for ^v results.
      if (signature.returns !== "@" || missing) return undefined;
      return { name, args };
    },
  };
  const layer = { setValue$forKey$: (value: unknown, key: string) => { stored.set(key, value); } };
  return { graphics: createPermissionGraphics(bridge), layer, stored, released };
}

test("native stroke and shadows receive real CF colors rather than unsupported pointer results", () => {
  const f = fixture();
  f.graphics.setBlackColor(f.layer, "borderColor", .15);
  expect(f.stored.get("borderColor")).toEqual({ name: "CGColorCreateGenericRGB", args: [0, 0, 0, .15] });
  expect(f.released).toEqual(["CGColorRelease"]);
});

test("native shadow layers receive a rounded path before the create reference is released", () => {
  const f = fixture();
  const bounds = { origin: { x: 0, y: 0 }, size: { width: 452, height: 44 } };
  f.graphics.setRoundedShadowPath(f.layer, bounds, 12);
  expect(f.stored.get("shadowPath")).toEqual({ name: "CGPathCreateWithRoundedRect", args: [bounds, 12, 12, null] });
  expect(f.released).toEqual(["CGPathRelease"]);
});

test("missing CF values fail explicitly instead of silently removing the visual effect", () => {
  const f = fixture(true);
  expect(() => f.graphics.setBlackColor(f.layer, "shadowColor", 1)).toThrow();
  expect(f.stored.size).toBe(0);
});

test("semantic separator colors retain their RGB and multiplied opacity", () => {
  const f = fixture();
  f.graphics.setColor(f.layer, "borderColor", [1, 1, 1, .3 * .75]);
  expect(f.stored.get("borderColor")).toEqual({ name: "CGColorCreateGenericRGB", args: [1, 1, 1, .3 * .75] });
  expect(f.released).toEqual(["CGColorRelease"]);
});
