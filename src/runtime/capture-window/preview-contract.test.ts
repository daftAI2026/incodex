/**
 * [INPUT]: 依赖共享编辑器模板、状态模型、双语文案与 preview harness
 * [OUTPUT]: 为浏览器预览、背景语义层级、交互结构和 Runtime 调试入口提供静态合同
 * [POS]: capture-window 的集成合同测试，在真实 Electron adapter 之前固定共享 UI 的产品边界
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
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
    expect(server).toContain('path === "/background-picker.css"');
    expect(server).toContain('path.startsWith("/capture-backgrounds/")');
    expect(server).toContain('"assets/capture-backgrounds"');
    expect(readFileSync(join(import.meta.dir, "preview.html"), "utf8")).toContain(
      'href="/background-picker.css"',
    );
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
    const retake = editor.slice(
      editor.indexOf("async function retake"),
      editor.indexOf("async function setPrivacy"),
    );

    expect(editor).toContain("onRetake?: (");
    expect(editor).toContain("privacyEnabled: boolean");
    expect(editor).toContain("automaticRegions: CaptureCandidate[]");
    expect(editor).not.toContain("onDetectRegions");
    expect(retake).toContain("resetView();");
  });

  test("matches the observed dialog structure without invented controls", () => {
    const state = createCaptureWindowState({ height: 720, scaleFactor: 1, width: 1280 });
    const copy = captureWindowCopy("en");
    const markup = captureWindowTemplate(state, copy);
    const css = ["capture-window.css", "background-picker.css"]
      .map((file) => readFileSync(join(import.meta.dir, file), "utf8"))
      .join("\n");
    const previewHtml = readFileSync(join(import.meta.dir, "preview.html"), "utf8");

    expect(markup).toContain('data-capture-icon="camera"');
    expect(markup).not.toContain("incodex-capture-subtitle");
    expect(markup).not.toContain("incodex-capture-style-grid");
    expect(markup).not.toContain("incodex-capture-footer-note");
    expect(markup).not.toContain('data-action="cancel"');
    expect(markup).toContain('data-action="retake"');
    expect(markup).toContain("incodex-capture-checker");
    expect(markup).toContain('data-action="zoom-reset" type="button" title="Reset view"');
    expect(markup).toContain('data-action="zoom-fit"');
    expect(markup).toContain('data-capture-icon="maximize"');
    expect(copy.shadow).toBe("Shadow");
    expect(captureWindowCopy("zh-CN").zoomReset).toBe("复位视图");
    expect(markup).toContain('data-color-trigger="background"');
    expect(markup).toContain(
      `style="--capture-swatch:url('/capture-backgrounds/sea.jpg') center / cover no-repeat"`,
    );
    expect(markup).toContain('aria-haspopup="dialog"');
    expect(markup).not.toContain('data-input="color" type="color"');

    const gradientsSection = markup.indexOf('data-background-section="gradients"');
    const wallpapersSection = markup.indexOf('data-background-section="wallpapers"');
    const plainColorSection = markup.indexOf('data-background-section="plain-color"');
    expect(markup).not.toContain('data-background-section="none"');
    expect(gradientsSection).toBeGreaterThan(-1);
    expect(gradientsSection).toBeLessThan(wallpapersSection);
    expect(wallpapersSection).toBeLessThan(plainColorSection);
    expect(markup.match(/data-background-color=/g)).toHaveLength(3);
    const plainColorMarkup = markup.slice(plainColorSection);
    expect(plainColorMarkup.indexOf('data-background="transparent"')).toBeLessThan(
      plainColorMarkup.indexOf('data-background-color="#121212"'),
    );
    expect(plainColorMarkup.indexOf('data-background-color="#d1444b"')).toBeLessThan(
      plainColorMarkup.indexOf("data-background-custom"),
    );
    expect(copy.backgroundNone).toBe("None");
    expect(copy.backgroundGradients).toBe("Gradients");
    expect(copy.backgroundWallpapers).toBe("Wallpapers");
    expect(copy.backgroundPlainColor).toBe("Plain color");
    expect(captureWindowCopy("zh-CN").backgroundGradients).toBe("渐变");
    expect(captureWindowCopy("zh-CN").backgroundWallpapers).toBe("壁纸");
    expect(captureWindowCopy("zh-CN").backgroundPlainColor).toBe("纯色");
    expect(readFileSync(join(import.meta.dir, "view.ts"), "utf8")).not.toContain(
      "capturePresetSections[",
    );
    const editor = readFileSync(join(import.meta.dir, "editor.ts"), "utf8");
    expect(editor.split("\n").length).toBeLessThanOrEqual(800);
    expect(editor).toContain('from "./background-controls.ts"');
    expect(readFileSync(join(import.meta.dir, "background-controls.ts"), "utf8")).toContain(
      'querySelectorAll<HTMLElement>("[data-background-color]")',
    );

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
    expect(css).toContain("background: var(--incodex-capture-skeleton)");
    expect(css).toContain('[data-incodex-capture-redact="blank"]::after');
    expect(css).toContain('[data-incodex-capture-redact="center"]::after');
    expect(css).toContain('[data-incodex-capture-redact="project"]::after');
    expect(css).toContain(
      ':nth-child(4n+1) [data-incodex-capture-redact]:not([data-incodex-capture-redact="project"])::after',
    );
    expect(css).toContain(".mac-traffic-light:first-of-type > div");
    expect(css).toMatch(
      /\.incodex-capture-background-option\[aria-pressed="true"\][\s\S]*?box-shadow: 0 0 0 2px var\(--incodex-capture-surface\), 0 0 0 4px var\(--incodex-capture-ring\)/,
    );
    expect(css).toMatch(
      /\.incodex-capture-background-option\s*\{[\s\S]*?height: calc\(var\(--incodex-capture-space\) \* 7\);[\s\S]*?width: calc\(var\(--incodex-capture-space\) \* 7\);/,
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
    const injectedCapture = readFileSync(join(import.meta.dir, "injected.ts"), "utf8");

    expect(inject).toContain('from "./capture-window/injected.ts"');
    expect(inject).toContain("window.__incodexCaptureDebug === true");
    expect(inject).toContain("openInjectedCaptureWindow");
    expect(inject).not.toContain('event.code === "KeyS"');
    expect(injectedCapture).toContain("markCodexPrivacyPlaceholders(document)");
    expect(injectedCapture).toContain("restorePrivacyPlaceholders()");
    expect(injectedCapture).not.toContain("createCodexPrivacyPlaceholderSession");
  });

  test("uses the existing Lucide camera at the shared header position and captures only on click", () => {
    const inject = readFileSync(join(import.meta.dir, "../inject.ts"), "utf8");
    const activate = inject.slice(inject.indexOf("async function activate"), inject.indexOf("function ensureStyle"));
    const start = inject.slice(inject.indexOf("function start"), inject.indexOf("declare global"));

    expect(inject).toContain('captureIcon("camera", 16)');
    expect(activate).toContain("openInjectedCaptureWindow");
    expect(start).not.toContain("openInjectedCaptureWindow");
  });
});
