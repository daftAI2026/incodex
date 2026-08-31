import type { CaptureCandidate, CaptureRegion } from "./model.ts";

export function resolveSelectedCaptureRegions(
  regions: CaptureRegion[],
  candidates: CaptureCandidate[],
): CaptureRegion[] {
  const currentCandidates = new Map(candidates.map((candidate) => [candidate.id, candidate]));
  const resolved: CaptureRegion[] = [];

  for (const region of regions) {
    if (region.source === "manual") {
      resolved.push(region);
      continue;
    }
    const candidate = currentCandidates.get(region.id);
    if (!candidate) continue;
    resolved.push({
      ...region,
      rect: {
        height: candidate.height,
        width: candidate.width,
        x: candidate.x,
        y: candidate.y,
      },
    });
  }

  return resolved;
}
