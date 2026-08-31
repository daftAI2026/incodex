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
});
