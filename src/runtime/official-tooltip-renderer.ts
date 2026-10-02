import type { TooltipLifecycle } from "./tooltip-lifecycle.ts";
import { isSearchLabel } from "./compatibility/search-labels.ts";
import { parseOfficialWindowZoom } from "./tooltip-presentation.ts";

type SharedTooltipState = {
  lifecycle: TooltipLifecycle | null;
  renderer: ReturnType<typeof createOfficialTooltipRenderer> | null;
  moduleLoader?: ReturnType<typeof createOfficialTooltipModuleLoader> | null;
};

export function sharedTooltipState(
  scope: { __incodexTooltipState?: SharedTooltipState },
  acquireLoader?: () => ReturnType<typeof createOfficialTooltipModuleLoader>,
): SharedTooltipState {
  const state = scope.__incodexTooltipState ??= { lifecycle: null, renderer: null };
  if (acquireLoader) state.moduleLoader ??= acquireLoader();
  return state;
}

type ModulePaths = { react: string; client: string; tooltip: string };
type Root = { render: (element: unknown) => void; unmount: () => void };
type RendererModules = {
  createElement: (type: unknown, props: Record<string, unknown>) => unknown;
  createRoot: (host: HTMLElement) => Root;
  Tooltip: unknown;
};

type ReactFiber = {
  return?: ReactFiber | null;
  type?: unknown;
  elementType?: unknown;
  memoizedProps?: unknown;
  pendingProps?: unknown;
};
type JsxFactoryFunction = (type: unknown, props: Record<string, unknown>) => unknown;

type NamedImport = { imported: string; local: string };

// Official releases consolidate vendor/UI code into larger shared chunks.
// This byte/character guard is not a UI timer or a fixed asset-size assumption.
export const SHARED_MODULE_SOURCE_BUDGET = 16_000_000;

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}

function namedImportsFrom(source: string, importer: string, dependency: string): NamedImport[] {
  const imports: NamedImport[] = [];
  const pattern = /\bimport\s*\{([^}]+)\}\s*from\s*["'`]([^"'`]+)["'`]/gu;
  for (const match of source.matchAll(pattern)) {
    if (new URL(match[2]!, importer).href !== dependency) continue;
    for (const binding of match[1]!.split(",")) {
      const parts = binding.trim().split(/\s+as\s+/u);
      const imported = parts[0]?.trim();
      const local = (parts[1] ?? parts[0])?.trim();
      if (imported && local && /^[A-Za-z_$][\w$]*$/u.test(imported) &&
          /^[A-Za-z_$][\w$]*$/u.test(local)) imports.push({ imported, local });
    }
  }
  return imports;
}

// The official entry's consumer reveals which current export creates its root.
// Import aliases and local temporary names are parsed from this generation's
// source and never stored as product constants.
export function discoverCreateRootFactoryExport(
  importer: string,
  source: string,
  dependency: string,
): string {
  const candidates = new Set<string>();
  for (const binding of namedImportsFrom(source, importer, dependency)) {
    const local = escapeRegExp(binding.local);
    const assignments = new RegExp(`\\b([A-Za-z_$][\\w$]*)\\s*=\\s*${local}\\s*\\(\\)`, "gu");
    for (const match of source.matchAll(assignments)) {
      const receiver = escapeRegExp(match[1]!);
      if (new RegExp(`\\b${receiver}\\s*\\.\\s*createRoot\\b`, "u").test(source)) {
        candidates.add(binding.imported);
      }
    }
    if (new RegExp(`\\b${local}\\s*\\(\\)\\s*\\.\\s*createRoot\\b`, "u").test(source)) {
      candidates.add(binding.imported);
    }
  }
  if (candidates.size !== 1) throw new Error("Official React root factory is unavailable or ambiguous");
  return [...candidates][0]!;
}

export function discoverCalledFactoryExports(importer: string, source: string, dependency: string): string[] {
  return namedImportsFrom(source, importer, dependency)
    .filter(({ local }) => {
      const escaped = escapeRegExp(local);
      return new RegExp(`\\b${escaped}\\s*\\(\\s*\\)`, "u").test(source);
    })
    .map(({ imported }) => imported);
}

export function discoverOfficialJsxFactoryExport(
  tooltip: unknown,
  sharedSource: string,
  importer: string,
  importerSource: string,
  dependency: string,
): string {
  if (typeof tooltip !== "function") throw new Error("Official Tooltip implementation is unavailable");
  const componentSource = Function.prototype.toString.call(tooltip);
  const receivers = new Set(discoverOfficialTooltipJsxReceivers(componentSource));
  if (receivers.size !== 1) throw new Error("Official Tooltip JSX runtime use is unavailable or ambiguous");

  const componentOffset = sharedSource.indexOf(componentSource);
  if (componentOffset < 0 || sharedSource.indexOf(componentSource, componentOffset + 1) >= 0) {
    throw new Error("Official Tooltip source is unavailable or ambiguous");
  }
  const receiver = escapeRegExp([...receivers][0]!);
  const initializers = new Set([...sharedSource.matchAll(new RegExp(
    `\\b${receiver}\\s*=\\s*(?:\\(\\s*0\\s*,\\s*)?([A-Za-z_$][\\w$]*)\\s*\\)?\\s*\\(\\s*\\)`, "gu",
  ))].map((match) => match[1]!));
  if (initializers.size !== 1) throw new Error("Official Tooltip JSX getter is unavailable or ambiguous");
  const localFactory = [...initializers][0]!;

  const exportClause = /\bexport\s*\{([^}]*)\}\s*;?\s*(?:\/\/[#@]\s*sourceMappingURL=[^\r\n]*)?\s*$/u.exec(sharedSource);
  if (!exportClause) throw new Error("Official shared export map is unavailable");
  const exports = new Set<string>();
  for (const binding of exportClause[1]!.split(",")) {
    const parts = binding.trim().split(/\s+as\s+/u);
    if (parts[0] === localFactory) exports.add(parts[1] ?? parts[0]!);
  }
  if (exports.size !== 1) throw new Error("Official Tooltip JSX getter export is unavailable or ambiguous");
  const exportName = [...exports][0]!;
  if (!discoverCalledFactoryExports(importer, importerSource, dependency).includes(exportName)) {
    throw new Error("Official app root consumer does not prewarm the Tooltip JSX runtime");
  }
  return exportName;
}

export function discoverOfficialTooltipJsxReceivers(componentSource: string): string[] {
  return [...new Set([...componentSource.matchAll(
    /(?:\(\s*0\s*,\s*)?\b([A-Za-z_$][\w$]*)\s*\.\s*(?:jsx|jsxs)\s*\)?\s*\(/gu,
  )].map((match) => match[1]!))];
}

export function discoverOfficialJsxRuntime(
  namespace: Record<string, unknown>,
  factoryExport: string,
): { Fragment: unknown; jsx: JsxFactoryFunction; jsxs: JsxFactoryFunction } {
  const factory = namespace[factoryExport];
  if (typeof factory !== "function") throw new Error("Official JSX runtime getter is unavailable");
  const value = Reflect.apply(factory, undefined, []) as Record<string, unknown> | null;
  if (typeof value?.jsx !== "function" || typeof value.jsxs !== "function" || !Object.hasOwn(value, "Fragment")) {
    throw new Error("Official JSX runtime capabilities are unavailable");
  }
  return value as { Fragment: unknown; jsx: JsxFactoryFunction; jsxs: JsxFactoryFunction };
}

function reactFiber(element: HTMLElement): ReactFiber | null {
  const key = Object.keys(element).find((name) => name.startsWith("__reactFiber$"));
  return key ? (element as unknown as Record<string, ReactFiber | undefined>)[key] ?? null : null;
}

function officialSearchTooltipPositioning(
  search: HTMLElement | null,
  tooltipType: unknown,
  doc: Document,
): Record<string, unknown> {
  if (!search) return {};
  let props: Record<string, unknown> | null = null;
  for (let fiber = reactFiber(search), depth = 0; fiber && depth < 64; fiber = fiber.return ?? null, depth += 1) {
    if (fiber.type !== tooltipType && fiber.elementType !== tooltipType) continue;
    const candidate = fiber.memoizedProps ?? fiber.pendingProps;
    if (typeof candidate !== "object" || candidate === null || !Object.hasOwn(candidate, "tooltipContent")) continue;
    if (props) return {};
    props = candidate as Record<string, unknown>;
  }
  if (!props) return {};

  const positioning: Record<string, unknown> = {};
  const side = props.side ?? doc.defaultView?.getComputedStyle(search).getPropertyValue("--side-tooltip").trim();
  if (["top", "bottom", "left", "right"].includes(String(side))) positioning.side = side;
  const sideOffset = props.sideOffset;
  if (typeof sideOffset === "number" && Number.isFinite(sideOffset)) positioning.sideOffset = sideOffset;
  const align = props.align;
  if (["start", "center", "end"].includes(String(align))) positioning.align = align;
  const alignOffset = props.alignOffset;
  if (typeof alignOffset === "number" && Number.isFinite(alignOffset)) positioning.alignOffset = alignOffset;
  return positioning;
}

function officialSearchZoomProvider(search: HTMLElement | null, doc: Document): { type: unknown; value: number } | null {
  if (!search) return null;
  const zoom = parseOfficialWindowZoom(
    doc.defaultView?.getComputedStyle(search).getPropertyValue("--codex-window-zoom") ?? "",
  );
  if (zoom === 1) return null;

  const candidates: Array<{ type: unknown; value: number }> = [];
  const visited = new Set<ReactFiber>();
  for (let fiber = reactFiber(search); fiber && !visited.has(fiber); fiber = fiber.return ?? null) {
    visited.add(fiber);
    const value = (fiber.memoizedProps as { value?: unknown } | undefined)?.value;
    const context = fiber.type as { Provider?: unknown; _currentValue?: unknown } | null;
    if (value !== zoom || !context || context._currentValue !== 1 || !context.Provider) continue;
    candidates.push({ type: context.Provider, value: zoom });
  }
  return candidates.length === 1 ? candidates[0]! : null;
}

export function findOfficialTooltipComponent(
  trigger: HTMLElement,
  namespace: Record<string, unknown>,
): unknown {
  const exports = new Set(Object.values(namespace));
  const candidates = new Set<unknown>();
  let fiber = reactFiber(trigger);
  for (let depth = 0; fiber && depth < 64; depth += 1, fiber = fiber.return ?? null) {
    const type = fiber.elementType ?? fiber.type;
    const props = fiber.memoizedProps ?? fiber.pendingProps;
    if (exports.has(type) && typeof props === "object" && props !== null &&
        Object.hasOwn(props, "tooltipContent")) candidates.add(type);
  }
  if (candidates.size !== 1) throw new Error("Official Tooltip component is unavailable or ambiguous");
  return [...candidates][0];
}

// Read the entry's dependency graph instead of assuming every release keeps
// React and Tooltip in separately named chunks. Rolldown/Vite may merge them.
export function discoverOfficialTooltipModuleGraph(entry: string, source: string): string[] {
  const entryUrl = new URL(entry);
  if (!["app:", "file:"].includes(entryUrl.protocol)) throw new Error("Not a packaged renderer");
  const assetsDirectory = new URL("./", entryUrl);
  const modules = new Set<string>();
  const specifierPattern = /["'`](\.\/[^"'`]+\.js)["'`]/g;
  for (const match of source.matchAll(specifierPattern)) {
    const specifier = match[1]!;
    if (!/^\.\/[A-Za-z0-9_-][A-Za-z0-9._-]*\.js$/u.test(specifier)) continue;
    const resolved = new URL(specifier, entryUrl);
    if (resolved.protocol !== entryUrl.protocol || resolved.host !== entryUrl.host ||
        !resolved.pathname.startsWith(assetsDirectory.pathname)) {
      throw new Error("Official module graph escapes the packaged assets directory");
    }
    modules.add(resolved.href);
    if (modules.size > 256) throw new Error("Official module graph is unexpectedly large");
  }
  return [...modules];
}

type SourceToken = { kind: "identifier" | "punctuation" | "string"; value: string; escaped?: boolean; template?: boolean };
const REGEX_PREFIX_KEYWORDS = new Set([
  "return", "throw", "case", "delete", "void", "typeof", "instanceof", "in", "of", "yield", "await", "else", "do",
]);

function regexMayStartAfter(token: SourceToken | undefined): boolean {
  if (!token) return true;
  if (token.kind === "identifier") return REGEX_PREFIX_KEYWORDS.has(token.value);
  return "({[=,:;!&|?+-*%^~<>".includes(token.value);
}

function sourceTokens(source: string): SourceToken[] {
  const tokens: SourceToken[] = [];
  let index = 0;
  while (index < source.length) {
    const char = source[index]!;
    if (/\s/u.test(char)) { index += 1; continue; }
    if (char === "/" && source[index + 1] === "/") {
      index = source.indexOf("\n", index + 2);
      if (index < 0) break;
      continue;
    }
    if (char === "/" && source[index + 1] === "*") {
      const end = source.indexOf("*/", index + 2);
      if (end < 0) throw new Error("Malformed official module source");
      index = end + 2;
      continue;
    }
    if (char === "/" && regexMayStartAfter(tokens.at(-1))) {
      let end = index + 1;
      let escaped = false;
      let inCharacterClass = false;
      for (; end < source.length; end += 1) {
        const next = source[end]!;
        if (escaped) { escaped = false; continue; }
        if (next === "\\") { escaped = true; continue; }
        if (next === "[" && !inCharacterClass) { inCharacterClass = true; continue; }
        if (next === "]" && inCharacterClass) { inCharacterClass = false; continue; }
        if (next === "/" && !inCharacterClass) break;
        if (next === "\n" || next === "\r") break;
      }
      if (source[end] === "/") {
        index = end + 1;
        while (index < source.length && /[A-Za-z]/u.test(source[index]!)) index += 1;
        continue;
      }
    }
    if (char === "'" || char === '"') {
      const quote = char;
      let end = index + 1;
      let escaped = false;
      let hasEscape = false;
      for (; end < source.length; end += 1) {
        if (escaped) { escaped = false; continue; }
        if (source[end] === "\\") { escaped = true; hasEscape = true; continue; }
        if (source[end] === quote) break;
      }
      if (end >= source.length) throw new Error("Malformed official module source");
      tokens.push({ kind: "string", value: source.slice(index + 1, end), escaped: hasEscape });
      index = end + 1;
      continue;
    }
    if (char === "`") {
      let end = index + 1;
      let escaped = false;
      let hasInterpolation = false;
      let hasEscape = false;
      for (; end < source.length; end += 1) {
        if (escaped) { escaped = false; continue; }
        if (source[end] === "\\") { escaped = true; hasEscape = true; continue; }
        if (source[end] === "$" && source[end + 1] === "{") hasInterpolation = true;
        if (source[end] === "`") break;
      }
      if (end >= source.length) throw new Error("Malformed official module source");
      if (!hasInterpolation) {
        tokens.push({ kind: "string", value: source.slice(index + 1, end), escaped: hasEscape, template: true });
      }
      index = end + 1;
      continue;
    }
    if (/[A-Za-z_$]/u.test(char)) {
      let end = index + 1;
      while (end < source.length && /[A-Za-z0-9_$]/u.test(source[end]!)) end += 1;
      tokens.push({ kind: "identifier", value: source.slice(index, end) });
      index = end;
      continue;
    }
    tokens.push({ kind: "punctuation", value: char });
    index += 1;
  }
  return tokens;
}

function staticModuleSpecifiers(source: string): string[] {
  const tokens = sourceTokens(source);
  const specifiers: string[] = [];
  for (let index = 0; index < tokens.length; index += 1) {
    const keyword = tokens[index]!;
    if (keyword.kind !== "identifier" || (keyword.value !== "import" && keyword.value !== "export")) continue;
    const next = tokens[index + 1];
    if (!next) continue;
    if (keyword.value === "export" && next.value !== "{" && next.value !== "*") continue;
    if (keyword.value === "import" && next.kind === "string") {
      if (next.escaped) throw new Error("Escaped official module specifier is unsupported");
      specifiers.push(next.value);
      index += 1;
      continue;
    }
    if (keyword.value === "import" && next.value === "(") continue;
    if (keyword.value === "import" && next.value === ".") continue;

    let braceDepth = 0;
    for (let cursor = index + 1; cursor < tokens.length; cursor += 1) {
      const token = tokens[cursor]!;
      if (token.kind === "punctuation") {
        if (token.value === "{") braceDepth += 1;
        else if (token.value === "}") braceDepth = Math.max(0, braceDepth - 1);
        else if (token.value === ";" && braceDepth === 0) break;
      }
      if (braceDepth !== 0 || token.kind !== "identifier" || token.value !== "from") continue;
      const specifier = tokens[cursor + 1];
      if (specifier?.kind !== "string") throw new Error("Malformed official module dependency");
      if (specifier.escaped) throw new Error("Escaped official module specifier is unsupported");
      specifiers.push(specifier.value);
      index = cursor + 1;
      break;
    }
  }
  return specifiers;
}

function literalDynamicModuleSpecifiers(source: string): string[] {
  const tokens = sourceTokens(source);
  const specifiers: string[] = [];
  for (let index = 0; index < tokens.length; index += 1) {
    if (tokens[index]?.kind !== "identifier" || tokens[index]?.value !== "import" || tokens[index + 1]?.value !== "(") continue;
    const specifier = tokens[index + 2];
    if (specifier?.kind !== "string" || tokens[index + 3]?.value !== ")") continue;
    if (specifier.escaped) throw new Error("Escaped official dynamic module specifier is unsupported");
    specifiers.push(specifier.value);
  }
  return specifiers;
}

// Unlike the preload map above, only actual ESM import declarations are
// executable dependency edges. This avoids loading lazy chunks merely because
// their URLs appear in an entry's preload table. The returned URLs are direct
// dependencies only; callers may inspect their source and continue traversal
// when the active runtime needs a transitive module.
export function discoverOfficialStaticModuleGraph(entry: string, source: string): string[] {
  const entryUrl = new URL(entry);
  if (!["app:", "file:"].includes(entryUrl.protocol)) throw new Error("Not a packaged renderer");
  const assetsDirectory = new URL("./", entryUrl);
  const specifiers = new Set<string>();
  for (const specifier of staticModuleSpecifiers(source)) specifiers.add(specifier);

  const modules = new Set<string>();
  for (const specifier of specifiers) {
    if (!specifier.startsWith(".")) continue;
    if (!/^\.\/[A-Za-z0-9_-][A-Za-z0-9._-]*\.js$/u.test(specifier)) {
      throw new Error("Official static dependency is not a direct packaged asset");
    }
    const resolved = new URL(specifier, entryUrl);
    if (resolved.protocol !== entryUrl.protocol || resolved.host !== entryUrl.host ||
        !resolved.pathname.startsWith(assetsDirectory.pathname)) {
      throw new Error("Official static dependency escapes the packaged assets directory");
    }
    modules.add(resolved.href);
    if (modules.size > 256) throw new Error("Official static dependency graph is unexpectedly large");
  }
  return [...modules];
}

export function discoverOfficialDynamicModuleGraph(entry: string, source: string): string[] {
  const entryUrl = new URL(entry);
  if (!["app:", "file:"].includes(entryUrl.protocol)) throw new Error("Not a packaged renderer");
  const assetsDirectory = new URL("./", entryUrl);
  const modules = new Set(discoverOfficialStaticModuleGraph(entry, source));
  for (const specifier of literalDynamicModuleSpecifiers(source)) {
    if (!specifier.startsWith(".")) continue;
    if (!/^\.\/[A-Za-z0-9_-][A-Za-z0-9._-]*\.js$/u.test(specifier)) {
      throw new Error("Official dynamic dependency is not a direct packaged asset");
    }
    const resolved = new URL(specifier, entryUrl);
    if (resolved.protocol !== entryUrl.protocol || resolved.host !== entryUrl.host ||
        !resolved.pathname.startsWith(assetsDirectory.pathname)) {
      throw new Error("Official dynamic dependency escapes the packaged assets directory");
    }
    modules.add(resolved.href);
    if (modules.size > 256) throw new Error("Official dynamic dependency graph is unexpectedly large");
  }
  return [...modules];
}

// Read the current packaged entry's dependency map, not fixed asset hashes or
// copied CSS. Only direct siblings in that entry's assets directory may load.
export function discoverOfficialTooltipModules(entry: string, source: string): ModulePaths {
  const graph = discoverOfficialTooltipModuleGraph(entry, source);
  const result = {} as ModulePaths;
  for (const name of ["react", "client", "tooltip"] as const) {
    const moduleName = new RegExp(`^${name}-[A-Za-z0-9_]+\\.js$`, "u");
    const paths = graph.filter((url) => moduleName.test(new URL(url).pathname.split("/").at(-1) ?? ""));
    if (paths.length !== 1) throw new Error(`Official ${name} module is unavailable or ambiguous`);
    result[name] = paths[0]!;
  }
  return result;
}

export function assertOfficialModuleSourceSize(source: string, maxCharacters: number): void {
  if (source.length > maxCharacters) throw new Error("Unexpected official renderer module size");
}

export type OfficialModuleSourceReader = (url: string, maxCharacters?: number) => Promise<string>;

export function createOfficialModuleSourceReader(read: OfficialModuleSourceReader = readOfficialModuleSource): OfficialModuleSourceReader {
  // 只合并同一预算的在途读取；请求结束即释放，不保留源码缓存。
  const pending = new Map<string, Promise<string>>();
  return (url, maxCharacters) => {
    const key = JSON.stringify([url, maxCharacters ?? null]);
    const existing = pending.get(key);
    if (existing) return existing;
    const reading = read(url, maxCharacters);
    pending.set(key, reading);
    const release = () => { if (pending.get(key) === reading) pending.delete(key); };
    void reading.then(release, release);
    return reading;
  };
}

export async function readOfficialModuleSource(url: string, maxCharacters = 2_000_000): Promise<string> {
  const response = await fetch(url, { signal: AbortSignal.timeout(5000), redirect: "error" });
  if (!response.ok) throw new Error("Cannot read official renderer module");
  const declaredBytes = Number(response.headers.get("content-length"));
  if (Number.isFinite(declaredBytes) && declaredBytes > maxCharacters) {
    throw new Error("Unexpected official renderer module size");
  }
  if (!response.body) {
    const source = await response.text();
    assertOfficialModuleSourceSize(source, maxCharacters);
    return source;
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const chunks: string[] = [];
  let totalBytes = 0;
  while (true) {
    const chunk = await reader.read();
    if (chunk.done) break;
    totalBytes += chunk.value.byteLength;
    if (totalBytes > maxCharacters) {
      await reader.cancel();
      throw new Error("Unexpected official renderer module size");
    }
    chunks.push(decoder.decode(chunk.value, { stream: true }));
  }
  chunks.push(decoder.decode());
  const source = chunks.join("");
  assertOfficialModuleSourceSize(source, maxCharacters);
  return source;
}

export function findOfficialSearchButton(doc: Document): HTMLElement {
  const matches = [...doc.querySelectorAll<HTMLElement>("button")]
    .filter((button) => isSearchLabel(button.getAttribute("aria-label")));
  if (matches.length !== 1) throw new Error("Official Search trigger is unavailable or ambiguous");
  return matches[0]!;
}

async function readOfficialTooltipConsumers(
  entry: string,
  entrySource: string,
  staticPaths: string[],
  readSource: OfficialModuleSourceReader,
): Promise<Array<{ url: string; source: string }>> {
  const paths = discoverOfficialDynamicModuleGraph(entry, entrySource).filter((url) => !staticPaths.includes(url));
  if (paths.length > 16) throw new Error("Official renderer has too many direct dynamic dependencies");
  const sources = await Promise.all(paths.map(async (url) => {
    try {
      return { url, source: await readSource(url, 512_000) };
    } catch {
      // 无关的大型 lazy chunk 保留原有预算拒绝，不作为 root consumer。
      return null;
    }
  }));
  return sources.filter((source) => source !== null);
}

async function loadSharedOfficialTooltipModules(
  doc: Document,
  entry: string,
  entrySource: string,
  readSource: OfficialModuleSourceReader,
  dependencies: PreparedTooltipEntry["dependencies"],
): Promise<RendererModules> {
  const { directModules, prefetchedSource, consumerSources } = await dependencies;
  const search = findOfficialSearchButton(doc);
  const matches: Array<{ url: string; namespace: Record<string, unknown>; Tooltip: unknown }> = [];
  for (const module of directModules) {
    try {
      const Tooltip = findOfficialTooltipComponent(search, module.namespace);
      matches.push({ url: module.url, namespace: module.namespace, Tooltip });
    } catch {
      // Most entry dependencies are runtime/loader helpers, not the component owner.
    }
  }
  if (matches.length !== 1) throw new Error("Official shared Tooltip module is unavailable or ambiguous");
  const { url: sharedModulePath, namespace, Tooltip } = matches[0]!;
  const earlySource = prefetchedSource?.url === sharedModulePath ? await prefetchedSource.reading : null;
  const sharedSource = earlySource ?? await readSource(sharedModulePath, SHARED_MODULE_SOURCE_BUDGET);

  const consumers: Array<{ url: string; source: string; rootFactoryExport: string }> = [];
  for (const candidate of await consumerSources) {
    try {
      const rootFactoryExport = discoverCreateRootFactoryExport(candidate.url, candidate.source, sharedModulePath);
      consumers.push({ ...candidate, rootFactoryExport });
    } catch {
      // Only the active root consumer imports a factory and uses its result.createRoot.
    }
  }
  if (consumers.length !== 1) throw new Error("Official React root consumer is unavailable or ambiguous");
  const consumer = consumers[0]!;
  const rootFactory = namespace[consumer.rootFactoryExport];
  if (typeof rootFactory !== "function") throw new Error("Official React root factory export is unavailable");
  const rootModule = Reflect.apply(rootFactory, undefined, []) as { createRoot?: unknown } | null;
  if (typeof rootModule?.createRoot !== "function") throw new Error("Official React root API is unavailable");

  const jsxFactoryExport = discoverOfficialJsxFactoryExport(
    Tooltip, sharedSource, consumer.url, consumer.source, sharedModulePath,
  );
  const jsxRuntime = discoverOfficialJsxRuntime(namespace, jsxFactoryExport);
  const element = Reflect.apply(jsxRuntime.jsx, undefined, [Tooltip, {}]);
  if (typeof element !== "object" || element === null || (element as { type?: unknown }).type !== Tooltip) {
    throw new Error("Official JSX factory did not create an element for the Search Tooltip");
  }
  const createElement = (type: unknown, props: Record<string, unknown>) =>
    Reflect.apply(jsxRuntime.jsx, undefined, [type, props]);
  const createRoot = (host: HTMLElement) => Reflect.apply(
    rootModule.createRoot as (...args: unknown[]) => Root,
    undefined,
    [host],
  );
  return {
    createElement,
    createRoot,
    Tooltip,
  };
}

type PreparedTooltipEntry = {
  entry: string;
  source: string;
  dependencies: Promise<{
    staticPaths: string[];
    directModules: Array<{ url: string; namespace: Record<string, unknown> }>;
    prefetchedSource?: { url: string; reading: Promise<string | null> };
    consumerSources: Promise<Array<{ url: string; source: string }>>;
  }>;
};

async function prepareOfficialTooltipEntry(
  doc: Document,
  readSource: OfficialModuleSourceReader,
  importModule: (url: string) => Promise<Record<string, unknown>> = (url) => import(url),
): Promise<PreparedTooltipEntry> {
  const page = new URL(doc.URL);
  if (!["app:", "file:"].includes(page.protocol)) throw new Error("Not a packaged renderer");
  const entries = [...doc.querySelectorAll<HTMLScriptElement>('script[type="module"][src]')]
    .map((script) => new URL(script.src, doc.URL))
    .filter((url) => url.protocol === page.protocol && url.host === page.host &&
      url.pathname.startsWith(new URL("./assets/", doc.URL).pathname) && /\.js$/.test(url.pathname));
  if (entries.length !== 1) throw new Error("Official renderer entry is unavailable or ambiguous");
  const entry = entries[0]!.href;
  const source = await readSource(entry);
  const dependencies = (async () => {
    const staticPaths = discoverOfficialStaticModuleGraph(entry, source);
    const consumerSources = readOfficialTooltipConsumers(entry, source, staticPaths, readSource);
    void consumerSources.catch(() => {});
    const directModules = await Promise.all(staticPaths.map(async (url) => ({ url, namespace: await importModule(url) })));
    // 能力线索仅提前读取，不决定组件归属；Search fiber 仍是最终证明。
    const hints = directModules.filter(({ namespace }) => Object.values(namespace).some((value) => {
      if (typeof value !== "function") return false;
      try {
        return /\btooltipContent\b/u.test(Function.prototype.toString.call(value));
      } catch {
        // 官方 callable proxy 可能拒绝源码读取；辅助预读不能阻断真实组件归属验证。
        return false;
      }
    }));
    const prefetchedSource = hints.length === 1 ? {
      url: hints[0]!.url,
      reading: readSource(hints[0]!.url, SHARED_MODULE_SOURCE_BUDGET).catch(() => null),
    } : undefined;
    return { staticPaths, directModules, prefetchedSource, consumerSources };
  })();
  // 提前启动静态依赖；失败仍交给原有旧分块回退，不产生未处理拒绝。
  void dependencies.catch(() => {});
  return { entry, source, dependencies };
}

export async function loadOfficialTooltipModules(
  doc: Document,
  readSource: OfficialModuleSourceReader = readOfficialModuleSource,
  prepared?: PreparedTooltipEntry,
): Promise<RendererModules> {
  const { entry, source, dependencies } = prepared ?? await prepareOfficialTooltipEntry(doc, readSource);
  try {
    return await loadSharedOfficialTooltipModules(doc, entry, source, readSource, dependencies);
  } catch (sharedError) {
    try {
      const paths = discoverOfficialTooltipModules(entry, source);
      const [reactModule, clientModule, tooltipModule] = await Promise.all([
        import(paths.react), import(paths.client), import(paths.tooltip),
      ]);
      // Preserve the original split-chunk loader for supported older builds.
      if (typeof reactModule.t !== "function" || typeof clientModule.t !== "function" ||
          typeof tooltipModule.r !== "function" || typeof tooltipModule.t !== "function") {
        throw new Error("Unsupported official Tooltip exports");
      }
      const react = reactModule.t();
      const client = clientModule.t();
      if (typeof react?.createElement !== "function" || typeof client?.createRoot !== "function") {
        throw new Error("Unsupported official React renderer");
      }
      tooltipModule.r();
      return {
        createElement: (type, props) => Reflect.apply(react.createElement, undefined, [type, props]),
        createRoot: (host) => Reflect.apply(client.createRoot, undefined, [host]),
        Tooltip: tooltipModule.t,
      };
    } catch {
      throw sharedError;
    }
  }
}

const TOOLTIP_ID = "incodex-official-tooltip";

export function createOfficialTooltipModuleLoader(
  doc: Document,
  readSource: OfficialModuleSourceReader = readOfficialModuleSource,
  importModule: (url: string) => Promise<Record<string, unknown>> = (url) => import(url),
) {
  let pending: Promise<PreparedTooltipEntry> | null = null;
  const start = () => {
    if (pending) return pending;
    const reading = prepareOfficialTooltipEntry(doc, readSource, importModule);
    pending = reading;
    void reading.catch(() => { if (pending === reading) pending = null; });
    return reading;
  };
  return {
    async prepare(): Promise<void> {
      const entry = await start();
      await entry.dependencies.then(() => {}, () => {});
    },
    async load(): Promise<RendererModules> {
      const reading = start();
      try {
        return await loadOfficialTooltipModules(doc, readSource, await reading);
      } finally {
        // 一次启动准备结束即释放源码快照；后续重挂载重新读当前入口。
        if (pending === reading) pending = null;
      }
    },
  };
}

export function createOfficialTooltipRenderer(
  doc: Document,
  load: () => Promise<RendererModules> = () => loadOfficialTooltipModules(doc),
) {
  let modules: RendererModules | null = null;
  let root: Root | null = null;
  let host: HTMLElement | null = null;
  let pending: Promise<void> | null = null;
  let disposed = false;
  let button: HTMLElement | null = null;

  function hide() {
    if (button) {
      const ids = (button.getAttribute("aria-describedby") ?? "").split(/\s+/)
        .filter((id) => id && id !== TOOLTIP_ID);
      if (ids.length) button.setAttribute("aria-describedby", ids.join(" "));
      else button.removeAttribute("aria-describedby");
      button = null;
      root?.render(null);
    }
  }

  return {
    ready: () => !disposed && root !== null && host?.isConnected !== false,
    needsRemount: () => root !== null && host?.isConnected === false,
    needsPreparation: () => !disposed && root === null && pending === null,
    async preparedModules(): Promise<RendererModules> {
      if (disposed) throw new Error("Official renderer is disposed");
      await this.prepare();
      if (disposed || !modules) throw new Error("Official renderer is disposed or unavailable");
      return modules;
    },
    prepare(): Promise<void> {
      if (pending) return pending;
      pending = load().then((loaded) => {
        if (disposed) return;
        modules = loaded;
        host = doc.createElement("div");
        host.setAttribute("data-incodex-official-tooltip-root", "true");
        doc.body.append(host);
        root = modules.createRoot(host);
      }).catch((error) => {
        host?.remove();
        host = null;
        modules = null;
        pending = null;
        throw error;
      });
      return pending;
    },
    show(target: HTMLElement, label: string, shortcut: string, search: HTMLElement | null = null) {
      if (disposed || !root || !modules || !target.isConnected) return;
      hide();
      button = target;
      target.removeAttribute("title");
      const ids = new Set((target.getAttribute("aria-describedby") ?? "").split(/\s+/).filter(Boolean));
      ids.add(TOOLTIP_ID);
      target.setAttribute("aria-describedby", [...ids].join(" "));
      const tooltip = modules.createElement(modules.Tooltip, {
        open: true, disableHoverOpen: true, tooltipId: TOOLTIP_ID,
        tooltipContent: label, shortcut, positioningElement: target,
        ...officialSearchTooltipPositioning(search, modules.Tooltip, doc),
        // Input/dismissal stays in our existing official-provider timing bridge.
        // The official component owns all tooltip DOM, styling and positioning.
        children: modules.createElement("span", { "aria-hidden": true }),
      });
      const zoomProvider = officialSearchZoomProvider(search, doc);
      root.render(zoomProvider
        ? modules.createElement(zoomProvider.type, { value: zoomProvider.value, children: tooltip })
        : tooltip);
    },
    hide,
    dispose() {
      if (disposed) return;
      hide();
      disposed = true;
      root?.unmount();
      host?.remove();
      root = null;
      host = null;
    },
  };
}
