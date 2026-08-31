import { describe, expect, test } from "bun:test";
import {
  applyCaptureCommand,
  createCaptureWindowState,
  type CaptureRect,
} from "./model.ts";

const SOURCE = { height: 801, scaleFactor: 2, width: 1200 } as const;

function region(x: number, y: number, width: number, height: number): CaptureRect {
  return { height, width, x, y };
}

function manualRegion(id: string, rect: CaptureRect) {
  return { id, rect, source: "manual" as const };
}

describe("capture window editor state", () => {
  test("starts with the observed capture defaults", () => {
    const state = createCaptureWindowState(SOURCE);

    expect(state.tool).toBe("move");
    expect(state.redactionSource).toBe("auto");
    expect(state.redactionStyle).toBe("mosaic");
    expect(state.privacyEnabled).toBe(true);
    expect(state.background).toEqual({ kind: "preset", id: "sea" });
    expect(state.padding).toBe(64);
    expect(state.shadow).toBe(true);
    expect(state.zoom).toBe(1);
    expect(state.regions).toEqual([]);
  });

  test("commits one normalized source-space region per completed gesture", () => {
    const initial = createCaptureWindowState(SOURCE);
    const next = applyCaptureCommand(initial, {
      kind: "add-region",
      id: "manual-1",
      rect: region(180, 120, -80, -40),
    });

    expect(next.regions).toEqual([
      manualRegion("manual-1", region(100, 80, 80, 40)),
    ]);
    expect(next.history.past).toHaveLength(1);
    expect(next.history.future).toHaveLength(0);
  });

  test("rejects tiny regions and clamps regions to the captured image", () => {
    const initial = createCaptureWindowState(SOURCE);
    const tiny = applyCaptureCommand(initial, {
      kind: "add-region",
      id: "manual-tiny",
      rect: region(10, 10, 5, 12),
    });
    const clamped = applyCaptureCommand(tiny, {
      kind: "add-region",
      id: "manual-edge",
      rect: region(1180, 790, 80, 40),
    });

    expect(tiny).toBe(initial);
    expect(clamped.regions).toEqual([
      manualRegion("manual-edge", region(1180, 790, 20, 11)),
    ]);
  });

  test("undo, redo, and clear preserve non-region editor preferences", () => {
    const initial = createCaptureWindowState(SOURCE);
    const configured = applyCaptureCommand(initial, { kind: "set-padding", padding: 96 });
    const withRegion = applyCaptureCommand(configured, {
      kind: "add-region",
      id: "manual-1",
      rect: region(20, 30, 80, 60),
    });
    const undone = applyCaptureCommand(withRegion, { kind: "undo" });
    const redone = applyCaptureCommand(undone, { kind: "redo" });
    const cleared = applyCaptureCommand(redone, { kind: "clear-regions" });

    expect(undone.regions).toEqual([]);
    expect(redone.regions).toEqual([
      manualRegion("manual-1", region(20, 30, 80, 60)),
    ]);
    expect(cleared.regions).toEqual([]);
    expect(cleared.padding).toBe(96);
  });

  test("retake changes the source revision without discarding user intent", () => {
    const initial = createCaptureWindowState(SOURCE);
    const withRegion = applyCaptureCommand(initial, {
      kind: "add-region",
      id: "manual-1",
      rect: region(20, 30, 80, 60),
    });
    const retaken = applyCaptureCommand(withRegion, {
      kind: "retake",
      source: { ...SOURCE, height: 820 },
    });

    expect(retaken.source).toEqual({ ...SOURCE, height: 820 });
    expect(retaken.sourceRevision).toBe(withRegion.sourceRevision + 1);
    expect(retaken.regions).toEqual(withRegion.regions);
    expect(retaken.background).toEqual(withRegion.background);
  });

  test("keeps automatic candidates inert until selected and removes one confirmed mask by id", () => {
    const initial = createCaptureWindowState(SOURCE);
    const selected = applyCaptureCommand(initial, {
      id: "project-1",
      kind: "select-automatic-region",
      rect: region(14, 145, 220, 28),
    });
    const removed = applyCaptureCommand(selected, {
      id: "project-1",
      kind: "remove-region",
    });

    expect(initial.regions).toEqual([]);
    expect(selected.regions).toEqual([
      { id: "project-1", rect: region(14, 145, 220, 28), source: "automatic" },
    ]);
    expect(removed.regions).toEqual([]);
    expect(removed.history.past).toHaveLength(2);
  });

  test("updates the editing tool, privacy style, background, and shadow independently", () => {
    const initial = createCaptureWindowState(SOURCE);
    const redact = applyCaptureCommand(initial, { kind: "set-tool", tool: "redact" });
    const draw = applyCaptureCommand(redact, {
      kind: "set-redaction-source",
      source: "draw",
    });
    const blurred = applyCaptureCommand(draw, {
      kind: "set-redaction-style",
      style: "blur",
    });
    const transparent = applyCaptureCommand(blurred, {
      kind: "set-background",
      background: { kind: "transparent" },
    });
    const noShadow = applyCaptureCommand(transparent, { kind: "set-shadow", shadow: false });
    const noPrivacy = applyCaptureCommand(noShadow, {
      kind: "set-privacy",
      enabled: false,
    });

    expect(noPrivacy.tool).toBe("redact");
    expect(noPrivacy.redactionSource).toBe("draw");
    expect(noPrivacy.redactionStyle).toBe("blur");
    expect(noPrivacy.background).toEqual({ kind: "transparent" });
    expect(noPrivacy.shadow).toBe(false);
    expect(noPrivacy.privacyEnabled).toBe(false);
  });

  test("clamps zoom and padding to the observed product boundaries", () => {
    const initial = createCaptureWindowState(SOURCE);
    const zoomedOut = applyCaptureCommand(initial, { kind: "set-zoom", zoom: 0.1 });
    const zoomedIn = applyCaptureCommand(zoomedOut, { kind: "set-zoom", zoom: 12 });
    const negativePadding = applyCaptureCommand(zoomedIn, {
      kind: "set-padding",
      padding: -20,
    });
    const largePadding = applyCaptureCommand(negativePadding, {
      kind: "set-padding",
      padding: 999,
    });

    expect(zoomedOut.zoom).toBe(0.4);
    expect(zoomedIn.zoom).toBe(6);
    expect(negativePadding.padding).toBe(0);
    expect(largePadding.padding).toBe(160);
  });
});
