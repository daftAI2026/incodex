import { describe, expect, test } from "bun:test";
import { classifyWindow } from "./runtime/incodex-window-kind.cts";

describe("window classification", () => {
  test("overlay pet is auxiliary: always on top and not focusable", () => {
    expect(
      classifyWindow({ alwaysOnTop: true, focusable: false, width: 120, height: 120, url: "" }),
    ).toBe("auxiliary");
  });

  test("small login or oauth popup is a main/dialog window, not auxiliary", () => {
    expect(
      classifyWindow({
        alwaysOnTop: false,
        focusable: true,
        width: 360,
        height: 280,
        url: "https://accounts.google.com/o/oauth2/auth",
      }),
    ).toBe("main");
  });

  test("a child dialog of the app window stays main", () => {
    expect(
      classifyWindow({
        alwaysOnTop: false,
        focusable: true,
        width: 320,
        height: 240,
        url: "file:///settings",
        hasParent: true,
      }),
    ).toBe("main");
  });

  test("size alone does not hide a focusable window", () => {
    expect(
      classifyWindow({ alwaysOnTop: false, focusable: true, width: 300, height: 200, url: "" }),
    ).toBe("main");
  });

  test("tiny always-on-top window with no app URL is auxiliary", () => {
    expect(
      classifyWindow({ alwaysOnTop: true, focusable: true, width: 180, height: 80, url: "" }),
    ).toBe("auxiliary");
  });
});
