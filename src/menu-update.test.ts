import { mkdtempSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";
import {
  buildUpdateNotice,
  fetchLatestReleaseTag,
  formatUpdateNotice,
  isNewerVersion,
  readUpdateMessageCache,
  refreshUpdateNotice,
  writeUpdateMessageCache,
} from "./menu-update";

describe("menu update notice", () => {
  test("formats the same sentence Mole writes to its cache", () => {
    expect(formatUpdateNotice("0.2.0")).toBe("Update 0.2.0 available, run incodex update");
  });

  test("only a newer version is an update", () => {
    expect(isNewerVersion("0.2.0", "0.1.0")).toBe(true);
    expect(isNewerVersion("0.1.0", "0.1.0")).toBe(false);
    expect(isNewerVersion("0.1.0", "0.2.0")).toBe(false);
  });

  test("script channel can advertise incodex update; source and homebrew cannot", () => {
    expect(buildUpdateNotice({ channel: "script", current: "0.1.0", latest: "0.2.0" })).toBe(
      "Update 0.2.0 available, run incodex update",
    );
    expect(buildUpdateNotice({ channel: "source", current: "0.1.0", latest: "0.2.0" })).toBeUndefined();
    expect(buildUpdateNotice({ channel: "homebrew", current: "0.1.0", latest: "0.2.0" })).toBeUndefined();
    expect(buildUpdateNotice({ channel: "script", current: "0.2.0", latest: "0.2.0" })).toBeUndefined();
    expect(buildUpdateNotice({ channel: "script", current: "0.1.0", latest: undefined })).toBeUndefined();
  });

  test("stale cache older than the current binary is ignored", () => {
    const dir = mkdtempSync(join(tmpdir(), "incodex-menu-update-"));
    const cachePath = join(dir, "update_message");
    const binaryPath = join(dir, "incodex");
    writeFileSync(binaryPath, "bin\n");
    writeUpdateMessageCache(cachePath, "Update 0.2.0 available, run incodex update");
    utimesSync(cachePath, 1_000_000_000, 1_000_000_000);
    utimesSync(binaryPath, 1_700_000_000, 1_700_000_000);

    expect(readUpdateMessageCache(cachePath, binaryPath)).toBe("");
    expect(readUpdateMessageCache(cachePath, binaryPath)).toBe("");
  });

  test("current cache is shown", () => {
    const dir = mkdtempSync(join(tmpdir(), "incodex-menu-update-"));
    const cachePath = join(dir, "update_message");
    const binaryPath = join(dir, "incodex");
    writeFileSync(binaryPath, "bin\n");
    utimesSync(binaryPath, 1_000_000_000, 1_000_000_000);
    writeUpdateMessageCache(cachePath, "Update 0.2.0 available, run incodex update");
    utimesSync(cachePath, 1_700_000_000, 1_700_000_000);

    expect(readUpdateMessageCache(cachePath, binaryPath)).toBe("Update 0.2.0 available, run incodex update");
  });

  test("fetchLatestReleaseTag reads tag_name and strips the v prefix", async () => {
    const tag = await fetchLatestReleaseTag({
      fetchImpl: async () => new Response(JSON.stringify({ tag_name: "v0.2.0" }), { status: 200 }),
    });
    expect(tag).toBe("0.2.0");
  });

  test("fetchLatestReleaseTag fails open on HTTP errors", async () => {
    expect(await fetchLatestReleaseTag({ fetchImpl: async () => new Response("", { status: 404 }) })).toBeUndefined();
  });

  test("refresh writes a notice for a newer script install", async () => {
    const dir = mkdtempSync(join(tmpdir(), "incodex-menu-update-"));
    const cachePath = join(dir, "update_message");
    const notice = await refreshUpdateNotice({
      cachePath,
      current: "0.1.0",
      channel: "script",
      fetchLatest: async () => "0.2.0",
    });
    expect(notice).toBe("Update 0.2.0 available, run incodex update");
    expect(readUpdateMessageCache(cachePath, join(dir, "missing-binary"))).toBe(
      "Update 0.2.0 available, run incodex update",
    );
  });
});

