/**
 * [INPUT]: 依赖地区 AX 原文、显式语义 runs 与语言解析器。
 * [OUTPUT]: 导出 65 语言原始权限目录；小卡统一文案由共享投影生成。
 * [POS]: AX 原文目录与译文证据；不从翻译句子推断系统名占位边界。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { ACCESSIBILITY_REGIONAL_COPY } from "./incognito-accessibility-copy-data.ts";
import { attachAccessibilityDragInstructionRuns } from "./incognito-accessibility-copy-runs.ts";
import { resolveLocaleFromCatalog } from "./incodex-locale.cts";
import { COPY as REGIONAL_COPY, type CopyKey, type CopyTable } from "./incognito-copy-data.ts";

export type { CopyKey, CopyTable } from "./incognito-copy-data.ts";

// Embedded into the main-process Runtime by build-runtime.ts. Keep the two
// source languages together without introducing an unverified Runtime asset.
const ACCESSIBILITY_SETUP_COPY_BASE = /* @__PURE__ */ (() => ({
  ...ACCESSIBILITY_REGIONAL_COPY,
  en: {
    title: "Set up ChatGPT permissions",
    installedTitle: "Set up ChatGPT permissions again",
    body: "After installing Incodex, confirm these permissions again to use apps on your Mac.",
    permissionTitle: "Accessibility",
    permissionDescription: "Read and interact with app interfaces",
    repair: "Allow",
    later: "Skip",
    back: "Back",
    addedTitle: "Allow ChatGPT in System Settings",
    addedBody: "Drag the ChatGPT icon above into the Accessibility list and enable it.",
    dragInstruction: "Drag ChatGPT to the list above to allow Accessibility",
    completeInSettings: "COMPLETE IN SYSTEM SETTINGS",
    checking: "Waiting for access · checking automatically",
    repairing: "Preparing System Settings…",
    openSettings: "Open Settings",
    errorTitle: "ChatGPT permission setup is incomplete",
    errorBody: "The permission setup could not be completed. Add /Applications/ChatGPT.app in System Settings → Privacy & Security → Accessibility, or run incodex accessibility to reopen the permission guide.",
  },
  "zh-CN": {
    title: "设置 ChatGPT 权限",
    installedTitle: "重新设置 ChatGPT 权限",
    body: "安装 Incodex 后，需要重新确认这些权限才能使用 Mac 上的应用。",
    permissionTitle: "无障碍",
    permissionDescription: "读取和操作其他应用的界面",
    repair: "允许",
    later: "跳过",
    back: "返回",
    addedTitle: "在系统设置中允许 ChatGPT",
    addedBody: "将上方 ChatGPT 图标拖入“无障碍”列表，并开启权限。",
    dragInstruction: "将 ChatGPT 拖到上方的“无障碍”列表中，然后开启对应权限",
    completeInSettings: "在系统设置中完成",
    checking: "等待授权 · 自动检查中",
    repairing: "正在准备系统设置…",
    openSettings: "打开系统设置",
    errorTitle: "ChatGPT 权限设置尚未完成",
    errorBody: "未能完成权限设置。请在系统设置 → 隐私与安全性 → 无障碍中添加 /Applications/ChatGPT.app，或运行 incodex accessibility 重新打开授权卡。",
  },
  "zh-HK": {
    title: "設定 ChatGPT 權限",
    installedTitle: "重新設定 ChatGPT 權限",
    body: "安裝 Incodex 後，需要重新確認這些權限才能使用 Mac 上的應用程式。",
    permissionTitle: "輔助使用",
    permissionDescription: "讀取並操作其他應用程式的介面",
    repair: "允許",
    later: "略過",
    back: "返回",
    addedTitle: "在系統設定中允許 ChatGPT",
    addedBody: "將上方 ChatGPT 圖示拖入「輔助使用」列表，並啟用權限。",
    dragInstruction: "將 ChatGPT 拖到上方的「輔助使用」列表中，然後啟用權限",
    completeInSettings: "在系統設定中完成",
    checking: "等待授權 · 自動檢查中",
    repairing: "正在準備系統設定…",
    openSettings: "開啟系統設定",
    errorTitle: "ChatGPT 權限設定尚未完成",
    errorBody: "未能完成權限設定。請在系統設定 → 私隱與安全性 → 輔助使用中加入 /Applications/ChatGPT.app，或執行 incodex accessibility 重新開啟授權卡。",
  },
  "zh-TW": {
    title: "設定 ChatGPT 權限",
    installedTitle: "重新設定 ChatGPT 權限",
    body: "安裝 Incodex 後，需要重新確認這些權限才能使用 Mac 上的 App。",
    permissionTitle: "輔助使用",
    permissionDescription: "讀取並操作其他應用程式的介面",
    repair: "允許",
    later: "略過",
    back: "返回",
    addedTitle: "在系統設定中允許 ChatGPT",
    addedBody: "將上方的 ChatGPT 圖示拖曳到「輔助使用」列表，並啟用權限。",
    dragInstruction: "將 ChatGPT 拖曳到上方的「輔助使用」列表中，然後啟用權限",
    completeInSettings: "在系統設定中完成",
    checking: "等待授權 · 自動檢查中",
    repairing: "正在準備系統設定…",
    openSettings: "開啟系統設定",
    errorTitle: "ChatGPT 權限設定尚未完成",
    errorBody: "未能完成權限設定。請在系統設定 → 隱私權與安全性 → 輔助使用中加入 /Applications/ChatGPT.app，或執行 incodex accessibility 重新開啟授權卡。",
  },
} as const))();

export const ACCESSIBILITY_SETUP_COPY = /* @__PURE__ */ attachAccessibilityDragInstructionRuns(
  ACCESSIBILITY_SETUP_COPY_BASE,
);

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

export function resolveLocale(raw: string): string {
  return resolveLocaleFromCatalog(raw, COPY);
}

export function translate(locale: string, key: CopyKey): string {
  const resolved = resolveLocale(locale);
  if (key === "body") return CORE_COPY[resolved]?.body ?? CORE_COPY.en.body;
  return COPY[resolved]?.[key] ?? COPY.en[key];
}
