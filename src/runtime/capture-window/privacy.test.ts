import { describe, expect, test } from "bun:test";
import {
  CAPTURE_CANDIDATE_SELECTOR,
  collectCaptureCandidates,
} from "./privacy.ts";

type FakeRect = {
  bottom: number;
  height: number;
  left: number;
  right: number;
  top: number;
  width: number;
};

class FakeElement {
  readonly children = new Set<FakeElement>();
  readonly hiddenFromCapture: boolean;
  readonly rect: FakeRect;
  readonly tagName: string;
  readonly textContent: string;
  readonly value: string;
  readonly visible: boolean;

  constructor(options: {
    hiddenFromCapture?: boolean;
    rect: FakeRect;
    tagName?: string;
    text?: string;
    value?: string;
    visible?: boolean;
  }) {
    this.hiddenFromCapture = options.hiddenFromCapture ?? false;
    this.rect = options.rect;
    this.tagName = options.tagName ?? "P";
    this.textContent = options.text ?? "Private text";
    this.value = options.value ?? "";
    this.visible = options.visible ?? true;
  }

  checkVisibility(): boolean {
    return this.visible;
  }

  closest(selector: string): FakeElement | null {
    return selector === "[data-incodex-capture-hide]" && this.hiddenFromCapture ? this : null;
  }

  contains(element: FakeElement): boolean {
    return this.children.has(element);
  }

  getBoundingClientRect(): FakeRect {
    return this.rect;
  }
}

function rect(left: number, top: number, width: number, height: number): FakeRect {
  return { bottom: top + height, height, left, right: left + width, top, width };
}

function fakeDocument(elements: FakeElement[]): Document {
  return {
    querySelectorAll(selector: string): FakeElement[] {
      expect(selector).toBe(CAPTURE_CANDIDATE_SELECTOR);
      return elements;
    },
  } as unknown as Document;
}

describe("capture redaction candidates", () => {
  test("collects visible text, image, and input candidates across the viewport", () => {
    const elements = [
      new FakeElement({ rect: rect(10, 20, 120, 24), tagName: "P", text: "Project name" }),
      new FakeElement({ rect: rect(300, 180, 240, 80), tagName: "IMG", text: "" }),
      new FakeElement({ rect: rect(420, 520, 260, 44), tagName: "TEXTAREA", value: "Secret" }),
    ];

    const candidates = collectCaptureCandidates(fakeDocument(elements), {
      height: 801,
      width: 1200,
    });

    expect(candidates).toEqual([
      { height: 24, id: "r:10:20:120:24", width: 120, x: 10, y: 20 },
      { height: 80, id: "r:300:180:240:80", width: 240, x: 300, y: 180 },
      { height: 44, id: "r:420:520:260:44", width: 260, x: 420, y: 520 },
    ]);
  });

  test("accepts any non-empty textarea value like the reference collector", () => {
    const candidates = collectCaptureCandidates(fakeDocument([
      new FakeElement({ rect: rect(20, 30, 120, 24), tagName: "TEXTAREA", value: "x" }),
    ]), { height: 801, width: 1200 });

    expect(candidates).toEqual([
      { height: 24, id: "r:20:30:120:24", width: 120, x: 20, y: 30 },
    ]);
  });

  test("rejects hidden, empty, tiny, offscreen, invisible, and nested candidates", () => {
    const parent = new FakeElement({ rect: rect(10, 20, 180, 60), text: "Parent content" });
    const child = new FakeElement({ rect: rect(20, 30, 100, 20), text: "Child content" });
    parent.children.add(child);
    const elements = [
      parent,
      child,
      new FakeElement({ hiddenFromCapture: true, rect: rect(10, 120, 100, 20) }),
      new FakeElement({ rect: rect(10, 160, 20, 20) }),
      new FakeElement({ rect: rect(10, 200, 100, 10) }),
      new FakeElement({ rect: rect(1300, 220, 100, 20) }),
      new FakeElement({ rect: rect(10, 260, 100, 20), text: "  " }),
      new FakeElement({ rect: rect(10, 300, 100, 20), tagName: "TEXTAREA", value: "" }),
      new FakeElement({ rect: rect(10, 340, 100, 20), visible: false }),
    ];

    const candidates = collectCaptureCandidates(fakeDocument(elements), {
      height: 801,
      width: 1200,
    });

    expect(candidates).toEqual([
      { height: 60, id: "r:10:20:180:60", width: 180, x: 10, y: 20 },
    ]);
  });

  test("clips candidates to the viewport and caps one capture at 150 regions", () => {
    const elements = Array.from({ length: 170 }, (_, index) =>
      new FakeElement({ rect: rect(-10, index * 4, 60, 20), text: `Candidate ${index}` }),
    );

    const candidates = collectCaptureCandidates(fakeDocument(elements), {
      height: 801,
      width: 1200,
    });

    expect(candidates).toHaveLength(150);
    expect(candidates[0]).toEqual({
      height: 20,
      id: "r:0:0:50:20",
      width: 50,
      x: 0,
      y: 0,
    });
  });
});
