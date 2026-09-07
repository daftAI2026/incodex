/**
 * [INPUT]: 依赖 live-tokens 的 class 到语义变量解析
 * [OUTPUT]: 约束官方变量改名、歧义拒绝与颜色快照拒绝
 * [POS]: 截图外壳 token 适配的纯回归；不将某组官方 token 固化进预期
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { resolveClassToken } from "./live-tokens.ts";

describe("official dialog token discovery", () => {
  test("follows whichever semantic variable the actual class selects", () => {
    const rules = [
      { selector: ".surface-old", property: "background-color", value: "var(--old-surface)" },
      { selector: ".surface-next", property: "background-color", value: "var(--next-surface)" },
    ];
    expect(resolveClassToken(["surface-old"], "background-color", rules)).toBe("var(--old-surface)");
    expect(resolveClassToken(["surface-next"], "background-color", rules)).toBe("var(--next-surface)");
  });
  test("does not freeze RGB or guess between competing utility classes", () => {
    const rules = [
      { selector: ".snapshot", property: "color", value: "rgb(1, 2, 3)" },
      { selector: ".a", property: "color", value: "var(--a)" },
      { selector: ".b", property: "color", value: "var(--b)" },
    ];
    expect(resolveClassToken(["snapshot"], "color", rules)).toBeNull();
    expect(resolveClassToken(["a", "b"], "color", rules)).toBeNull();
    expect(resolveClassToken([], "color", rules)).toBeNull();
  });
  test("does not borrow descendant or state rules as the dialog base", () => {
    expect(resolveClassToken(["a"], "color", [
      { selector: ".a:hover", property: "color", value: "var(--hover)" },
      { selector: ".a span", property: "color", value: "var(--child)" },
    ])).toBeNull();
  });
});

describe("Codex live dialog CSS shapes", () => {
  test("preserves opacity expressions and the last active declaration", () => {
    expect(resolveClassToken(["surface/90"], "background-color", [
      { selector: ".surface\\/90", property: "background-color", value: "var(--surface)" },
      { selector: ".surface\\/90", property: "background-color", value: "color-mix(in oklab, var(--surface) 90%, transparent)" },
    ])).toBe("color-mix(in oklab, var(--surface) 90%, transparent)");
  });
  test("rejects element-local Tailwind shadow machinery as a root token", () => {
    expect(resolveClassToken(["shadow-lg"], "box-shadow", [
      { selector: ".shadow-lg", property: "box-shadow", value: "var(--tw-shadow)" },
    ])).toBeNull();
  });
});
