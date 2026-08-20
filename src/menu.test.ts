import { describe, expect, test } from "bun:test";
import {
  erasePaintedLines,
  CLEAR_LINE,
  ERASE_DOWN,
  handleMenuKey,
  HIDE_CURSOR,
  MENU_HOME,
  MENU_ITEMS,
  menuControlsLine,
  renderMenu,
  SHOW_CURSOR,
} from "./menu";

describe("menu", () => {
  test("covers open, install, uninstall, status, doctor, and quit", () => {
    expect(MENU_ITEMS.map((item) => item.id)).toEqual([
      "open",
      "install",
      "uninstall",
      "status",
      "doctor",
      "quit",
    ]);
  });

  test("each item has a short title and a description", () => {
    for (const item of MENU_ITEMS) {
      expect(item.title.length).toBeGreaterThan(0);
      expect(item.description.length).toBeGreaterThan(0);
      expect(item.title.includes(" ")).toBe(false);
    }
  });

  test("render shows the supplied INCODEX wordmark, repo URL, tagline, items, and key footer", () => {
    const text = renderMenu(0, { color: false });

    expect(text).toContain("  _____   _   _    _____    ____    _____    ______  __   __");
    expect(text).toContain(" |_   _| | \\ | |  / ____|  / __ \\  |  __ \\  |  ____| \\ \\ / /");
    expect(text).toContain(" |_____| |_| \\_|  \\_____|  \\____/  |_____/  |______| /_/ \\_\\");
    expect(text).not.toContain("|_ _|| \\ | | / ___|");
    expect(text).not.toMatch(/^incodex\n/);
    expect(text).toContain("https://github.com/daftAI2026/incodex");
    expect(text).toContain("Incognito toggle for Codex desktop");
    expect(text).toContain("1. Open");
    expect(text).toContain("Open an incognito window without patching");
    expect(text).toContain("2. Install");
    expect(text).toContain("Patch the Codex app you are using");
    expect(text).toContain("3. Uninstall");
    expect(text).toContain("Restore the official Codex app");
    expect(text).toContain("4. Status");
    expect(text).toContain("5. Doctor");
    expect(text).toContain("6. Quit");
    expect(text).toContain("↑↓ | Enter | V Version | Q Quit | 1-6 Jump");
    expect(handleMenuKey("5", 0)).toEqual({ action: "select", id: "doctor" });
  });

  test("selected row uses Mole's arrow", () => {
    const text = renderMenu(0, { color: false });
    expect(text).toContain("➤ 1. Open");
    expect(text).not.toContain("➤ 2. Install");
  });

  test("optional update line appears and unlocks the U footer shortcut", () => {
    const text = renderMenu(0, {
      color: false,
      updateMessage: "Update 0.2.0 available, run incodex update",
    });
    expect(text).toContain("Update 0.2.0 available, run incodex update");
    expect(text).toContain("↑↓ | Enter | U Update | V Version | Q Quit | 1-6 Jump");
    expect(menuControlsLine(false)).toBe("↑↓ | Enter | V Version | Q Quit | 1-6 Jump");
    expect(menuControlsLine(true)).toBe("↑↓ | Enter | U Update | V Version | Q Quit | 1-6 Jump");
  });

  test("render without color has no ANSI escapes", () => {
    const text = renderMenu(0, { color: false });
    expect(text.includes("\x1b[")).toBe(false);
  });

  test("repo URL and tagline left-align with the I of the wordmark", () => {
    const text = renderMenu(0, { color: false });
    const lines = text.split("\n");
    const indent = lines[0]?.search(/\S/) ?? -1;
    expect(indent).toBeGreaterThan(0);

    const urlLine = lines.find((line) => line.includes("https://github.com/daftAI2026/incodex"));
    const tagLine = lines.find((line) => line.includes("Incognito toggle for Codex desktop"));
    expect(urlLine?.search(/\S/)).toBe(indent);
    expect(tagLine?.search(/\S/)).toBe(indent);
    expect(urlLine?.startsWith(" ".repeat(indent + 2))).toBe(false);
  });

  test("color mode uses Mole's green banner, blue URL, cyan selection, gray footer", () => {
    const text = renderMenu(0, { color: true, updateMessage: "Update 0.2.0 available, run incodex update" });
    const green = "\x1b[0;32m";
    const blue = "\x1b[1;34m";
    const cyan = "\x1b[0;36m";
    const gray = "\x1b[0;38;5;244m";
    const reset = "\x1b[0m";

    expect(text).toContain(`${green}  _____   _   _    _____    ____    _____    ______  __   __${reset}`);
    expect(text).toContain(`${blue}  https://github.com/daftAI2026/incodex${reset}`);
    expect(text).toContain(`${green}  Incognito toggle for Codex desktop.${reset}`);
    expect(text).toContain(`${green}  Update 0.2.0 available, run incodex update${reset}`);
    expect(text).toContain(`${cyan}➤ 1. Open`);
    expect(text).not.toContain(`${cyan}➤ 2. Install`);
    expect(text).toContain(`${gray}↑↓ | Enter | U Update | V Version | Q Quit | 1-6 Jump${reset}`);
  });
});


describe("handleMenuKey", () => {
  test("arrow keys and vim j/k move the selection", () => {
    expect(handleMenuKey("\u001b[A", 0)).toEqual({ action: "move", selected: 5 });
    expect(handleMenuKey("k", 2)).toEqual({ action: "move", selected: 1 });
    expect(handleMenuKey("K", 2)).toEqual({ action: "move", selected: 1 });
    expect(handleMenuKey("\u001b[B", 5)).toEqual({ action: "move", selected: 0 });
    expect(handleMenuKey("j", 0)).toEqual({ action: "move", selected: 1 });
    expect(handleMenuKey("J", 0)).toEqual({ action: "move", selected: 1 });
  });

  test("digits jump to that item and enter keeps the highlight", () => {
    expect(handleMenuKey("1", 3)).toEqual({ action: "select", id: "open" });
    expect(handleMenuKey("2", 0)).toEqual({ action: "select", id: "install" });
    expect(handleMenuKey("3", 0)).toEqual({ action: "select", id: "uninstall" });
    expect(handleMenuKey("6", 0)).toEqual({ action: "select", id: "quit" });
    expect(handleMenuKey("7", 0)).toEqual({ action: "ignore" });
    expect(handleMenuKey("\r", 0)).toEqual({ action: "select", id: "open" });
    expect(handleMenuKey("\n", 4)).toEqual({ action: "select", id: "doctor" });
  });

  test("leaving the menu erases it and restores the cursor", () => {
    expect(HIDE_CURSOR).toBe("\u001b[?25l");
    expect(SHOW_CURSOR).toBe("\u001b[?25h");
    expect(erasePaintedLines(12)).toBe("\u001b[12A\u001b[J");
    expect(erasePaintedLines(0)).toBe("");
  });

  test("a menu frame starts at the top of the screen and clears everything below", () => {
    expect(MENU_HOME).toBe("\u001b[H");
    expect(ERASE_DOWN).toBe("\u001b[J");
    expect(CLEAR_LINE).toBe("\r\u001b[2K");
  });

  test("q and escape quit; U only runs update when a notice is showing", () => {
    expect(handleMenuKey("q", 0)).toEqual({ action: "select", id: "quit" });
    expect(handleMenuKey("Q", 0)).toEqual({ action: "select", id: "quit" });
    expect(handleMenuKey("\u001b", 0)).toEqual({ action: "select", id: "quit" });
    expect(handleMenuKey("u", 0)).toEqual({ action: "ignore" });
    expect(handleMenuKey("U", 0, { updateAvailable: true })).toEqual({ action: "select", id: "update" });
    expect(handleMenuKey("v", 0)).toEqual({ action: "select", id: "version" });
    expect(handleMenuKey("V", 0)).toEqual({ action: "select", id: "version" });
  });
});
