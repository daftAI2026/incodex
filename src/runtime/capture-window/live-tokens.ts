/**
 * [INPUT]: 依赖官方 modal dialog 的实际 class 与同源 CSSOM 变量声明
 * [OUTPUT]: 为 Shot 外壳提供动态语义变量覆盖，不复制布局或冻结 RGB
 * [POS]: capture-window 的官方视觉适配器；基础 CSS 仅作为未取样时的兼容映射
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
type TokenRule = { selector: string; property: string; value: string };
const DIALOG_PROPERTIES = {
  "background-color": "surface",
  color: "text",
  "--tw-ring-color": "border",
  "border-color": "border",
  "border-radius": "radius-dialog",
  "box-shadow": "shadow",
} as const;
const STYLE_ID = "incodex-capture-live-tokens";
const OWN_UI = "[data-incodex-capture], [data-incodex-capture-preview], [data-incodex-capture-host]";

export function resolveClassToken(
  classes: string[], property: string, rules: TokenRule[],
): string | null {
  // 只采直接 utility；保留同一 class 的最后一条有效声明和透明度表达式。
  const selectors = new Set(classes.filter((name) => /^[a-zA-Z_][\w/-]*$/.test(name))
    .map((name) => `.${name.replaceAll("/", "\\/")}`));
  const selected = new Map<string, string>();
  for (const rule of rules) {
    if (rule.property !== property) continue;
    for (const selector of rule.selector.split(",").map((item) => item.trim())) {
      if (selectors.has(selector)) selected.set(selector, rule.value);
    }
  }
  const values = new Set(selected.values());
  if (values.size !== 1) return null;
  const value = [...values][0]!;
  // 元素局部的 Tailwind shadow/ring 中间量不可直接挪到 Shot 上求值。
  return /var\(--[\w-]+/.test(value) && !value.includes("--tw-") ? value : null;
}

function tokenRules(document: Document): TokenRule[] {
  const result: TokenRule[] = [];
  function visit(rules: CSSRuleList): void {
    for (const rule of Array.from(rules)) {
      if (rule instanceof CSSStyleRule) {
        for (const property of Object.keys(DIALOG_PROPERTIES)) {
          const value = rule.style.getPropertyValue(property).trim();
          if (value) result.push({ selector: rule.selectorText, property, value });
        }
      } else if (rule instanceof CSSLayerBlockRule) {
        visit(rule.cssRules);
      } else if (rule instanceof CSSSupportsRule && CSS.supports(rule.conditionText)) {
        visit(rule.cssRules);
      }
      // 媒体和容器规则不参与基础推断；supports 仅在当前浏览器支持时展开。
    }
  }
  for (const sheet of Array.from(document.styleSheets)) {
    if (sheet.ownerNode instanceof Element && sheet.ownerNode.id === STYLE_ID) continue;
    try { visit(sheet.cssRules); } catch { /* 跨源样式不可读时保留兼容映射。 */ }
  }
  return result;
}

type SampleState = { element: Element | null; classes: string; sheetCount: number };
const states = new WeakMap<Document, SampleState>();

export function syncOfficialCaptureTokens(document: Document): void {
  const dialogs = Array.from(document.querySelectorAll('[role="dialog"].codex-dialog, [role="dialog"][aria-modal="true"]'))
    .filter((element) => !element.closest(OWN_UI));
  // Portal 卸载后保留同一份语义声明，主题变量继续由浏览器实时求值。
  if (dialogs.length === 0) return;
  if (dialogs.length !== 1) return;
  const element = dialogs[0]!;
  const classes = element.getAttribute("class") ?? "";
  const previous = states.get(document);
  if (previous?.element === element && previous.classes === classes && previous.sheetCount === document.styleSheets.length) return;
  const rules = tokenRules(document);
  const declarations = Object.entries(DIALOG_PROPERTIES).flatMap(([property, alias]) => {
    const value = resolveClassToken(classes.split(/\s+/), property, rules);
    return value ? [`--incodex-capture-${alias}:${value};`] : [];
  });
  let style = document.getElementById(STYLE_ID) as HTMLStyleElement | null;
  if (!style && declarations.length) {
    style = document.createElement("style");
    style.id = STYLE_ID;
    document.head.append(style);
  }
  if (style) {
    const text = `[data-incodex-capture][data-incodex-capture], [data-incodex-capture-preview][data-incodex-capture-preview]{${declarations.join("")}}`;
    if (style.textContent !== text) style.textContent = text;
  }
  states.set(document, { element, classes, sheetCount: document.styleSheets.length });
}
