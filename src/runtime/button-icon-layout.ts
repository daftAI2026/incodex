/**
 * [INPUT]: 官方 Search 的 SVG 祖先布局与新的注入图标
 * [OUTPUT]: 只保留 class/style 的图标布局克隆
 * [POS]: 注入按钮的布局适配器，让相机与帽子共享官方尺寸约束
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
export function cloneButtonIconLayout(
  icon: SVGElement,
  sample: SVGElement | null,
  search: HTMLElement,
  styleAttributes: ReadonlySet<string> = new Set(),
): Element {
  let root: Element = icon;
  // Keep the positioning and token-sized ancestors that constrain a size-full SVG.
  for (let parent = sample?.parentElement; parent && parent !== search; parent = parent.parentElement) {
    const shell = parent.cloneNode(false) as HTMLElement;
    for (const { name } of [...shell.attributes]) {
      if (name !== "class" && name !== "style" && !styleAttributes.has(name)) shell.removeAttribute(name);
    }
    shell.setAttribute("aria-hidden", "true");
    shell.append(root);
    root = shell;
  }
  return root;
}
