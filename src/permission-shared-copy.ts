/**
 * [INPUT]: 依赖辅助功能文案、官方宿主说明与独立的屏幕录制文案目录。
 * [OUTPUT]: 导出不改写输入目录的 65 语言原生权限文案合并器。
 * [POS]: 原生 CLI 权限文案的汇合点；保留 AX 原字段并附加 Screen Recording 行。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { ACCESSIBILITY_OFFICIAL_COPY } from "./runtime/incognito-accessibility-official-copy-data.ts";
import { SCREEN_RECORDING_COPY } from "./permission-screen-copy.ts";

type Copy = Record<string, string>;

function assertLocaleCoverage(source: string[], candidate: string[], label: string): void {
  if (JSON.stringify(source) !== JSON.stringify(candidate)) {
    throw new Error(`${label} must cover exactly the source locales`);
  }
}

// Preserve existing AX copy and append independent Screen Recording copy.
export function sharedPermissionCopy(catalog: Record<string, Copy>): Record<string, Copy> {
  const sourceLocales = Object.keys(catalog).sort();
  const officialLocales = Object.keys(ACCESSIBILITY_OFFICIAL_COPY).sort();
  const screenLocales = Object.keys(SCREEN_RECORDING_COPY).sort();
  assertLocaleCoverage(sourceLocales, officialLocales, "official Accessibility copy");
  assertLocaleCoverage(sourceLocales, screenLocales, "Screen Recording copy");
  return Object.fromEntries(Object.entries(catalog).map(([locale, copy]) => {
    const officialBody = ACCESSIBILITY_OFFICIAL_COPY[locale];
    if (!officialBody?.trim()) throw new Error(`missing official Accessibility reason for ${locale}`);
    return [locale, {
      ...copy,
      ...SCREEN_RECORDING_COPY[locale],
      officialBody,
      errorBody: copy.errorBody.replaceAll("incodex install", "incodex accessibility"),
    }];
  }));
}
