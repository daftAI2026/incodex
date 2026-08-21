import { COPY, type CopyKey } from "./incognito-copy-data";

export type { CopyKey, CopyTable } from "./incognito-copy-data";

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
