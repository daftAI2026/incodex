import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import vm from "node:vm";

type Listener = (...args: unknown[]) => unknown;

class FakeStyle {
  private readonly properties = new Map<string, string>();

  get cssText(): string {
    return [...this.properties]
      .map(([name, value]) => `${name}: ${value};`)
      .join(" ");
  }

  set cssText(value: string) {
    this.properties.clear();
    for (const declaration of value.split(";")) {
      const separator = declaration.indexOf(":");
      if (separator < 0) continue;
      this.setProperty(declaration.slice(0, separator).trim(), declaration.slice(separator + 1).trim());
    }
  }

  setProperty(name: string, value: string): void {
    this.properties.set(name, value);
  }

  getPropertyValue(name: string): string {
    return this.properties.get(name) ?? "";
  }

  removeProperty(name: string): void {
    this.properties.delete(name);
  }
}

class FakeElement {
  readonly attributesMap = new Map<string, string>();
  readonly children: FakeElement[] = [];
  readonly listeners = new Map<string, Listener[]>();
  readonly style = new FakeStyle();
  parentElement: FakeElement | null = null;
  private text = "";

  constructor(readonly tagName: string) {}

  get attributes(): Array<{ name: string; value: string }> {
    return [...this.attributesMap].map(([name, value]) => ({ name, value }));
  }

  get childNodes(): FakeElement[] {
    return this.children;
  }

  get firstElementChild(): FakeElement | null {
    return this.children[0] ?? null;
  }

  get firstChild(): FakeElement | null {
    return this.firstElementChild;
  }

  get nextElementSibling(): FakeElement | null {
    const siblings = this.parentElement?.children ?? [];
    const index = siblings.indexOf(this);
    return index >= 0 ? siblings[index + 1] ?? null : null;
  }

  get isConnected(): boolean {
    let current: FakeElement | null = this;
    while (current) {
      if (current.tagName === "document") return true;
      current = current.parentElement;
    }
    return false;
  }

  get className(): string {
    return this.getAttribute("class") ?? "";
  }

  set className(value: string) {
    this.setAttribute("class", value);
  }

  get textContent(): string {
    return this.text || this.children.map((child) => child.textContent).join("");
  }

  set textContent(value: string) {
    this.text = value;
    this.children.splice(0).forEach((child) => { child.parentElement = null; });
  }

  get innerHTML(): string {
    return this.children.map((child) => child.outerHTML()).join("");
  }

  set innerHTML(value: string) {
    this.children.splice(0).forEach((child) => { child.parentElement = null; });
    this.text = "";
    const root = parseSvg(value);
    if (root) this.append(root);
  }

  setAttribute(name: string, value: string): void {
    this.attributesMap.set(name, String(value));
    if (name === "style") this.style.cssText = String(value);
  }

  getAttribute(name: string): string | null {
    if (name === "style") return this.style.cssText || null;
    return this.attributesMap.get(name) ?? null;
  }

  hasAttribute(name: string): boolean {
    return this.attributesMap.has(name) || (name === "style" && this.style.cssText.length > 0);
  }

  removeAttribute(name: string): void {
    this.attributesMap.delete(name);
    if (name === "style") this.style.cssText = "";
  }

  append(...nodes: FakeElement[]): void {
    for (const node of nodes) {
      if (node.parentElement) node.removeFromParent();
      node.parentElement = this;
      this.children.push(node);
    }
  }

  insertBefore(node: FakeElement, before: FakeElement | null): void {
    if (node.parentElement) node.removeFromParent();
    node.parentElement = this;
    const index = before ? this.children.indexOf(before) : -1;
    if (index < 0) this.children.push(node);
    else this.children.splice(index, 0, node);
  }

  replaceWith(node: FakeElement): void {
    const parent = this.parentElement;
    if (!parent) return;
    const index = parent.children.indexOf(this);
    if (index < 0) return;
    this.parentElement = null;
    if (node.parentElement) node.removeFromParent();
    node.parentElement = parent;
    parent.children[index] = node;
  }

  remove(): void {
    this.removeFromParent();
  }

  cloneNode(deep = false): FakeElement {
    const clone = new FakeElement(this.tagName);
    for (const [name, value] of this.attributesMap) clone.setAttribute(name, value);
    clone.style.cssText = this.style.cssText;
    if (deep) {
      clone.text = this.text;
      clone.append(...this.children.map((child) => child.cloneNode(true)));
    }
    return clone;
  }

  addEventListener(type: string, listener: Listener): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  querySelector(selector: string): FakeElement | null {
    return this.querySelectorAll(selector)[0] ?? null;
  }

  querySelectorAll(selector: string): FakeElement[] {
    const matches: FakeElement[] = [];
    const visit = (element: FakeElement) => {
      for (const child of element.children) {
        if (matchesSelector(child, selector)) matches.push(child);
        visit(child);
      }
    };
    visit(this);
    return matches;
  }

  outerHTML(): string {
    const attrs = this.attributes.map(({ name, value }) => ` ${name}="${value}"`).join("");
    return `<${this.tagName}${attrs}>${this.innerHTML}</${this.tagName}>`;
  }

  private removeFromParent(): void {
    if (!this.parentElement) return;
    const parent = this.parentElement;
    const index = parent.children.indexOf(this);
    if (index >= 0) parent.children.splice(index, 1);
    this.parentElement = null;
  }
}

class FakeDocument extends FakeElement {
  readonly documentElement = new FakeElement("html");
  readonly head = new FakeElement("head");
  readonly body = new FakeElement("body");
  readonly registeredListeners = new Map<string, Listener[]>();
  readonly readyState = "loading";

  constructor() {
    super("document");
    this.append(this.documentElement);
    this.documentElement.append(this.head, this.body);
  }

  createElement(tagName: string): FakeElement {
    return new FakeElement(tagName.toLowerCase());
  }

  addEventListener(type: string, listener: Listener): void {
    this.registeredListeners.set(type, [...(this.registeredListeners.get(type) ?? []), listener]);
  }

  getElementById(id: string): FakeElement | null {
    return this.querySelector(`[id="${id}"]`);
  }
}

function matchesSelector(element: FakeElement, selector: string): boolean {
  const normalized = selector.trim();
  if (normalized === "*") return true;
  const tag = normalized.match(/^[a-zA-Z][a-zA-Z0-9-]*/)?.[0];
  if (tag && element.tagName !== tag.toLowerCase()) return false;
  for (const [, name, value] of normalized.matchAll(/\[([^=\]]+)(?:="([^"]*)")?\]/g)) {
    if (!element.hasAttribute(name)) return false;
    if (value !== undefined && element.getAttribute(name) !== value) return false;
  }
  for (const className of normalized.match(/\.([a-zA-Z0-9_-]+)/g) ?? []) {
    if (!element.className.split(/\s+/).includes(className.slice(1))) return false;
  }
  return true;
}

function parseSvg(source: string): FakeElement | null {
  const match = source.match(/<svg\b([^>]*)>/i);
  if (!match) return null;
  const svg = new FakeElement("svg");
  for (const [, name, value] of match[1].matchAll(/([:\w-]+)="([^"]*)"/g)) {
    svg.setAttribute(name, value);
  }
  return svg;
}

async function bundleInject(): Promise<string> {
  const runtimeDir = import.meta.dir;
  const assets = {
    hat: readFileSync(join(runtimeDir, "../../assets/hat-glasses.svg"), "utf8").trim(),
    exit: readFileSync(join(runtimeDir, "../../assets/circle-x.svg"), "utf8").trim(),
  };
  const source = readFileSync(join(runtimeDir, "inject.ts"), "utf8")
    .replaceAll("\r\n", "\n")
    .replace("const ICON_SVG = `{{HAT_GLASSES_SVG}}`;", `const ICON_SVG = ${JSON.stringify(assets.hat)};`)
    .replace("const EXIT_ICON_SVG = `{{CIRCLE_X_SVG}}`;", `const EXIT_ICON_SVG = ${JSON.stringify(assets.exit)};`);
  const result = await Bun.build({
    entrypoints: ["inject-icon-layout-entry.ts"],
    plugins: [{
      name: "virtual-inject-icon-layout-entry",
      setup(build) {
        build.onResolve({ filter: /^inject-icon-layout-entry\.ts$/ }, () => ({
          path: "inject-icon-layout-entry.ts",
          namespace: "incodex-test",
        }));
        build.onResolve({ filter: /^\./ }, (args) => ({
          path: resolve(runtimeDir, args.path),
        }));
        build.onLoad({ filter: /.*/, namespace: "incodex-test" }, () => ({
          contents: `${source}\nglobalThis.__incodexLayoutExports = { buildButton, setButtonHover };`,
          loader: "ts",
          resolveDir: runtimeDir,
        }));
      },
    }],
    target: "bun",
    format: "iife",
    minify: false,
  });
  if (!result.success) throw new Error(result.logs.map((log) => log.message).join("\n"));
  return result.outputs[0]!.text();
}

const INJECT_BUNDLE = await bundleInject();

function makeRuntime(): {
  buildButton: (search: FakeElement) => FakeElement;
  setButtonHover: (button: FakeElement, hovered: boolean) => void;
} {
  const document = new FakeDocument();
  const window = {
    __incodexIncognito: true,
    __incodexPlatform: "darwin",
    addEventListener: () => {},
    setTimeout: () => 1,
    clearTimeout: () => {},
  } as Record<string, unknown>;
  const navigator = { language: "en-US" };
  const context = vm.createContext({
    document,
    window,
    navigator,
    TextEncoder,
    TextDecoder,
    crypto,
    MutationObserver: class {},
    requestAnimationFrame: () => {},
    globalThis: undefined,
  });
  context.globalThis = context;
  new vm.Script(INJECT_BUNDLE, { filename: "inject.ts" }).runInContext(context);
  return (context as typeof context & {
    __incodexLayoutExports: {
      buildButton: (search: FakeElement) => FakeElement;
      setButtonHover: (button: FakeElement, hovered: boolean) => void;
    };
  }).__incodexLayoutExports;
}

function makeSearch(document: FakeDocument, nested: boolean): { search: FakeElement; parent: FakeElement; officialTooltipListener: Listener } {
  const search = document.createElement("button");
  search.className = "flex size-8 items-center justify-center text-secondary";
  search.setAttribute("aria-label", "Search");
  search.setAttribute("id", "official-search");
  search.setAttribute("title", "Search official tooltip");
  search.setAttribute("aria-describedby", "official-search-tooltip");
  search.setAttribute("data-testid", "official-search-button");
  search.textContent = "Search text that must not be cloned";

  const icon = document.createElement("svg");
  icon.className = nested ? "absolute inset-0 size-full" : "icon-xs";
  icon.style.setProperty("stroke-width", "1.5");
  icon.setAttribute("width", nested ? "100%" : "16");
  icon.setAttribute("height", nested ? "100%" : "16");

  if (nested) {
    const outer = document.createElement("div");
    outer.className = "flex size-full items-center justify-center";
    outer.style.setProperty("--icon-outer-scale", "var(--icon-leading-size)");
    outer.setAttribute("id", "official-icon-outer");
    outer.setAttribute("role", "presentation");
    outer.setAttribute("data-testid", "official-icon-outer");
    const wrapper = document.createElement("span");
    wrapper.className = "relative shrink-0 icon-leading";
    wrapper.style.setProperty("--icon-leading-size", "var(--size-icon-leading)");
    wrapper.setAttribute("id", "official-icon-wrapper");
    wrapper.setAttribute("role", "img");
    wrapper.setAttribute("title", "official icon tooltip");
    wrapper.setAttribute("data-testid", "official-icon-wrapper");
    wrapper.append(icon);
    outer.append(wrapper);
    search.append(outer);
  } else {
    search.append(icon);
  }

  const label = document.createElement("span");
  label.className = "official-label-child";
  label.textContent = "official text child";
  search.append(label);
  const parent = document.createElement("div");
  const sibling = document.createElement("aside");
  sibling.setAttribute("data-sibling", "must-not-clone");
  parent.append(search, sibling);
  const officialTooltipListener = () => {};
  search.addEventListener("pointerenter", officialTooltipListener);
  document.body.append(parent);
  return { search, parent, officialTooltipListener };
}

function iconWrapper(button: FakeElement): FakeElement | null {
  return layoutWrappers(button).at(-1) ?? null;
}

function layoutWrappers(button: FakeElement): FakeElement[] {
  const wrappers: FakeElement[] = [];
  let current = button.firstElementChild;
  while (current && current.tagName !== "svg") {
    wrappers.push(current);
    current = current.firstElementChild;
  }
  return current?.tagName === "svg" ? wrappers : [];
}

describe("8881 hat-glasses icon layout", () => {
  test("copies the Search icon's non-interactive layout wrapper and CSS variable", () => {
    const document = new FakeDocument();
    const { buildButton } = makeRuntime();
    const { search } = makeSearch(document, true);
    const button = buildButton(search);
    const wrappers = layoutWrappers(button);
    const outer = wrappers[0];
    const wrapper = wrappers[1];

    expect(wrappers).toHaveLength(2);
    expect(outer).not.toBeNull();
    expect(outer?.style.getPropertyValue("--icon-outer-scale")).toBe("var(--icon-leading-size)");
    expect(wrapper?.className).toContain("relative shrink-0 icon-leading");
    expect(wrapper?.style.getPropertyValue("--icon-leading-size")).toBe("var(--size-icon-leading)");
    for (const layout of wrappers) {
      expect(layout.getAttribute("aria-hidden")).toBe("true");
      expect(layout.getAttribute("id")).toBeNull();
      expect(layout.getAttribute("role")).toBeNull();
      expect(layout.getAttribute("title")).toBeNull();
      expect(layout.getAttribute("data-testid")).toBeNull();
    }

    const svg = wrapper?.querySelector("svg[data-incodex-icon]");
    expect(svg?.className).toBe("absolute inset-0 size-full");
    expect(svg?.style.getPropertyValue("stroke-width")).toBe("1.5");
    expect(svg?.getAttribute("width")).toBe("100%");
    expect(svg?.getAttribute("height")).toBe("100%");
    expect(svg?.getAttribute("width")).not.toBe("16");
  });

  test("keeps legacy direct-SVG Search markup without inventing a fixed-size wrapper", () => {
    const document = new FakeDocument();
    const { buildButton } = makeRuntime();
    const { search } = makeSearch(document, false);
    const button = buildButton(search);

    expect(button.children).toHaveLength(1);
    expect(button.children[0]?.tagName).toBe("svg");
    expect(button.children[0]?.className).toBe("icon-xs");
    expect(button.children[0]?.getAttribute("width")).toBe("16");
    expect(iconWrapper(button)).toBeNull();
  });

  test("does not clone Search text, siblings, identity attributes, or tooltip listeners", () => {
    const document = new FakeDocument();
    const { buildButton } = makeRuntime();
    const { search, parent, officialTooltipListener } = makeSearch(document, true);
    const button = buildButton(search);

    expect(button.textContent).toBe("");
    expect(button.querySelector(".official-label-child")).toBeNull();
    expect(parent.children).toHaveLength(2);
    expect(parent.children[1]?.getAttribute("data-sibling")).toBe("must-not-clone");
    expect(button.getAttribute("id")).toBeNull();
    expect(button.getAttribute("title")).toBeNull();
    expect(button.getAttribute("aria-describedby")).toBeNull();
    expect(button.getAttribute("data-testid")).toBeNull();
    expect(button.listeners.get("pointerenter") ?? []).not.toContain(officialTooltipListener);
  });

  test("replaces only the SVG on hover and retains the copied wrapper", () => {
    const document = new FakeDocument();
    const runtime = makeRuntime();
    const { search } = makeSearch(document, true);
    const button = runtime.buildButton(search);
    const wrappers = layoutWrappers(button);
    const outer = wrappers[0];
    const wrapper = wrappers[1];
    const variable = wrapper?.style.getPropertyValue("--icon-leading-size");

    runtime.setButtonHover(button, true);
    expect(layoutWrappers(button)[0]).toBe(outer);
    expect(layoutWrappers(button)[1]).toBe(wrapper);
    expect(wrapper?.style.getPropertyValue("--icon-leading-size")).toBe(variable);
    expect(wrapper?.className).toContain("icon-leading");
    expect(wrapper?.querySelector('svg[data-incodex-icon="circle-x"]')).not.toBeNull();

    runtime.setButtonHover(button, false);
    expect(layoutWrappers(button)[0]).toBe(outer);
    expect(layoutWrappers(button)[1]).toBe(wrapper);
    expect(wrapper?.querySelector('svg[data-incodex-icon="hat-glasses"]')).not.toBeNull();
    expect(wrapper?.getAttribute("aria-hidden")).toBe("true");
  });
});
