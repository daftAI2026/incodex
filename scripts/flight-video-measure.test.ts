import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { describe, expect, test } from "bun:test";

const analyzer = resolve(import.meta.dir, "flight-video-measure.swift");

describe("offline flight video analyzer", () => {
  test("self-test checks anchor geometry, confidence bounds, VFR gaps, and anchor-only output", () => {
    const result = spawnSync("swift", [analyzer, "--self-test"], {
      encoding: "utf8",
      timeout: 120_000,
    });

    expect(result.error?.message).toBeUndefined();
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("self-test passed");
    expect(result.stdout).toContain("anchorCentroidXPx");
    expect(result.stdout).toContain("anchorWidthPx");
    expect(result.stdout).not.toContain("cardCentroid");
  }, 120_000);
});
