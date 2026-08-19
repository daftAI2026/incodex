import { describe, expect, test } from "bun:test";
import { parseCli } from "./parse-cli";

describe("parseCli", () => {
  test("no args open the menu, not an install", () => {
    expect(parseCli(["bun", "cli"])).toMatchObject({ command: "menu", help: false });
  });

  test("bare install targets the official app", () => {
    expect(parseCli(["bun", "cli", "install"])).toMatchObject({
      command: "install",
      live: true,
      clone: false,
      yes: false,
      dryRun: false,
    });
  });

  test("bare uninstall targets the official app", () => {
    expect(parseCli(["bun", "cli", "uninstall"])).toMatchObject({
      command: "uninstall",
      live: true,
      clone: false,
    });
  });

  test("rejects --clone and --live together", () => {
    expect(() => parseCli(["bun", "cli", "install", "--clone", "--live"])).toThrow(/cannot be used together/);
  });

  test("rejects --app eating the next flag", () => {
    expect(() => parseCli(["bun", "cli", "status", "--app", "--live"])).toThrow(/requires a path/);
  });

  test("--yes and hidden --confirm-live both set yes", () => {
    expect(parseCli(["bun", "cli", "install", "--yes"]).yes).toBe(true);
    expect(parseCli(["bun", "cli", "install", "--live", "--confirm-live"])).toMatchObject({
      live: true,
      yes: true,
    });
    expect(parseCli(["bun", "cli", "install", "--live"]).yes).toBe(false);
  });

  test("recover requires a transaction id", () => {
    expect(() => parseCli(["bun", "cli", "recover"])).toThrow(/transaction/);
    expect(parseCli(["bun", "cli", "recover", "--transaction", "abc"]).transaction).toBe("abc");
  });

  test("accepts clone, custom app, json, dry-run, and doctor", () => {
    expect(parseCli(["bun", "cli", "install", "--clone"])).toMatchObject({
      command: "install",
      clone: true,
      live: false,
    });
    expect(parseCli(["bun", "cli", "uninstall", "--app", "/tmp/ChatGPT.app"])).toMatchObject({
      command: "uninstall",
      app: "/tmp/ChatGPT.app",
      live: false,
    });
    expect(parseCli(["bun", "cli", "status", "--json"])).toMatchObject({ command: "status", json: true });
    expect(parseCli(["bun", "cli", "doctor"])).toMatchObject({ command: "doctor" });
    expect(parseCli(["bun", "cli", "install", "--dry-run"]).dryRun).toBe(true);
  });

  test("unknown commands are rejected", () => {
    expect(() => parseCli(["bun", "cli", "wipe"])).toThrow(/unknown command/);
  });

  test("unknown flags are rejected instead of becoming a live install", () => {
    expect(() => parseCli(["bun", "cli", "install", "--dry-run\uFF0C"])).toThrow(/unknown flag/);
    expect(() => parseCli(["bun", "cli", "install", "--dry-run,"])).toThrow(/unknown flag/);
    expect(() => parseCli(["bun", "cli", "install", "--please"])).toThrow(/unknown flag: --please/);
  });

  test("unexpected positional arguments are rejected", () => {
    expect(() => parseCli(["bun", "cli", "install", "now"])).toThrow(/unexpected argument: now/);
  });

  test("runtime updates external files without an install target", () => {
    expect(parseCli(["bun", "cli", "runtime"])).toMatchObject({ command: "runtime", live: false, clone: false });
  });

  test("help and version flags", () => {
    expect(parseCli(["bun", "cli", "--help"]).command).toBe("help");
    expect(parseCli(["bun", "cli", "-V"]).command).toBe("version");
    expect(parseCli(["bun", "cli", "install", "--help"])).toMatchObject({ command: "install", help: true });
  });

  test("open is a recognized command", () => {
    expect(parseCli(["bun", "cli", "open"]).command).toBe("open");
  });

  test("update and self-uninstall are recognized", () => {
    expect(parseCli(["bun", "cli", "update"])).toMatchObject({ command: "update", dryRun: false });
    expect(parseCli(["bun", "cli", "self-uninstall"])).toMatchObject({
      command: "self-uninstall",
      restoreApp: false,
    });
    expect(parseCli(["bun", "cli", "self-uninstall", "--restore-app"]).restoreApp).toBe(true);
    expect(parseCli(["bun", "cli", "update", "--dry-run"]).dryRun).toBe(true);
  });
});

