/**
 * [INPUT]: 依赖官方 Search 按钮内 SVG 的布局祖先，以及待挂载的独立图案。
 * [OUTPUT]: 提供 cloneButtonIconLayout，保留官方 class、CSS 变量引用与定位作用域。
 * [POS]: 注入器的图标布局适配层；只复制布局壳，不复制 Search 的身份、文字或交互。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

export function cloneButtonIconLayout(
  icon: SVGElement,
  sample: SVGElement | null,
  search: HTMLElement,
): Element {
  let root: Element = icon;
  // -- 布局壳与图案分离：size-full 必须保留自己的定位、尺寸和 token 祖先。 --
  for (let parent = sample?.parentElement; parent && parent !== search; parent = parent.parentElement) {
    const shell = parent.cloneNode(false) as HTMLElement;
    for (const { name } of [...shell.attributes]) {
      if (name !== "class" && name !== "style") shell.removeAttribute(name);
    }
    shell.setAttribute("aria-hidden", "true");
    shell.append(root);
    root = shell;
  }
  return root;
}
