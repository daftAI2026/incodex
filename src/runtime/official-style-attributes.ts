// Style keys come from loaded official CSS, not a list of current Button props.
// Identity and transient input state remain explicit non-inheritable boundaries.
function isStyleAttribute(name: string): boolean {
  return !/^data-(?:incodex-|test|react|radix|tracking|analytics|telemetry)/.test(name) &&
    !/(?:^|-)(?:state|status|loading|selected|disabled|inert|open|closed|expanded|pressed|checked|focused|hovered|active|busy|pending|id|key|slot|component|part)(?:-|$)/.test(name);
}

type RuleContainer = { cssRules?: CSSRuleList; styleSheet?: CSSStyleSheet | null };

export function officialStyleAttributes(document: Pick<Document, "styleSheets"> & Partial<Pick<Document, "adoptedStyleSheets">>): Set<string> {
  const names = new Set<string>();
  const visited = new Set<object>();
  const visit = (container: RuleContainer): void => {
    if (visited.has(container)) return;
    visited.add(container);
    let rules: CSSRuleList | undefined;
    try { rules = container.cssRules; } catch { return; } // Cross-origin CSS remains unread.
    for (const rule of Array.from(rules ?? [])) {
      const selector = (rule as CSSStyleRule).selectorText;
      if (typeof selector === "string") {
        for (const match of selector.matchAll(/\[\s*(data-[a-zA-Z0-9_-]+)(?=[\s\]=~|^$*])/g)) {
          const name = match[1]!.toLowerCase();
          if (isStyleAttribute(name)) names.add(name);
        }
      }
      visit(rule as RuleContainer);
      const imported = (rule as CSSImportRule).styleSheet;
      if (imported) visit(imported);
    }
  };
  for (const sheet of [...Array.from(document.styleSheets ?? []), ...Array.from(document.adoptedStyleSheets ?? [])]) visit(sheet);
  return names;
}
