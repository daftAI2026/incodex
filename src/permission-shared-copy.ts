/**
 * [INPUT]: 依赖 65 语言 AX 原文、官方宿主说明与显式小卡动作模板。
 * [OUTPUT]: 导出统一 AX/录屏小卡文案、系统权限名模板与语义 runs。
 * [POS]: CLI 与兼容 Runtime 的文案投影；原始 AX 翻译保持只读。
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

// 保留 AX 原文作为翻译证据，只在生产投影中统一小卡动作。
export function sharedPermissionCopy(catalog: Record<string, Copy>): Record<string, Copy> {
  const sourceLocales = Object.keys(catalog).sort();
  const officialLocales = Object.keys(ACCESSIBILITY_OFFICIAL_COPY).sort();
  const screenLocales = Object.keys(SCREEN_RECORDING_COPY).sort();
  assertLocaleCoverage(sourceLocales, officialLocales, "official Accessibility copy");
  assertLocaleCoverage(sourceLocales, screenLocales, "Screen Recording copy");
  return Object.fromEntries(Object.entries(catalog).map(([locale, copy]) => {
    const officialBody = ACCESSIBILITY_OFFICIAL_COPY[locale];
    if (!officialBody?.trim()) throw new Error(`missing official Accessibility reason for ${locale}`);
    const screen = SCREEN_RECORDING_COPY[locale];
    const permissionTitle = copy.permissionTitle;
    const accessibilityInstruction = screen.permissionListInstruction.replaceAll(
      "{permissionTitle}",
      permissionTitle,
    );
    const accessibilityRuns = JSON.parse(screen.permissionListInstructionRuns) as Array<{ text: string; role: string }>;
    return [locale, {
      ...copy,
      ...screen,
      permissionLocale: locale,
      dragInstruction: accessibilityInstruction,
      dragInstructionRuns: JSON.stringify(accessibilityRuns.map(run => ({
        ...run,
        text: run.text.replaceAll("{permissionTitle}", permissionTitle),
      }))),
      officialBody,
      errorBody: copy.errorBody.replaceAll("incodex install", "incodex accessibility"),
    }];
  }));
}
