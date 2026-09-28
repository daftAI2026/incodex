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
