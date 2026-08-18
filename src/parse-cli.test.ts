import { describe, expect, test } from "bun:test";
import { parseCli } from "./parse-cli";

describe("parseCli", () => {
  test("bare uninstall cannot touch the official app", () => {
    expect(() => parseCli(["bun", "cli", "uninstall"])).toThrow(/explicit/);
  });

  test("rejects --clone and --live together", () => {
    expect(() => parseCli(["bun", "cli", "install", "--clone", "--live"])).toThrow(/cannot be used together/);
  });

  test("rejects --app eating the next flag", () => {
    expect(() => parseCli(["bun", "cli", "status", "--app", "--live"])).toThrow(/requires a path/);
  });

  test("live install requires --confirm-live", () => {
    expect(() => parseCli(["bun", "cli", "install", "--live"])).toThrow(/confirm-live/);
    expect(parseCli(["bun", "cli", "install", "--live", "--confirm-live"]).live).toBe(true);
  });

  test("recover requires a transaction id", () => {
    expect(() => parseCli(["bun", "cli", "recover"])).toThrow(/transaction/);
    expect(parseCli(["bun", "cli", "recover", "--transaction", "abc"]).transaction).toBe("abc");
  });
});
