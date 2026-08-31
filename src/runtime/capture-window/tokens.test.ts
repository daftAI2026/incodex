import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const cssPath = join(import.meta.dir, "capture-window.css");

describe("capture window visual tokens", () => {
  test("maps Incodex semantics to live Codex tokens with stable fallbacks", () => {
    const css = readFileSync(cssPath, "utf8");

    expect(css).toContain("--incodex-capture-space: var(--spacing, 4px)");
    expect(css).toContain("--incodex-capture-surface: var(--color-surface-elevated-secondary");
    expect(css).toContain("--incodex-capture-text: var(--color-text");
    expect(css).toContain("--incodex-capture-border: var(--color-border");
    expect(css).toContain("--incodex-capture-radius-dialog: var(--radius-xl");
    expect(css).toContain("--incodex-capture-font-xs: var(--text-xs, 12px)");
    expect(css).toContain("--incodex-capture-icon-xs: calc(var(--incodex-capture-space) * 4)");
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

  test("keeps the selected background ring inside its grid and sizes thumbnail icons from tokens", () => {
    const css = readFileSync(cssPath, "utf8");

    expect(css).toMatch(/\.incodex-capture-background-grid[\s\S]*?padding: var\(--incodex-capture-space\)/);
    expect(css).toMatch(/\.incodex-capture-background-option svg[\s\S]*?height: var\(--incodex-capture-icon-compact\)/);
    expect(css).toMatch(/\.incodex-capture-background-option svg[\s\S]*?width: var\(--incodex-capture-icon-compact\)/);
    expect(css).not.toContain("font-size: 11px");
    expect(css).not.toContain("font-size: 10px");
  });
});
