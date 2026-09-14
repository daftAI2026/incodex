/**
 * [INPUT]: 截图适配器、候选收集器与帧等待能力
 * [OUTPUT]: 串行截图、偏好先读与临时隐藏状态恢复流程
 * [POS]: Shot 截图时序边界，在成功/失败/超时后撤销临时样式
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
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
  timeoutMs?: number;
  waitForFrame: () => Promise<void>;
};

const CAPTURE_TIMEOUT_MS = 10_000;
let captureInFlight = false;

export type PrepareCaptureWindowOptions<T, P extends { privacyEnabled: boolean }> = {
  capture: (privacyEnabled: boolean) => Promise<T>;
  loadPreferences: () => P;
  onCaptureError?: (error: unknown) => void;
};

export async function prepareCaptureWindow<T, P extends { privacyEnabled: boolean }>(
  options: PrepareCaptureWindowOptions<T, P>,
): Promise<{ preferences: P; snapshot: T } | null> {
  const preferences = options.loadPreferences();
  try {
    const snapshot = await options.capture(preferences.privacyEnabled);
    return { preferences, snapshot };
  } catch (error) {
    options.onCaptureError?.(error);
    return null;
  }
}

export async function capturePreparedWindow<T>(
  options: CapturePreparedWindowOptions<T>,
): Promise<PreparedCapture<T>> {
  if (captureInFlight) throw new Error("A window capture is already in progress");
  captureInFlight = true;
  try {
    try {
      await options.begin?.();
    } catch {
    }
    options.root.classList.add(CAPTURE_ACTIVE_CLASS);
    if (options.privacyEnabled) {
      options.root.classList.add(CAPTURE_PRIVACY_CLASS);
    }
    await options.waitForFrame();
    await options.waitForFrame();
    let candidates: CaptureCandidate[] = [];
    try {
      candidates = options.collectCandidates();
    } catch {
    }
    const source = await withCaptureTimeout(
      Promise.resolve(options.capture()),
      options.timeoutMs ?? CAPTURE_TIMEOUT_MS,
    );
    return { candidates, source };
  } finally {
    options.root.classList.remove(CAPTURE_ACTIVE_CLASS);
    options.root.classList.remove(CAPTURE_PRIVACY_CLASS);
    captureInFlight = false;
  }
}

export function waitForCaptureFrame(timeoutMs = 120): Promise<void> {
  return new Promise((resolve) => {
    let completed = false;
    let timeout: ReturnType<typeof setTimeout>;
    const finish = (): void => {
      if (completed) return;
      completed = true;
      clearTimeout(timeout);
      resolve();
    };
    timeout = setTimeout(finish, timeoutMs);
    window.requestAnimationFrame(finish);
  });
}

function withCaptureTimeout<T>(capture: Promise<T>, timeoutMs: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("Window capture timed out")), timeoutMs);
    capture.then(
      (value) => {
        clearTimeout(timeout);
        resolve(value);
      },
      (error: unknown) => {
        clearTimeout(timeout);
        reject(error);
      },
    );
  });
}
