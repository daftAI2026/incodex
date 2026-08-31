import type { CaptureCandidate } from "./model.ts";

export const CAPTURE_ACTIVE_CLASS = "incodex-capturing";
export const CAPTURE_PRIVACY_CLASS = "incodex-capture-redact";

type CaptureClassList = {
  add: (value: string) => void;
  remove: (value: string) => void;
};

type CaptureRoot = {
  classList: CaptureClassList;
};

export type PreparedCapture<T> = {
  candidates: CaptureCandidate[];
  source: T;
};

export type CapturePreparedWindowOptions<T> = {
  begin?: () => Promise<void> | void;
  capture: () => Promise<T> | T;
  collectCandidates: () => CaptureCandidate[];
  privacyEnabled: boolean;
  root: CaptureRoot;
  waitForFrame: () => Promise<void>;
};

export async function capturePreparedWindow<T>(
  options: CapturePreparedWindowOptions<T>,
): Promise<PreparedCapture<T>> {
  await options.begin?.();
  options.root.classList.add(CAPTURE_ACTIVE_CLASS);
  if (options.privacyEnabled) {
    options.root.classList.add(CAPTURE_PRIVACY_CLASS);
  }

  try {
    await options.waitForFrame();
    await options.waitForFrame();
    const candidates = options.collectCandidates();
    const source = await options.capture();
    return { candidates, source };
  } finally {
    options.root.classList.remove(CAPTURE_ACTIVE_CLASS);
    options.root.classList.remove(CAPTURE_PRIVACY_CLASS);
  }
}

export function waitForCaptureFrame(): Promise<void> {
  return new Promise((resolve) => {
    window.requestAnimationFrame(() => resolve());
  });
}
