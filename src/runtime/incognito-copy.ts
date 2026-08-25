import { COPY as REGIONAL_COPY, type CopyKey, type CopyTable } from "./incognito-copy-data.ts";

export type { CopyKey, CopyTable } from "./incognito-copy-data.ts";

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
    open: "打开无痕窗口",
    exit: "退出无痕窗口",
    title: "无痕窗口",
    body: "账号和设置跟平时一样，看不到以前的对话，这次的聊天也不会进平时的列表。正常关掉后，这次的临时数据会清掉。",
    dismiss: "关闭无痕窗口横幅",
    errorTitle: '无法打开无痕窗口',
    errorBody: '再试一次。如果还是不行，先退出 Codex 再打开。',
    errorRetry: '再试一次',
    errorClose: '关闭',
  },
  "zh-HK": {
    open: "開啟無痕視窗",
    exit: "離開無痕視窗",
    title: "無痕視窗",
    body: "帳戶和設定跟平時一樣，看不到以前的對話，這次的聊天也不會進平時的列表。正常關掉後，這次的臨時資料會清掉。",
    dismiss: "關閉無痕視窗橫額",
    errorTitle: '無法開啟無痕視窗',
    errorBody: '再試一次。如果仍然不行，先退出 Codex 再開。',
    errorRetry: '再試一次',
    errorClose: '關閉',
  },
  "zh-TW": {
    open: "開啟無痕視窗",
    exit: "離開無痕視窗",
    title: "無痕視窗",
    body: "帳號和設定跟平時一樣，看不到以前的對話，這次的聊天也不會進平時的列表。正常關掉後，這次的臨時資料會清掉。",
    dismiss: "關閉無痕視窗橫幅",
    errorTitle: '無法開啟無痕視窗',
    errorBody: '再試一次。如果還是不行，先退出 Codex 再開啟。',
    errorRetry: '再試一次',
    errorClose: '關閉',
  },
};

export const COPY: Record<string, CopyTable> = {
  ...REGIONAL_COPY,
  ...CORE_COPY,
};

const DEFAULT_BY_LANGUAGE: Record<string, string> = {
  bg: "bg-BG",
  bn: "bn-BD",
  bs: "bs-BA",
  ca: "ca-ES",
  cs: "cs-CZ",
  da: "da-DK",
  de: "de-DE",
  el: "el-GR",
  es: "es-419",
  et: "et-EE",
  fi: "fi-FI",
  fr: "fr-FR",
  gu: "gu-IN",
  hi: "hi-IN",
  hr: "hr-HR",
  hu: "hu-HU",
  hy: "hy-AM",
  id: "id-ID",
  is: "is-IS",
  it: "it-IT",
  ja: "ja-JP",
  ka: "ka-GE",
  kn: "kn-IN",
  ko: "ko-KR",
  lv: "lv-LV",
  mk: "mk-MK",
  mr: "mr-IN",
  ms: "ms-MY",
  my: "my-MM",
  nb: "nb-NO",
  no: "nb-NO",
  nl: "nl-NL",
  pl: "pl-PL",
  pt: "pt-BR",
  ro: "ro-RO",
  ru: "ru-RU",
  sk: "sk-SK",
  sl: "sl-SI",
  so: "so-SO",
  sq: "sq-AL",
  sr: "sr-RS",
  sv: "sv-SE",
  sw: "sw-TZ",
  ta: "ta-IN",
  te: "te-IN",
  th: "th-TH",
  tr: "tr-TR",
  uk: "uk-UA",
  vi: "vi-VN",
  zh: "zh-CN",
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
  if (DEFAULT_BY_LANGUAGE[language] && COPY[DEFAULT_BY_LANGUAGE[language]]) {
    return DEFAULT_BY_LANGUAGE[language];
  }
  const regional = Object.keys(COPY).find((key) => key.toLowerCase().startsWith(`${language}-`));
  return regional ?? "en";
}

const TIGHT_BODY: Record<string, string> = {
  en: "Same account and settings as usual, without earlier chats. This conversation will not show up in your everyday chat list. Temporary data is removed after a normal exit.",
  "zh-CN": "账号和设置跟平时一样，看不到以前的对话，这次的聊天也不会进平时的列表。正常关掉后，这次的临时数据会清掉。",
  "zh-TW": "帳號和設定跟平時一樣，看不到以前的對話，這次的聊天也不會進平時的列表。正常關掉後，這次的臨時資料會清掉。",
  "zh-HK": "帳戶和設定跟平時一樣，看不到以前的對話，這次的聊天也不會進平時的列表。正常關掉後，這次的臨時資料會清掉。",
};

export function translate(locale: string, key: CopyKey): string {
  const resolved = resolveLocale(locale);
  if (key === "body") return TIGHT_BODY[resolved] ?? TIGHT_BODY.en;
  return COPY[resolved]?.[key] ?? COPY.en[key];
}
