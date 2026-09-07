/**
 * [INPUT]: 依赖 capture-window 的共享样式、背景选择器样式与编辑器模板源码
 * [OUTPUT]: 为视觉令牌、布局密度、样式职责边界和图标尺寸提供回归合同
 * [POS]: capture-window 的视觉结构测试，防止产品语义演进重新引入魔法值或臃肿样式单体
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "../../..");
const cssPath = join(import.meta.dir, "capture-window.css");
const backgroundPickerCssPath = join(import.meta.dir, "background-picker.css");
const viewPath = join(import.meta.dir, "view.ts");

describe("capture window visual tokens", () => {
  test("lets the bitmap fill its fitted frame without a second pixel cap", () => {
    const css = readFileSync(cssPath, "utf8");
    const canvas = css.match(/\.incodex-capture-canvas\s*\{([^}]*)\}/)?.[1];
    expect(canvas).toBeDefined();
    expect(canvas).toContain("max-width: none");
    expect(canvas).toContain("max-height: none");
    expect(canvas).not.toContain("760px");
    expect(canvas).not.toContain("280px");
  });
  test("enlarges only the window and preserves control density", () => {
    const css = readFileSync(cssPath, "utf8");
    expect(css).toContain("--incodex-capture-window-scale: 1.2");
    expect(css).toContain("--incodex-capture-font-base: var(--text-base, 14px)");
    expect(css).toContain("--incodex-capture-font-sm: var(--text-sm, 13px)");
    expect(css).not.toContain("--incodex-capture-editor-scale");
    expect(css).toContain("width: calc(clamp(56rem, 60vw, 64rem) * var(--incodex-capture-window-scale))");
    expect(css).toContain("--incodex-capture-stage-height: calc((50vh + var(--incodex-capture-chrome-height)) * var(--incodex-capture-window-scale) - var(--incodex-capture-chrome-height))");
    expect(css).toContain("grid-template-rows: calc(var(--incodex-capture-space) * 7) var(--incodex-capture-stage-height)");
    expect(css).toContain("height: var(--incodex-capture-stage-height)");
  });
  test("paints the translucent shell once instead of stacking an inspector surface", () => {
    const css = readFileSync(cssPath, "utf8");
    const inspector = css.match(/\.incodex-capture-inspector\s*\{([^}]*)\}/)?.[1];
    expect(inspector).toBeDefined();
    expect(inspector).toContain("background: transparent");
    expect(inspector).not.toContain("var(--incodex-capture-surface)");
    expect(css).toMatch(/\.incodex-capture-dialog\s*\{[^}]*background: var\(--incodex-capture-surface\)/);
  });

  test("maps Incodex semantics to live Codex tokens with stable fallbacks", () => {
    const css = readFileSync(cssPath, "utf8");

    expect(css).toContain("--incodex-capture-space: var(--spacing, 4px)");
    expect(css).toContain("--incodex-capture-surface: var(--color-surface-elevated-secondary");
    expect(css).toContain("--incodex-capture-text: var(--color-text");
    expect(css).toContain("--incodex-capture-border: var(--color-border");
    expect(css).toContain("--incodex-capture-radius-dialog: var(--radius-xl");
    expect(css).toContain("--incodex-capture-font-xs: var(--text-xs, 12px)");
    expect(css).toContain("--incodex-capture-icon-base: calc(var(--incodex-capture-space) * 4)");
    expect(css).toContain("--incodex-capture-icon-sm: calc(var(--incodex-capture-space) * 3.5)");
    expect(css).toContain("--incodex-capture-skeleton: var(--color-background-button-tertiary-active");
    expect(css).toContain("--color-background-primary-soft-active");
    expect(css).toContain("backdrop-filter: blur(3px)");
    expect(css).toContain("font-family: var(--vscode-font-family");
  });

  test("uses the Codex four-pixel spacing base instead of scattered magic gaps", () => {
    const css = readFileSync(cssPath, "utf8");

    expect(css).toContain("calc(var(--incodex-capture-space) * 2)");
    expect(css).toContain("calc(var(--incodex-capture-space) * 3)");
    expect(css).toContain("calc(var(--incodex-capture-space) * 4)");
  });

  test("lets the fitted canvas reach the limiting preview edge without an inner gutter", () => {
    const css = readFileSync(cssPath, "utf8");

    expect(css).toMatch(/\.incodex-capture-stage\s*\{[^}]*padding: 0/);
  });

  test("fills five columns with square background options and a safe selection gutter", () => {
    const css = readFileSync(backgroundPickerCssPath, "utf8");

    expect(css).toMatch(
      /\.incodex-capture-background-grid\s*\{[^}]*padding: var\(--incodex-capture-space\)/,
    );
    expect(css).toMatch(
      /\.incodex-capture-background-grid\s*\{[^}]*grid-template-columns: repeat\(5, minmax\(0, 1fr\)\)/,
    );
    expect(css).toMatch(/\.incodex-capture-background-grid\s*\{[^}]*justify-items: stretch/);
    expect(css).toMatch(/\.incodex-capture-background-grid\s*\{[^}]*width: 100%/);
    expect(css).toMatch(
      /\.incodex-capture-background-grid-plain\s*\{[^}]*grid-template-columns: repeat\(8, minmax\(0, 1fr\)\)/,
    );
    const option = css.match(/(?:^|\n)\.incodex-capture-background-option\s*\{([^}]*)\}/)?.[1];
    expect(option).toContain("aspect-ratio: 1");
    expect(option).toContain("width: 100%");
    expect(option).toContain("height: auto");
    expect(option).toContain("min-width: 0");
    expect(css).toMatch(/\.incodex-capture-background-grid-plain \.incodex-capture-background-option\s*\{[^}]*border-radius: var\(--radius-full/);
    expect(css).toMatch(/\.incodex-capture-background-option\s*\{[\s\S]*?border-radius: var\(--incodex-capture-radius-sm/);
    expect(css).toMatch(/\.incodex-capture-background-option\s*\{[\s\S]*?background-origin: border-box/);
    expect(css).toMatch(/\.incodex-capture-background-option\s*\{[\s\S]*?background-clip: border-box/);
    expect(css).toMatch(/\.incodex-capture-background-option\s*\{[\s\S]*?overflow: hidden/);
    const selectedRule = css.match(
      /\.incodex-capture-background-option\[aria-pressed="true"\],[\s\S]*?\{([^}]*)\}/,
    )?.[1];
    expect(selectedRule).toBeDefined();
    expect(selectedRule).not.toContain("border-color:");
    expect(css).not.toMatch(/@supports \(corner-shape:[\s\S]*?\.incodex-capture-background-option/);
    expect(css).toMatch(
      /\.incodex-capture-wallpaper-label\s*\{[^}]*color: var\(--incodex-capture-text-secondary\)/,
    );
    expect(css).toMatch(
      /\.incodex-capture-wallpaper-label:hover\s*\{[^}]*color: var\(--incodex-capture-text\)/,
    );
  });

  test("keeps background picker styling isolated from the editor shell", () => {
    const shellCss = readFileSync(cssPath, "utf8");
    const pickerCss = readFileSync(backgroundPickerCssPath, "utf8");
    const buildRuntime = readFileSync(join(root, "src/build-runtime.ts"), "utf8");

    expect(shellCss.split("\n").length).toBeLessThanOrEqual(800);
    expect(shellCss).not.toContain(".incodex-capture-background-sections");
    expect(pickerCss).toContain(".incodex-capture-background-sections");
    expect(buildRuntime).toContain('capture-window/background-picker.css');
  });

  test("sizes editor icon roles from Codex spacing tokens", () => {
    const css = [cssPath, backgroundPickerCssPath]
      .map((path) => readFileSync(path, "utf8"))
      .join("\n");
    const view = readFileSync(viewPath, "utf8");

    expect(css).toMatch(/\.incodex-capture-heading > svg[\s\S]*?height: var\(--incodex-capture-icon-base\)/);
    expect(css).toMatch(/\.incodex-capture-icon-button > svg[\s\S]*?height: var\(--incodex-capture-icon-sm\)/);
    expect(css).toMatch(/\.incodex-capture-button > svg[\s\S]*?height: var\(--incodex-capture-icon-sm\)/);
    expect(css).toMatch(/\.incodex-capture-background-option svg[\s\S]*?height: var\(--incodex-capture-icon-sm\)/);
    expect(css).not.toContain("font-size: 11px");
    expect(css).not.toContain("font-size: 10px");
    expect(view).not.toMatch(/captureIcon\([^\n]+,\s*\d+\)/);
  });
  test("system wallpaper catalogs scroll without increasing the preview-owned window height", () => {
    const css = readFileSync(cssPath, "utf8");
    expect(css).toMatch(/\.incodex-capture-inspector\s*\{[^}]*max-height: calc\(var\(--incodex-capture-stage-height\) \+ var\(--incodex-capture-space\) \* 9\)/);
  });

  test("mouse focus on an unselected wallpaper must not look like a second selection", () => {
    const css = readFileSync(backgroundPickerCssPath, "utf8");
    expect(css).not.toContain(".incodex-capture-wallpaper-label:focus-within");
    expect(css).toContain(".incodex-capture-wallpaper-label:focus-visible");
    expect(css).toContain('.incodex-capture-background-option[aria-busy="true"]');
  });

  test("transparent heading actions hover with surface text, not solid-button inverse text", () => {
    const css = readFileSync(backgroundPickerCssPath, "utf8");
    const hover = css.match(/\.incodex-capture-background-expand:hover\s*\{([^}]*)\}/)?.[1];
    expect(hover).toBeDefined();
    expect(hover).toContain("color: var(--incodex-capture-text)");
    expect(hover).not.toContain("--incodex-capture-primary-text");
  });

  test("picker icons inherit their paired control surface instead of inverse primary text", () => {
    const css = readFileSync(backgroundPickerCssPath, "utf8");
    const icon = css.match(/\.incodex-capture-background-option svg\s*\{([^}]*)\}/)?.[1];
    expect(icon).toContain("color: inherit");
    const picker = css.match(/\.incodex-capture-color-label\[data-selected="false"\]\s*\{([^}]*)\}/)?.[1];
    expect(picker).toContain("background: var(--incodex-capture-surface-tertiary)");
    expect(picker).toContain("color: var(--incodex-capture-text)");
    expect(css).not.toContain("--incodex-capture-primary-text");
  });

});

 test("padding ticks are decorative ten-division guides without changing the native slider", () => {
  const view = readFileSync(viewPath, "utf8");
  const css = readFileSync(cssPath, "utf8");
  expect(view).toContain('class="incodex-capture-range-ticks" aria-hidden="true"');
  expect(view).toContain('Array.from({ length: 11 }');
  expect(view).toContain('index * 10');
  expect(view).toContain('type="range" min="${CAPTURE_MIN_PADDING}" max="${CAPTURE_MAX_PADDING}" step="${CAPTURE_PADDING_STEP}"');
  expect(css).toMatch(/\.incodex-capture-range-ticks\s*\{[^}]*pointer-events: none/);
  expect(css).toMatch(/\.incodex-capture-range-ticks\s*\{[^}]*color: var\(--incodex-capture-text-tertiary\)/);
  expect(css).toContain("left: var(--capture-tick-position)");
});

test("padding heading and live updates display percent, not pixels", () => {
  const view = readFileSync(viewPath, "utf8");
  const editor = readFileSync(join(import.meta.dir, "editor.ts"), "utf8");
  expect(view).toContain("${state.padding}%");
  expect(editor).not.toContain("${state.padding}px");
  expect(editor.match(/\$\{state.padding\}%/g)?.length).toBe(2);
});
