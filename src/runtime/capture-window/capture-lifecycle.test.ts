import { describe, expect, test } from "bun:test";
import {
  CAPTURE_ACTIVE_CLASS,
  CAPTURE_PRIVACY_CLASS,
  capturePreparedWindow,
  prepareCaptureWindow,
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
});
