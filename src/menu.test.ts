import { describe, expect, test } from "bun:test";
import { MENU_ITEMS } from "./menu";

describe("menu", () => {
  test("covers install, uninstall, open, status, doctor, and quit", () => {
    expect(MENU_ITEMS.map((item) => item.id)).toEqual([
      "install",
      "uninstall",
      "open",
      "status",
      "doctor",
      "quit",
    ]);
  });
});
