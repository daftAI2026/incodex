/**
 * [INPUT]: 依赖当前 packaged document 实际引用的 ESM 和官方 Rspack 已初始化缓存。
 * [OUTPUT]: 提供同一窗口的只读 exports 快照及 React 能力，不执行 require 或模块工厂。
 * [POS]: 官方模块布局适配边界；按能力选择入口，不固定脚本顺序、hash 或 module ID。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
type Namespace = Record<string, unknown>;
type Importer = (url: string) => Promise<Namespace>;
type Root = { render(element: unknown): void; unmount(): void };
type ElementFactory = (type: unknown, props: Record<string, unknown>) => unknown;
type RootFactory = (host: HTMLElement) => Root;
type RuntimeRequire = ((id: string) => unknown) & { c: Namespace };
export type OfficialRspackRuntime = { values: unknown[]; namespaces: Namespace[] };
const MAX_ENTRY_SCRIPTS = 16;
const MAX_CACHED_MODULES = 16_384;
// 大型 UI namespace 会重导出多个依赖；这是读取属性数量上限，不是组件数量或延时。
const MAX_CACHED_EXPORTS = 262_144;

function object(value: unknown): value is Namespace {
  return value !== null && (typeof value === "object" || typeof value === "function");
}

export async function loadOfficialRspackRuntime(
  doc: Document,
  importer: Importer = url => import(url),
): Promise<OfficialRspackRuntime | null> {
  const page = new URL(doc.URL);
  if (!["app:", "file:"].includes(page.protocol)) throw Error("Not a packaged renderer");
  const assets = new URL("./assets/", doc.URL).pathname;
  const scripts = [...new Set([...doc.querySelectorAll<HTMLScriptElement>('script[type="module"][src]')]
    .map(e => new URL(e.src, doc.URL))
    .filter(url => url.protocol === page.protocol && url.host === page.host && url.pathname.startsWith(assets) && /\.js$/u.test(url.pathname))
    .map(url => url.href))];
  // 原单入口加载器保留既有发现规则；不为旧版增加模块 import 或缓存遍历。
  if (scripts.length <= 1) return null;
  if (scripts.length > MAX_ENTRY_SCRIPTS) throw Error("Official renderer has too many entry scripts");
  const entries = await Promise.all(scripts.map(importer));
  const loaders = new Set(entries.map(e => e.__webpack_require__).filter((r): r is RuntimeRequire =>
    typeof r === "function" && object((r as unknown as Namespace).c)));
  if (loaders.size !== 1) throw Error("Official Rspack runtime is unavailable or ambiguous");
  const cache = [...loaders][0]!.c;
  const modules = Object.values(cache);
  if (modules.length > MAX_CACHED_MODULES) throw Error("Official module cache exceeds its bound");
  const values: unknown[] = [], namespaces: Namespace[] = [];
  const seen = new Set<unknown>();
  let inspected = 0;
  for (const module of modules) {
    if (!object(module) || module.error) continue;
    const namespace = module.exports;
    if (!object(namespace) || seen.has(namespace)) continue;
    seen.add(namespace); namespaces.push(namespace); values.push(namespace);
    for (const key of Object.getOwnPropertyNames(namespace)) {
      if (++inspected > MAX_CACHED_EXPORTS) throw Error("Official cached exports exceed their bound");
      try {
        const value = namespace[key];
        if (object(value)) values.push(value);
      } catch { /* 无关导出不可读不能遮蔽已加载能力。 */ }
    }
  }
  return { values: [...new Set(values)], namespaces };
}

export function discoverOfficialRspackReact(runtime: OfficialRspackRuntime): {
  createElement(type: unknown, props: Record<string, unknown>): unknown;
  createRoot(host: HTMLElement): Root;
} {
  const elements = new Set<ElementFactory>(), roots = new Set<RootFactory>();
  for (const value of runtime.values) {
    if (!object(value)) continue;
    // 读取公开 API，包括不可枚举属性；别名按函数身份去重，不调用未知导出。
    try {
      const keys = new Set(Object.getOwnPropertyNames(value));
      if (keys.has("createElement") && keys.has("useState") && typeof value.createElement === "function" && typeof value.useState === "function") elements.add(value.createElement as ElementFactory);
      if (keys.has("createRoot") && typeof value.createRoot === "function") roots.add(value.createRoot as RootFactory);
    } catch { /* 保留其他缓存 namespace 的能力。 */ }
  }
  if (elements.size !== 1 || roots.size !== 1) throw Error("Official cached React renderer is unavailable or ambiguous");
  const createElement = [...elements][0]!, createRoot = [...roots][0]!;
  return {
    createElement: (type, props) => Reflect.apply(createElement, undefined, [type, props]),
    createRoot: host => Reflect.apply(createRoot, undefined, [host]) as Root,
  };
}
