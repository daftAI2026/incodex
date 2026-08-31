import type { CaptureRect, CaptureSize } from "./model.ts";

export type CodexPrivacyCandidateKind = "conversation" | "identity" | "project";

export type CodexPrivacyCandidate = CaptureRect & {
  kind: CodexPrivacyCandidateKind;
};

const PREVIEW_WIDTH = 1200;
const PREVIEW_HEIGHT = 801;

const CODEX_PREVIEW_CANDIDATES: readonly CodexPrivacyCandidate[] = [
  { height: 28, kind: "project", width: 220, x: 14, y: 145 },
  { height: 28, kind: "project", width: 220, x: 14, y: 177 },
  { height: 28, kind: "conversation", width: 220, x: 14, y: 263 },
  { height: 28, kind: "conversation", width: 220, x: 14, y: 295 },
  { height: 36, kind: "identity", width: 220, x: 14, y: 741 },
] as const;

export function codexPreviewPrivacyRegions(size: CaptureSize): CodexPrivacyCandidate[] {
  const scaleX = size.width / PREVIEW_WIDTH;
  const scaleY = size.height / PREVIEW_HEIGHT;
  return CODEX_PREVIEW_CANDIDATES.map((candidate) => ({
    height: candidate.height * scaleY,
    kind: candidate.kind,
    width: candidate.width * scaleX,
    x: candidate.x * scaleX,
    y: candidate.y * scaleY,
  }));
}
