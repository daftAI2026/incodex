import { describe, expect, test } from "bun:test";
import { codexPreviewPrivacyRegions } from "./privacy.ts";

describe("Codex capture privacy candidates", () => {
  test("automatically masks only private sidebar rows in the closed-menu preview", () => {
    const regions = codexPreviewPrivacyRegions({ height: 801, width: 1200 });

    expect(regions).toHaveLength(5);
    expect(regions.every((region) => region.x >= 0 && region.x + region.width <= 248)).toBe(true);
    expect(regions.map((region) => region.kind)).toEqual([
      "project",
      "project",
      "conversation",
      "conversation",
      "identity",
    ]);
  });

  test("scales the stable preview layout without leaking candidates into message content", () => {
    const regions = codexPreviewPrivacyRegions({ height: 1602, width: 2400 });

    expect(regions[0]).toMatchObject({ x: 28, y: 290, width: 440, height: 56 });
    expect(regions.at(-1)).toMatchObject({ x: 28, y: 1482, width: 440, height: 72 });
  });
});
