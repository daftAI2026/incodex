import { describe, expect, test } from "bun:test";
import type { CaptureCandidate, CaptureRegion } from "./model.ts";
import { resolveSelectedCaptureRegions } from "./redactions.ts";

const regions: CaptureRegion[] = [
  {
    id: "r:10:20:80:20",
    rect: { height: 20, width: 80, x: 10, y: 20 },
    source: "automatic",
    style: "mosaic",
  },
  {
    id: "manual-1",
    rect: { height: 40, width: 90, x: 100, y: 120 },
    source: "manual",
    style: "solid",
  },
];

describe("capture redaction resolution", () => {
  test("resolves picked automatic ids against the current capture candidates", () => {
    const candidates: CaptureCandidate[] = [
      { height: 24, id: "r:10:20:80:20", width: 96, x: 12, y: 22 },
    ];

    expect(resolveSelectedCaptureRegions(regions, candidates)).toEqual([
      {
        id: "r:10:20:80:20",
        rect: { height: 24, width: 96, x: 12, y: 22 },
        source: "automatic",
        style: "mosaic",
      },
      regions[1],
    ]);
  });

  test("keeps the picked id but omits it while the current capture has no match", () => {
    expect(resolveSelectedCaptureRegions(regions, [])).toEqual([regions[1]]);
    expect(regions[0]?.id).toBe("r:10:20:80:20");
  });
});
