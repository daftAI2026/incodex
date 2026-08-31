import type { CaptureCandidate, CaptureSize } from "./model.ts";

export type CodexPrivacyCandidateKind = "conversation" | "identity" | "project";

export type CodexPrivacyCandidate = CaptureCandidate & {
  kind: CodexPrivacyCandidateKind;
};

const PREVIEW_WIDTH = 1200;
const PREVIEW_HEIGHT = 801;

const CODEX_PREVIEW_CANDIDATES: readonly CodexPrivacyCandidate[] = [
  { height: 28, id: "project-incodex", kind: "project", width: 220, x: 14, y: 145 },
  { height: 28, id: "project-client-work", kind: "project", width: 220, x: 14, y: 177 },
  { height: 28, id: "conversation-launch", kind: "conversation", width: 220, x: 14, y: 263 },
  { height: 28, id: "conversation-capture", kind: "conversation", width: 220, x: 14, y: 295 },
  { height: 36, id: "identity-account", kind: "identity", width: 220, x: 14, y: 741 },
] as const;

export function codexPreviewPrivacyRegions(size: CaptureSize): CodexPrivacyCandidate[] {
  const scaleX = size.width / PREVIEW_WIDTH;
  const scaleY = size.height / PREVIEW_HEIGHT;
  return CODEX_PREVIEW_CANDIDATES.map((candidate) => ({
    height: candidate.height * scaleY,
    id: candidate.id,
    kind: candidate.kind,
    width: candidate.width * scaleX,
    x: candidate.x * scaleX,
    y: candidate.y * scaleY,
  }));
}
