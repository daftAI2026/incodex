import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { blobatarUri } from "blobatar/uri";
import { profileMaskHealth, refreshProfileMaskHealth } from "./incognito-profile-mask.ts";

// 用真实 HTML selector 匹配实测结构；不读取账户文本、图片或 React props。
function withRail(run: (f: ReturnType<typeof fixture>) => void) {
  const keys = ["window", "document", "HTMLImageElement"];
  const previous = keys.map(key => Object.getOwnPropertyDescriptor(globalThis, key));
  const f = fixture();
  const values = [{
    __incodexIncognito: true,
    __incodexProfileMask: { name: "Temporary", avatar: { kind: "generated" } },
    __incodexProfileAvatarDecodeState: {
      dataUrl: blobatarUri("Temporary", { background: "circle" }), status: "ready", probe: null,
    },
  }, f.doc, f.Avatar];
  try {
    keys.forEach((key, i) => Object.defineProperty(globalThis, key, { configurable: true, value: values[i] }));
    run(f);
  } finally {
    keys.forEach((key, i) => {
      if (previous[i]) Object.defineProperty(globalThis, key, previous[i]!);
      else Reflect.deleteProperty(globalThis, key);
    });
  }
}

function fixture() {
  const nodes = new Map<string, Node>();
  let documentRoot: Node;
  const escape = (s: string) => s.replaceAll("&", "&amp;").replaceAll('"', "&quot;").replaceAll("<", "&lt;");
  class Node {
    key = String(nodes.size);
    textContent = "";
    style = {
      objectFit: "",
      get objectPosition() { return "center center"; },
      set objectPosition(_value: string) {},
    };
    constructor(public tag: string, public attrs: Record<string, string> = {}, public children: Node[] = []) {
      nodes.set(this.key, this);
    }
    setAttribute(key: string, value: string) { this.attrs[key] = value; }
    getAttribute(key: string) { return this.attrs[key] ?? null; }
    html(): string {
      const attrs = Object.entries({ ...this.attrs, "data-fixture-key": this.key })
        .map(([k, v]) => `${k}="${escape(v)}"`).join(" ");
      return `<${this.tag} ${attrs}>${this.children.map(n => n.html()).join("")}</${this.tag}>`;
    }
    querySelectorAll(selector: string): Node[] {
      const found: Node[] = [];
      new HTMLRewriter().on(selector.replaceAll(":scope", `[data-fixture-key="${this.key}"]`), {
        element(el) { found.push(nodes.get(el.getAttribute("data-fixture-key")!)!); },
      }).transform(this.html());
      return found;
    }
    querySelector(selector: string): Node | null { return this.querySelectorAll(selector)[0] ?? null; }
    matches(selector: string): boolean { return documentRoot.querySelectorAll(selector).includes(this); }
  }
  class Avatar extends Node {
    constructor(classes = "rounded-full size-6") { super("img", { class: classes, src: "official-avatar" }); }
    get src() { return this.attrs.src; }
    set src(value: string) { this.attrs.src = value; }
  }
  const node = (tag: string, attrs: Record<string, string> = {}, children: Node[] = []) => new Node(tag, attrs, children);
  const avatar = new Avatar();
  const footer = node("button", { type: "button", "aria-haspopup": "menu", "aria-expanded": "false" }, [
    node("span", {}, [node("span", { class: "relative isolate inline-flex shrink-0" }, [avatar])]),
  ]);
  const slot = node("div", { class: "sidebar-item" }, [node("div", {}, [footer])]);
  const rail = node("nav", { "data-app-navigation-rail": "" }, [slot]);
  documentRoot = node("body", {}, [rail]);
  const doc = {
    querySelectorAll: (selector: string) => documentRoot.querySelectorAll(selector),
    getElementById: (id: string) => [...nodes.values()].find(n => n.attrs.id === id) ?? null,
  };
  function openMenu() {
    const menuAvatar = new Avatar("icon-sm rounded-full");
    const name = node("span", { class: "min-w-0 truncate" }); name.textContent = "Official Name";
    const subtitle = node("span", { class: "text-xs text-tertiary truncate" }); subtitle.textContent = "Plan";
    const identity = node("div", { role: "menuitem" }, [node("div", {}, [
      node("span", {}, [node("span", {}, [menuAvatar])]),
      node("div", { class: "flex-1 min-w-0" }, [name, subtitle]),
    ])]);
    const menu = node("div", { role: "menu", id: "account-menu" }, [identity]);
    documentRoot.children.push(menu);
    footer.setAttribute("aria-controls", "account-menu"); footer.setAttribute("aria-expanded", "true");
    return { menu, name, subtitle, menuAvatar, identity };
  }
  return { doc, Avatar, avatar, footer, rail, slot, root: documentRoot, node, openMenu };
}

test("repairs the observed avatar-only account trigger inside the official rail", () => {
  withRail(f => {
    expect(profileMaskHealth()).toBe(false);
    expect(refreshProfileMaskHealth()).toBe(true);
    expect(f.avatar.src).toBe(blobatarUri("Temporary", { background: "circle" }));
    expect(f.avatar.getAttribute("data-incodex-profile-mask-avatar")).toBe("true");
    expect(f.footer.children).toHaveLength(1);
    expect(f.footer.getAttribute("aria-label")).toBeNull();
  });
});

test("masks only the identity in the rail trigger's linked account menu", () => {
  withRail(f => {
    const menu = f.openMenu();
    expect(refreshProfileMaskHealth()).toBe(true);
    expect(menu.name.textContent).toBe("Temporary");
    expect(menu.subtitle.textContent).toBe("Plan");
    expect(menu.menuAvatar.getAttribute("data-incodex-profile-mask-avatar")).toBe("true");
    menu.name.textContent = "Official Name";
    expect(profileMaskHealth()).toBe(false);
    expect(refreshProfileMaskHealth()).toBe(true);
  });
});

test("rejects missing or duplicate rail avatars and lookalikes outside the rail", () => {
  withRail(f => {
    f.rail.attrs = {};
    expect(refreshProfileMaskHealth()).toBe(false);
  });
  withRail(f => {
    f.footer.children = [];
    expect(refreshProfileMaskHealth()).toBe(false);
  });
  withRail(f => {
    f.slot.children.push(f.node("button", { type: "button", "aria-haspopup": "menu" }, [
      f.node("span", {}, [f.node("span", {}, [new f.Avatar()])]),
    ]));
    expect(refreshProfileMaskHealth()).toBe(false);
  });
});

test("rejects an expanded account trigger without its linked identity", () => {
  withRail(f => {
    f.footer.setAttribute("aria-expanded", "true");
    expect(refreshProfileMaskHealth()).toBe(false);
    const menu = f.openMenu();
    expect(refreshProfileMaskHealth()).toBe(true);
    menu.menu.children = [];
    expect(profileMaskHealth()).toBe(false);
  });
});

test("repeated injector bootstrap preserves the sidebar manager and activation handler", () => {
  const source = readFileSync(new URL("./inject.ts", import.meta.url), "utf8");
  const disposal = source.indexOf("window.__incodexSidebarEntry?.dispose();");
  const start = disposal >= 0 ? disposal : source.indexOf("const sidebarEntry =");
  const end = source.indexOf("function ensureSidebarEntry()", start);
  const bootstrap = new Bun.Transpiler({ loader: "ts" }).transformSync(source.slice(start, end));
  let created = 0, disposed = 0;
  const context = vm.createContext({
    window: {}, document: {}, tooltipState: {}, tooltipModules: { load() {} }, activate() {},
    createOfficialSidebarEntry() { created++; return { dispose() { disposed++; } }; },
  });
  const script = new vm.Script(`(()=>{${bootstrap}; return {manager:sidebarEntry,action:activateSidebar}})()`);
  const first = script.runInContext(context), second = script.runInContext(context);
  expect(created).toBe(1);
  expect(disposed).toBe(0);
  expect(second.manager).toBe(first.manager);
  expect(second.action).toBe(first.action);
});
