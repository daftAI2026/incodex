/**
 * [INPUT]: 依赖辅助功能文案、官方宿主说明与独立的屏幕录制文案目录。
 * [OUTPUT]: 导出 65 语言文案、显式语言与系统页名模板；保留输入。
 * [POS]: CLI 与兼容 Runtime 共用目录；原生呈现层选择系统权限名。
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
    const screen = SCREEN_RECORDING_COPY[locale];
    // 两项权限复用“加入当前 App”动作。只把明确的页名变成占位符。
    // 不从 AX 自然语句推断词界，避免破坏带屈折变化的翻译。
    const listTemplate = (text: string) => text.replaceAll(screen.screenTitle, "{permissionTitle}");
    const listRuns = JSON.parse(screen.screenDragInstructionRuns) as Array<{ text: string; role: string }>;
    return [locale, {
      ...copy,
      ...screen,
      permissionLocale: locale,
      permissionListInstruction: listTemplate(screen.screenDragInstruction),
      permissionListInstructionRuns: JSON.stringify(listRuns.map(run => ({ ...run, text: listTemplate(run.text) }))),
      officialBody,
      errorBody: copy.errorBody.replaceAll("incodex install", "incodex accessibility"),
    }];
  }));
}
