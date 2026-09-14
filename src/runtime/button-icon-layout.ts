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
