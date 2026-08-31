import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { captureWindowCopy } from "./copy.ts";
import { createCaptureWindowState } from "./model.ts";
import { captureWindowTemplate } from "./view.ts";

const root = join(import.meta.dir, "../../..");

describe("capture window preview", () => {
  test("is a local browser harness over the reusable editor", () => {
    const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as {
      scripts?: Record<string, string>;
    };
    const server = readFileSync(join(root, "scripts/capture-window-preview.ts"), "utf8");
    const editor = readFileSync(join(import.meta.dir, "editor.ts"), "utf8");
    const preview = readFileSync(join(import.meta.dir, "preview.ts"), "utf8");

    expect(packageJson.scripts?.["preview:capture-window"]).toBe(
      "bun scripts/capture-window-preview.ts",
    );
    expect(server).toContain("127.0.0.1");
    expect(preview).toContain("mountCaptureWindowEditor");
    expect(preview).not.toContain("incodex-main.cts");
    expect(editor).toContain("onCopy?:");
    expect(editor).toContain("onSave?:");
    expect(editor).toContain("onDetectRegions?:");
    expect(`${server}\n${preview}`).not.toMatch(/https?:\/\//);
  });

  test("keeps Lucide icons local and current-color driven", () => {
    const icons = readFileSync(join(import.meta.dir, "icons.ts"), "utf8");

    expect(icons).toContain('stroke="currentColor"');
    expect(icons).toContain('stroke-linecap="round"');
    expect(icons).not.toContain('src="http');
    expect(icons).not.toContain("fetch(");
  });

  test("switches redaction appearance without rebuilding the modal shell", () => {
    const editor = readFileSync(join(import.meta.dir, "editor.ts"), "utf8");

    expect(editor).toContain("updateRedactionStyle");
    expect(editor).toContain("wireActions(root, dispatch, dispatchRegion, preview");
    expect(editor).not.toMatch(
      /const actions:[\s\S]*"style-mosaic"[\s\S]*"tool-move"/,
    );
  });

  test("matches the observed dialog structure without invented controls", () => {
    const state = createCaptureWindowState({ height: 720, scaleFactor: 1, width: 1280 });
    const copy = captureWindowCopy("en");
    const markup = captureWindowTemplate(state, copy);
    const css = readFileSync(join(import.meta.dir, "capture-window.css"), "utf8");

    expect(markup).toContain('data-capture-icon="camera"');
    expect(markup).not.toContain("incodex-capture-subtitle");
    expect(markup).not.toContain("incodex-capture-style-grid");
    expect(markup).not.toContain("incodex-capture-footer-note");
    expect(markup).not.toContain('data-action="cancel"');
    expect(markup).toContain('data-action="retake"');

    const background = markup.indexOf(copy.background);
    const padding = markup.indexOf(copy.padding);
    const shadow = markup.indexOf(copy.shadow);
    const privacy = markup.indexOf(copy.privacy);
    expect(background).toBeGreaterThan(-1);
    expect(background).toBeLessThan(padding);
    expect(padding).toBeLessThan(shadow);
    expect(shadow).toBeLessThan(privacy);

    expect(css).toContain("backdrop-filter: blur(3px)");
    expect(css).toContain("max-width: 896px");
    expect(css).toContain("grid-template-columns: minmax(0, 1fr) 224px");
    expect(css).toContain("height: 50vh");
  });
});
