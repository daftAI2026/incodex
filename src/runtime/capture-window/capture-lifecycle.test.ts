import { describe, expect, test } from "bun:test";
import {
  CAPTURE_ACTIVE_CLASS,
  CAPTURE_PRIVACY_CLASS,
  capturePreparedWindow,
  prepareCaptureWindow,
  waitForCaptureFrame,
} from "./capture-lifecycle.ts";

class FakeClassList {
  readonly values = new Set<string>();

  add(value: string): void {
    this.values.add(value);
  }

  remove(value: string): void {
    this.values.delete(value);
  }

  contains(value: string): boolean {
    return this.values.has(value);
  }
}

describe("capture preparation lifecycle", () => {
  test("loads persisted privacy before taking the initial capture", async () => {
    const events: string[] = [];
    const result = await prepareCaptureWindow({
      capture: async (privacyEnabled) => {
        events.push(`capture:${privacyEnabled}`);
        return { candidates: [], source: "png" };
      },
      loadPreferences: () => {
        events.push("preferences");
        return { privacyEnabled: false };
      },
    });

    expect(events).toEqual(["preferences", "capture:false"]);
    expect(result).toEqual({
      preferences: { privacyEnabled: false },
      snapshot: { candidates: [], source: "png" },
    });
  });

  test("keeps the editor closed when the initial capture fails", async () => {
    let capturedError: unknown;
    const result = await prepareCaptureWindow({
      capture: async () => {
        throw new Error("capture failed");
      },
      loadPreferences: () => ({ privacyEnabled: true }),
      onCaptureError: (error) => {
        capturedError = error;
      },
    });

    expect(result).toBeNull();
    expect(capturedError).toBeInstanceOf(Error);
  });

  test("prepares the live document for two frames before collecting candidates and capturing", async () => {
    const classList = new FakeClassList();
    const events: string[] = [];

    const result = await capturePreparedWindow({
      begin: async () => {
        events.push("begin");
      },
      capture: async () => {
        events.push("capture");
        expect(classList.contains(CAPTURE_ACTIVE_CLASS)).toBe(true);
        expect(classList.contains(CAPTURE_PRIVACY_CLASS)).toBe(true);
        return "png";
      },
      collectCandidates: () => {
        events.push("collect");
        return [{ height: 20, id: "r:10:20:80:20", width: 80, x: 10, y: 20 }];
      },
      privacyEnabled: true,
      root: { classList },
      waitForFrame: async () => {
        events.push("frame");
      },
    });

    expect(events).toEqual(["begin", "frame", "frame", "collect", "capture"]);
    expect(result).toEqual({
      candidates: [{ height: 20, id: "r:10:20:80:20", width: 80, x: 10, y: 20 }],
      source: "png",
    });
    expect(classList.contains(CAPTURE_ACTIVE_CLASS)).toBe(false);
    expect(classList.contains(CAPTURE_PRIVACY_CLASS)).toBe(false);
  });

  test("keeps candidate detection independent from privacy and always restores document classes", async () => {
    const classList = new FakeClassList();

    await expect(
      capturePreparedWindow({
        capture: async () => {
          expect(classList.contains(CAPTURE_ACTIVE_CLASS)).toBe(true);
          expect(classList.contains(CAPTURE_PRIVACY_CLASS)).toBe(false);
          throw new Error("capture failed");
        },
        collectCandidates: () => [],
        privacyEnabled: false,
        root: { classList },
        waitForFrame: async () => {},
      }),
    ).rejects.toThrow("capture failed");

    expect(classList.contains(CAPTURE_ACTIVE_CLASS)).toBe(false);
    expect(classList.contains(CAPTURE_PRIVACY_CLASS)).toBe(false);
  });

  test("treats begin and candidate discovery as best-effort preparation", async () => {
    const classList = new FakeClassList();
    const result = await capturePreparedWindow({
      begin: async () => {
        throw new Error("begin unavailable");
      },
      capture: async () => "png",
      collectCandidates: () => {
        throw new Error("scan failed");
      },
      privacyEnabled: true,
      root: { classList },
      waitForFrame: async () => {},
    });

    expect(result).toEqual({ candidates: [], source: "png" });
  });

  test("times out a stalled capture and rejects overlapping capture attempts", async () => {
    const classList = new FakeClassList();
    let finishCapture: ((value: string) => void) | undefined;
    const firstCapture = capturePreparedWindow({
      capture: () => new Promise<string>((resolve) => {
        finishCapture = resolve;
      }),
      collectCandidates: () => [],
      privacyEnabled: false,
      root: { classList },
      timeoutMs: 50,
      waitForFrame: async () => {},
    });

    await expect(capturePreparedWindow({
      capture: async () => "second",
      collectCandidates: () => [],
      privacyEnabled: false,
      root: { classList: new FakeClassList() },
      waitForFrame: async () => {},
    })).rejects.toThrow("already in progress");

    while (!finishCapture) await Promise.resolve();
    finishCapture?.("first");
    expect((await firstCapture).source).toBe("first");

    await expect(capturePreparedWindow({
      capture: () => new Promise<string>(() => {}),
      collectCandidates: () => [],
      privacyEnabled: false,
      root: { classList },
      timeoutMs: 5,
      waitForFrame: async () => {},
    })).rejects.toThrow("timed out");
    expect(classList.contains(CAPTURE_ACTIVE_CLASS)).toBe(false);
  });

  test("finishes a frame wait when an occluded window does not receive animation frames", async () => {
    const originalWindow = globalThis.window;
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: { requestAnimationFrame: () => 1 },
    });
    try {
      const startedAt = performance.now();
      await waitForCaptureFrame(5);
      expect(performance.now() - startedAt).toBeGreaterThanOrEqual(4);
    } finally {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: originalWindow,
      });
    }
  });
});
