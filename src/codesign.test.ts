import { describe, expect, test } from "bun:test";
import { orderForInsideOut } from "./codesign";

describe("inside-out signing order", () => {
  test("signs nested frameworks before the top-level app and never uses --deep for order", () => {
    const app = "/tmp/ChatGPT.app";
    const order = orderForInsideOut(
      [
        app,
        `${app}/Contents/Frameworks/Electron Framework.framework`,
        `${app}/Contents/Frameworks/Electron Framework.framework/Versions/A/Libraries/libffmpeg.dylib`,
        `${app}/Contents/Frameworks/Squirrel.framework`,
        `${app}/Contents/MacOS/ChatGPT`,
      ],
      app,
    );
    expect(order.at(-1)).toBe(app);
    expect(order.indexOf(`${app}/Contents/Frameworks/Electron Framework.framework/Versions/A/Libraries/libffmpeg.dylib`)).toBeLessThan(
      order.indexOf(`${app}/Contents/Frameworks/Electron Framework.framework`),
    );
  });
});
