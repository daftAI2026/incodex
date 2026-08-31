import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { captureWindowCopy } from "./copy.ts";
import { applyCaptureCommand, createCaptureWindowState } from "./model.ts";
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
    expect(server).toContain('path.startsWith("/capture-backgrounds/")');
    expect(server).toContain('"assets/capture-backgrounds"');
    expect(preview).toContain("mountCaptureWindowEditor");
    expect(preview).not.toContain("incodex-main.cts");
    expect(editor).toContain("onCopy?:");
    expect(editor).toContain("onSave?:");
    expect(editor).not.toContain("onDetectRegions");
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

    expect(editor).toContain('dispatch({ kind: "set-redaction-style", style })');
    expect(editor).toContain("wireToolbarActions(root, dispatch, dispatchRegion, resetView)");
    expect(editor).not.toContain("root.innerHTML = captureToolbarTemplate");
  });

  test("retakes one atomic source-and-candidate snapshot with the current privacy choice", () => {
    const editor = readFileSync(join(import.meta.dir, "editor.ts"), "utf8");

    expect(editor).toContain("onRetake?: (");
    expect(editor).toContain("privacyEnabled: boolean");
    expect(editor).toContain("automaticRegions: CaptureCandidate[]");
    expect(editor).not.toContain("onDetectRegions");
  });

  test("matches the observed dialog structure without invented controls", () => {
    const state = createCaptureWindowState({ height: 720, scaleFactor: 1, width: 1280 });
    const copy = captureWindowCopy("en");
    const markup = captureWindowTemplate(state, copy);
    const css = readFileSync(join(import.meta.dir, "capture-window.css"), "utf8");
    const previewHtml = readFileSync(join(import.meta.dir, "preview.html"), "utf8");

    expect(markup).toContain('data-capture-icon="camera"');
    expect(markup).not.toContain("incodex-capture-subtitle");
    expect(markup).not.toContain("incodex-capture-style-grid");
    expect(markup).not.toContain("incodex-capture-footer-note");
    expect(markup).not.toContain('data-action="cancel"');
    expect(markup).toContain('data-action="retake"');
    expect(markup).toContain("incodex-capture-checker");
    expect(markup).toContain('data-action="zoom-reset" type="button" title="Reset zoom"');
    expect(markup).toContain('data-color-trigger="background"');
    expect(markup).toContain(
      `style="--capture-swatch:url('/capture-backgrounds/sea.jpg') center / cover no-repeat"`,
    );
    expect(markup).toContain('aria-haspopup="dialog"');
    expect(markup).not.toContain('data-input="color" type="color"');

    const solidMarkup = captureWindowTemplate(
      applyCaptureCommand(
        applyCaptureCommand(state, { kind: "set-tool", tool: "redact" }),
        { kind: "set-redaction-style", style: "solid" },
      ),
      copy,
    );
    expect(solidMarkup).toContain('data-color-trigger="solid"');

    const wallpaperMarkup = captureWindowTemplate(
      applyCaptureCommand(state, {
        background: { dataUrl: "data:image/png;base64,wallpaper", kind: "wallpaper" },
        kind: "set-background",
      }),
      copy,
    );
    expect(wallpaperMarkup).toContain('data-background-wallpaper type="button"');
    expect(wallpaperMarkup).toContain('src="data:image/png;base64,wallpaper"');
    expect(wallpaperMarkup).toContain('data-action="change-wallpaper"');
    expect(wallpaperMarkup).toContain(copy.changeImage);

    const background = markup.indexOf(copy.background);
    const padding = markup.indexOf(copy.padding);
    const shadow = markup.indexOf(copy.shadow);
    const privacy = markup.indexOf(copy.privacy);
    expect(background).toBeGreaterThan(-1);
    expect(background).toBeLessThan(padding);
    expect(padding).toBeLessThan(shadow);
    expect(shadow).toBeLessThan(privacy);

    expect(css).toContain("backdrop-filter: blur(3px)");
    expect(css).toContain("max-width: 56rem");
    expect(css).not.toContain("padding-top: calc(var(--incodex-capture-space) * 13)");
    expect(previewHtml).toContain("--spacing: .25rem");
    expect(css).toContain(
      "grid-template-columns: minmax(0, 1fr) calc(var(--incodex-capture-space) * 56)",
    );
    expect(css).toContain("height: calc(var(--incodex-capture-space) * 7)");
    expect(css).toContain("height: 50vh");
    expect(css).toContain("background-size: 16px 16px");
    expect(css).toContain("html.incodex-capturing [data-incodex-capture-hide]");
    expect(css).toContain("color-mix(in srgb, currentColor 22%, transparent)");
    expect(css).toContain('[data-incodex-capture-redact="blank"]::after');
    expect(css).toContain('[data-incodex-capture-redact="center"]::after');
    expect(css).toContain(":nth-child(4n+1) [data-incodex-capture-redact]::after");
    expect(css).toContain(".mac-traffic-light:first-of-type > div");
    expect(css).toMatch(
      /\.incodex-capture-background-option\[aria-pressed="true"\][\s\S]*?box-shadow: 0 0 0 2px var\(--incodex-capture-surface\), 0 0 0 4px var\(--incodex-capture-ring\)/,
    );
  });

  test("keeps the dialog and canvas nodes stable while editor controls change", () => {
    const editor = readFileSync(join(import.meta.dir, "editor.ts"), "utf8");

    expect(editor).toContain("function refreshEditor");
    expect(editor).toContain("function refreshToolbar");
    expect(editor).toContain("readState: () => CaptureWindowState");
    expect(editor).toContain(
      "state = applyCaptureCommand(state, command);\n    refreshEditor(command);",
    );
    expect(editor).not.toContain(
      "state = applyCaptureCommand(state, command);\n    render();",
    );
  });

  test("keeps the CDP experiment behind an explicit bootstrap without adding a public shortcut", () => {
    const inject = readFileSync(join(import.meta.dir, "../inject.ts"), "utf8");

    expect(inject).toContain('from "./capture-window/injected.ts"');
    expect(inject).toContain("window.__incodexCaptureDebug === true");
    expect(inject).toContain("openInjectedCaptureWindow");
    expect(inject).not.toContain('event.code === "KeyS"');
  });
});
