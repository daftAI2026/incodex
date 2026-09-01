import type { CaptureCandidate, CaptureSize } from "./model.ts";

export const CAPTURE_CANDIDATE_SELECTOR =
  'p, li, pre, blockquote, h1, h2, h3, h4, h5, h6, td, img, textarea, [contenteditable="true"]';

const CAPTURE_HIDE_SELECTOR = "[data-incodex-capture-hide]";
const CAPTURE_REDACT_ATTRIBUTE = "data-incodex-capture-redact";
const CAPTURE_PROFILE_ATTRIBUTE = "data-incodex-capture-redact-profile";
const CODEX_THREAD_SELECTOR =
  "[data-app-action-sidebar-thread-row][data-app-action-sidebar-thread-title]";
const CODEX_PROJECT_SELECTOR =
  "[data-app-action-sidebar-project-row][data-app-action-sidebar-project-label]";
const CODEX_PROFILE_SELECTOR = 'button.sidebar-item[aria-haspopup="menu"]';
const CODEX_COMPOSER_PROJECT_SELECTOR = '[data-composer-navigation-target="workspace-project"]';
const CODEX_EMPTY_STATE_PROJECT_SELECTOR =
  '[data-feature="game-source"] [data-slot="popover-trigger"]';
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

type AttributeSnapshot = {
  element: Element;
  name: string;
  value: string | null;
};

export function markCodexPrivacyPlaceholders(documentRoot: Document): () => void {
  const snapshots: AttributeSnapshot[] = [];
  markRows(
    documentRoot.querySelectorAll<HTMLElement>(CODEX_THREAD_SELECTOR),
    "data-app-action-sidebar-thread-title",
    snapshots,
  );
  markRows(
    documentRoot.querySelectorAll<HTMLElement>(CODEX_PROJECT_SELECTOR),
    "data-app-action-sidebar-project-label",
    snapshots,
  );
  markProfile(documentRoot, snapshots);
  markProjectTextLeaves(
    documentRoot.querySelectorAll<HTMLElement>(CODEX_COMPOSER_PROJECT_SELECTOR),
    snapshots,
  );
  markProjectTextLeaves(
    documentRoot.querySelectorAll<HTMLElement>(CODEX_EMPTY_STATE_PROJECT_SELECTOR),
    snapshots,
  );

  return () => restoreAttributes(snapshots);
}

function markProjectTextLeaves(
  elements: NodeListOf<HTMLElement>,
  snapshots: AttributeSnapshot[],
): void {
  for (const element of elements) {
    if (element.children.length === 0 && element.textContent.trim().length > 0) {
      setTemporaryAttribute(element, CAPTURE_REDACT_ATTRIBUTE, "project", snapshots);
      continue;
    }
    for (const leaf of element.querySelectorAll("*")) {
      if (leaf.children.length > 0 || leaf.textContent.trim().length === 0) continue;
      setTemporaryAttribute(leaf, CAPTURE_REDACT_ATTRIBUTE, "project", snapshots);
    }
  }
}

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

function markRows(
  rows: NodeListOf<HTMLElement>,
  labelAttribute: string,
  snapshots: AttributeSnapshot[],
): void {
  for (const row of rows) {
    const label = row.getAttribute(labelAttribute)?.trim();
    if (!label) continue;
    const textElement = findExactTextLeaf(row, label);
    if (textElement) setTemporaryAttribute(textElement, CAPTURE_REDACT_ATTRIBUTE, "text", snapshots);
  }
}

function markProfile(documentRoot: Document, snapshots: AttributeSnapshot[]): void {
  const buttons = documentRoot.querySelectorAll<HTMLButtonElement>(CODEX_PROFILE_SELECTOR);
  for (const button of buttons) {
    const directChildren = Array.from(button.children);
    const avatar = directChildren.find((element) => element.tagName === "IMG");
    const name = directChildren.find(
      (element) => element.tagName === "SPAN" && element.textContent.trim().length > 0,
    );
    if (!avatar || !name) continue;
    setTemporaryAttribute(button, CAPTURE_PROFILE_ATTRIBUTE, "", snapshots);
    setTemporaryAttribute(name, CAPTURE_REDACT_ATTRIBUTE, "text", snapshots);
  }
}

function findExactTextLeaf(root: Element, text: string): Element | null {
  for (const element of root.querySelectorAll("*")) {
    if (element.children.length === 0 && element.textContent.trim() === text) return element;
  }
  return null;
}

function setTemporaryAttribute(
  element: Element,
  name: string,
  value: string,
  snapshots: AttributeSnapshot[],
): void {
  snapshots.push({ element, name, value: element.getAttribute(name) });
  element.setAttribute(name, value);
}

function restoreAttributes(snapshots: AttributeSnapshot[]): void {
  for (const snapshot of snapshots.reverse()) {
    if (snapshot.value === null) snapshot.element.removeAttribute(snapshot.name);
    else snapshot.element.setAttribute(snapshot.name, snapshot.value);
  }
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
