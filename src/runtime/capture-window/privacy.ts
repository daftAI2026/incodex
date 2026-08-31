import type { CaptureCandidate, CaptureSize } from "./model.ts";

export const CAPTURE_CANDIDATE_SELECTOR =
  'p, li, pre, blockquote, h1, h2, h3, h4, h5, h6, td, img, textarea, [contenteditable="true"]';

const CAPTURE_HIDE_SELECTOR = "[data-incodex-capture-hide]";
const MAX_CAPTURE_CANDIDATES = 150;
const MIN_CANDIDATE_WIDTH = 24;
const MIN_CANDIDATE_HEIGHT = 12;

type CandidateElement = HTMLElement & {
  checkVisibility?: () => boolean;
  value?: string;
};

type AcceptedCandidate = {
  candidate: CaptureCandidate;
  element: CandidateElement;
};

export function collectCaptureCandidates(
  documentRoot: Document,
  viewport: CaptureSize,
): CaptureCandidate[] {
  const accepted: AcceptedCandidate[] = [];
  const elements = documentRoot.querySelectorAll<CandidateElement>(CAPTURE_CANDIDATE_SELECTOR);

  for (const element of elements) {
    if (accepted.length >= MAX_CAPTURE_CANDIDATES) break;
    const bounds = element.getBoundingClientRect();
    if (bounds.width < MIN_CANDIDATE_WIDTH || bounds.height < MIN_CANDIDATE_HEIGHT) continue;
    if (
      bounds.bottom <= 0 ||
      bounds.right <= 0 ||
      bounds.top >= viewport.height ||
      bounds.left >= viewport.width
    ) {
      continue;
    }
    if (element.closest(CAPTURE_HIDE_SELECTOR)) continue;
    if (!hasCandidateContent(element)) continue;
    if (!isCandidateVisible(element)) continue;
    if (accepted.some((entry) => entry.element.contains(element))) continue;

    const rect = clipToViewport(bounds, viewport);
    if (!rect || rect.width < MIN_CANDIDATE_WIDTH || rect.height < MIN_CANDIDATE_HEIGHT) {
      continue;
    }

    const candidate = {
      ...rect,
      id: `r:${Math.round(rect.x)}:${Math.round(rect.y)}:${Math.round(rect.width)}:${Math.round(rect.height)}`,
    };
    accepted.push({ candidate, element });
  }

  return accepted.map((entry) => entry.candidate);
}

function isCandidateVisible(element: CandidateElement): boolean {
  return element.checkVisibility?.() ?? true;
}

function hasCandidateContent(element: CandidateElement): boolean {
  const tagName = element.tagName.toUpperCase();
  if (tagName === "IMG") return true;
  if (tagName === "TEXTAREA") return (element.value ?? "").trim().length > 0;
  return element.textContent.trim().length >= 3;
}

function clipToViewport(rect: DOMRect, viewport: CaptureSize): Omit<CaptureCandidate, "id"> | null {
  const x = Math.max(0, rect.left);
  const y = Math.max(0, rect.top);
  const right = Math.min(viewport.width, rect.right);
  const bottom = Math.min(viewport.height, rect.bottom);
  if (right <= x || bottom <= y) return null;
  return {
    height: bottom - y,
    width: right - x,
    x,
    y,
  };
}
