import { COPY as REGIONAL_COPY, type CopyKey, type CopyTable } from "./incognito-copy-data.ts";

export type { CopyKey, CopyTable } from "./incognito-copy-data.ts";

// Embedded into the main-process Runtime by build-runtime.ts. Keep the two
// source languages together without introducing an unverified Runtime asset.
export const ACCESSIBILITY_SETUP_COPY = {
  en: {
    title: "Finish ChatGPT script-control setup",
    body: "ChatGPT cannot currently control other apps through scripts. Repair clears only ChatGPT’s old Accessibility registration and opens System Settings. Add the selected ChatGPT app and enable its access. macOS may ask for your password or Touch ID.",
    repair: "Repair and open Settings",
    later: "Later",
    addedTitle: "Allow ChatGPT in System Settings",
    addedBody: "Drag the ChatGPT app selected in Finder into the Accessibility list and enable it. Complete any macOS authentication, then return to ChatGPT to check access. Access is not ready until that check succeeds.",
    checkAgain: "Check access",
    openSettings: "Open Settings",
    errorTitle: "ChatGPT permission setup is incomplete",
    errorBody: "The permission setup could not be completed. Add /Applications/ChatGPT.app in System Settings → Privacy & Security → Accessibility, then run incodex install to check again.",
  },
  "zh-CN": {
    title: "完成 ChatGPT 脚本控制设置",
    body: "ChatGPT 当前无法通过脚本控制其他应用。修复会仅清除 ChatGPT 的旧辅助功能登记，并打开系统设置。请添加选中的 ChatGPT 应用并开启权限。macOS 可能要求密码或 Touch ID。",
    repair: "重新登记并打开设置",
    later: "稍后",
    addedTitle: "在系统设置中允许 ChatGPT",
    addedBody: "将 Finder 中选中的 ChatGPT 拖入辅助功能列表并开启权限。完成 macOS 要求的认证后，回到 ChatGPT 检查。只有检查通过，脚本控制权限才算设置完成。",
    checkAgain: "检查权限",
    openSettings: "打开系统设置",
    errorTitle: "ChatGPT 权限设置尚未完成",
    errorBody: "未能完成权限设置。请在系统设置 → 隐私与安全性 → 辅助功能中添加 /Applications/ChatGPT.app，再运行 incodex install 重新检查。",
  },
} as const;

// English and Chinese are the source copy; keep them beside locale resolution.
const CORE_COPY: Record<string, CopyTable> = {
  en: {
    open: "Open incognito window",
    exit: "Exit incognito window",
    title: "Incognito window",
    body: "Same account and settings as usual, without earlier chats. This conversation will not show up in your everyday chat list. Temporary data is removed after a normal exit.",
    dismiss: "Dismiss incognito banner",
    errorTitle: 'Couldn’t open the incognito window',
    errorBody: 'Try again. If it still fails, quit Codex and open it again.',
    errorRetry: 'Try again',
    errorClose: 'Close',
  },
  "zh-CN": {
    open: "打开私密窗口",
    exit: "退出私密窗口",
    title: "私密窗口",
    body: "账号和设置跟平时一样，看不到以前的对话，这次的聊天也不会进平时的列表。正常关掉后，这次的临时数据会清掉。",
    dismiss: "关闭私密窗口横幅",
    errorTitle: '无法打开私密窗口',
    errorBody: '再试一次。如果还是不行，先退出 Codex 再打开。',
    errorRetry: '再试一次',
    errorClose: '关闭',
  },
  "zh-HK": {
    open: "開啟私密視窗",
    exit: "離開私密視窗",
    title: "私密視窗",
    body: "帳戶和設定跟平時一樣，看不到以前的對話，這次的聊天也不會進平時的列表。正常關掉後，這次的臨時資料會清掉。",
    dismiss: "關閉私密視窗橫額",
    errorTitle: '無法開啟私密視窗',
    errorBody: '再試一次。如果仍然不行，先退出 Codex 再開。',
    errorRetry: '再試一次',
    errorClose: '關閉',
  },
  "zh-TW": {
    open: "開啟私密視窗",
    exit: "離開私密視窗",
    title: "私密視窗",
    body: "帳號和設定跟平時一樣，看不到以前的對話，這次的聊天也不會進平時的列表。正常關掉後，這次的臨時資料會清掉。",
    dismiss: "關閉私密視窗橫幅",
    errorTitle: '無法開啟私密視窗',
    errorBody: '再試一次。如果還是不行，先退出 Codex 再開啟。',
    errorRetry: '再試一次',
    errorClose: '關閉',
  },
};

export const COPY: Record<string, CopyTable> = {
  ...REGIONAL_COPY,
  ...CORE_COPY,
};

// 只有多个区域候选或跨语言别名需要显式默认；单一候选由下方扫描自然解析。
const LANGUAGE_DEFAULT_OVERRIDES: Record<string, string> = {
  es: "es-419",
  fr: "fr-FR",
  no: "nb-NO",
  pt: "pt-BR",
};

export function resolveLocale(raw: string): string {
  const normalized = raw.trim().replaceAll("_", "-");
  if (!normalized) return "en";
  if (COPY[normalized]) return normalized;
  const lower = normalized.toLowerCase();
  const exact = Object.keys(COPY).find((key) => key.toLowerCase() === lower);
  if (exact) return exact;
  if (lower.startsWith("zh-hant-hk") || lower.startsWith("zh-hk")) return "zh-HK";
  if (lower.startsWith("zh-hant") || lower.startsWith("zh-tw")) return "zh-TW";
  if (lower.startsWith("zh")) return "zh-CN";
  if (lower === "en" || lower.startsWith("en-")) return "en";
  const language = lower.split("-")[0] ?? "en";
  if (COPY[language]) return language;
  const defaultOverride = LANGUAGE_DEFAULT_OVERRIDES[language];
  if (defaultOverride) {
    return defaultOverride;
  }
  const regional = Object.keys(COPY).find((key) => key.toLowerCase().startsWith(`${language}-`));
  return regional ?? "en";
}

export function translate(locale: string, key: CopyKey): string {
  const resolved = resolveLocale(locale);
  if (key === "body") return CORE_COPY[resolved]?.body ?? CORE_COPY.en.body;
  return COPY[resolved]?.[key] ?? COPY.en[key];
}
