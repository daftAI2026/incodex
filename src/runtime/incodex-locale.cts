// @ts-nocheck
/**
 * [INPUT]: 依赖调用方语言与可用文案目录。
 * [OUTPUT]: 提供规范语言键与左右排版方向。
 * [POS]: renderer 与原生引导共用语言策略；不决定系统权限名称。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
// Shared locale selection for renderer copy and the embedded native guide.
// 只有多个区域候选或跨语言别名需要显式默认；单一候选由下方扫描自然解析。
const LANGUAGE_DEFAULT_OVERRIDES = {
  es: "es-419",
  fr: "fr-FR",
  no: "nb-NO",
  pt: "pt-BR",
};

function resolveLocaleFromCatalog(raw, catalog) {
  const normalized = raw.trim().replaceAll("_", "-");
  if (!normalized) return "en";
  if (catalog[normalized]) return normalized;
  const lower = normalized.toLowerCase();
  const exact = Object.keys(catalog).find((key) => key.toLowerCase() === lower);
  if (exact) return exact;
  if (lower.startsWith("zh-hant-hk") || lower.startsWith("zh-hk")) return "zh-HK";
  if (lower.startsWith("zh-hant") || lower.startsWith("zh-tw")) return "zh-TW";
  if (lower.startsWith("zh")) return "zh-CN";
  if (lower === "en" || lower.startsWith("en-")) return "en";
  const language = lower.split("-")[0] ?? "en";
  if (catalog[language]) return language;
  const defaultOverride = LANGUAGE_DEFAULT_OVERRIDES[language];
  if (defaultOverride) {
    return defaultOverride;
  }
  const regional = Object.keys(catalog).find((key) => key.toLowerCase().startsWith(`${language}-`));
  return regional ?? "en";
}

function resolveLocaleDirection(raw, catalog) {
  const canonical = resolveLocaleFromCatalog(raw, catalog);
  const language = canonical.toLowerCase().split("-")[0] ?? "en";
  return ["ar", "fa", "ur"].includes(language) ? "rightToLeft" : "leftToRight";
}

export { resolveLocaleDirection, resolveLocaleFromCatalog };
