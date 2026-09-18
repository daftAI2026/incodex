import { describe, expect, test } from "bun:test";
import { COPY, ACCESSIBILITY_SETUP_COPY, resolveLocale, translate } from "./runtime/incognito-copy.ts";
import { resolveLocaleDirection } from "./runtime/incodex-locale.cts";

describe("locale fallback", () => {
  test("empty and unknown values fall back to English", () => {
    expect(resolveLocale("")).toBe("en");
    expect(resolveLocale("xx-YY")).toBe("en");
    expect(translate("xx-YY", "open")).toBe(translate("en", "open"));
  });

  test("Chinese variants map to the tight body copy", () => {
    expect(resolveLocale("zh")).toBe("zh-CN");
    expect(resolveLocale("zh-Hant")).toBe("zh-TW");
    expect(resolveLocale("zh-HK")).toBe("zh-HK");
    expect(translate("zh-CN", "body")).toContain("平时的列表");
    expect(translate("zh-TW", "body")).toContain("平時的列表");
  });

  test("Chinese window labels use 私密 across regional variants", () => {
    for (const locale of ["zh-CN", "zh-HK", "zh-TW"]) {
      for (const key of ["open", "exit", "title", "dismiss", "errorTitle"] as const) {
        expect(translate(locale, key)).toContain("私密");
      }
    }
  });

  test("Accessibility setup copy preserves the supported Chinese script variants", () => {
    const accessibilityCopy = ACCESSIBILITY_SETUP_COPY as Record<string, { body: string; back?: string; addedBody?: string }>;
    expect(resolveLocale("zh-CN")).toBe("zh-CN");
    expect(resolveLocale("zh-HK")).toBe("zh-HK");
    expect(resolveLocale("zh-TW")).toBe("zh-TW");
    expect(resolveLocale("zh-Hant-HK")).toBe("zh-HK");
    expect(resolveLocale("zh-Hant")).toBe("zh-TW");

    expect(accessibilityCopy["en"].body).toBe(
      "Installing Incodex modifies ChatGPT, so its Accessibility permission needs to be granted again.",
    );
    expect(accessibilityCopy["en"].back).toBe("Back");
    expect(accessibilityCopy["zh-CN"].body).toBe(
      "安装 Incodex 会修改 ChatGPT，因此需要重新授予它辅助功能权限。",
    );
    expect(accessibilityCopy["zh-CN"].back).toBe("返回");
    expect(accessibilityCopy["zh-HK"].body).toBe(
      "安裝 Incodex 會修改 ChatGPT，因此需要重新授予它輔助功能權限。",
    );
    expect(accessibilityCopy["zh-HK"].back).toBe("返回");
    expect(accessibilityCopy["zh-HK"].addedBody).toContain("確認取得權限後");
    expect(accessibilityCopy["zh-TW"].body).toBe(
      "安裝 Incodex 會修改 ChatGPT，因此需要重新授予它輔助功能權限。",
    );
    expect(accessibilityCopy["zh-TW"].back).toBe("返回");
    expect(accessibilityCopy["zh-TW"].addedBody).toContain("確認取得權限後");
    for (const key of ["en", "zh-CN", "zh-HK", "zh-TW"] as const) {
      expect(accessibilityCopy[key].body).not.toContain("\n");
    }
  });

  test("language-only tags pick a regional default when needed", () => {
    expect(resolveLocale("de")).toBe("de-DE");
    expect(resolveLocale("es")).toBe("es-419");
    expect(resolveLocale("fr")).toBe("fr-FR");
    expect(resolveLocale("no")).toBe("nb-NO");
    expect(resolveLocale("pt")).toBe("pt-BR");
    expect(resolveLocale("en-GB")).toBe("en");
  });

  test("resolves native layout direction from the canonical locale", () => {
    const catalog = ACCESSIBILITY_SETUP_COPY as Record<string, unknown>;
    for (const locale of ["ar", "AR", "ar-SA", "ar_SA", "fa", "fa-IR", "ur", "ur-PK"]) {
      expect(resolveLocaleDirection(locale, catalog), locale).toBe("rightToLeft");
    }
    for (const locale of ["", "en-US", "zh-Hant-HK", "de-DE", "xx-YY", "unknown"]) {
      expect(resolveLocaleDirection(locale, catalog), locale).toBe("leftToRight");
    }
  });

  test("regional locales keep the intentionally tight English body fallback", () => {
    expect(translate("fr-FR", "body")).toBe(translate("en", "body"));
  });
});


test("permission guide covers every supported Codex locale with complete copy", () => {
  const guide = ACCESSIBILITY_SETUP_COPY as Record<string, Record<string, string>>;
  expect(Object.keys(guide).sort()).toEqual(Object.keys(COPY).sort());
  const keys = Object.keys(guide.en).sort();
  for (const copy of Object.values(guide)) {
    expect(Object.keys(copy).sort()).toEqual(keys);
    for (const text of Object.values(copy)) expect(text.trim().length).toBeGreaterThan(0);
    expect(copy.body).toContain("Incodex");
    expect(copy.body).toContain("ChatGPT");
    expect(copy.dragInstruction).toContain("ChatGPT");
    expect(copy.errorBody).toContain("/Applications/ChatGPT.app");
    expect(copy.errorBody).toContain("incodex install");
  }
});


test("permission setup dismiss action retains the reference Skip meaning in source languages", () => {
  expect(ACCESSIBILITY_SETUP_COPY.en.later).toBe("Skip");
  expect(ACCESSIBILITY_SETUP_COPY["zh-CN"].later).toBe("跳过");
  expect(ACCESSIBILITY_SETUP_COPY["zh-HK"].later).toBe("略過");
  expect(ACCESSIBILITY_SETUP_COPY["zh-TW"].later).toBe("略過");
});

const AUDITED_REGIONAL_SKIP_COPY: Record<string, string> = {
  "am": "ዝለል",
  "ar": "تخطي",
  "bg-BG": "Пропусни",
  "bn-BD": "এড়িয়ে যান",
  "bs-BA": "Preskoči",
  "ca-ES": "Omet",
  "cs-CZ": "Přeskočit",
  "da-DK": "Spring over",
  "de-DE": "Überspringen",
  "el-GR": "Παράλειψη",
  "es-419": "Omitir",
  "es-ES": "Omitir",
  "et-EE": "Jäta vahele",
  "fa": "رد شدن",
  "fi-FI": "Ohita",
  "fr-CA": "Ignorer",
  "fr-FR": "Ignorer",
  "gu-IN": "છોડી દો",
  "hi-IN": "छोड़ें",
  "hr-HR": "Preskoči",
  "hu-HU": "Kihagyás",
  "hy-AM": "Բաց թողնել",
  "id-ID": "Lewati",
  "is-IS": "Sleppa",
  "it-IT": "Salta",
  "ja-JP": "スキップ",
  "ka-GE": "გამოტოვება",
  "kk": "Өткізіп жіберу",
  "kn-IN": "ಬಿಟ್ಟುಬಿಡಿ",
  "ko-KR": "건너뛰기",
  "lt": "Praleisti",
  "lv-LV": "Izlaist",
  "mk-MK": "Прескокни",
  "ml": "ഒഴിവാക്കുക",
  "mn": "Алгасах",
  "mr-IN": "वगळा",
  "ms-MY": "Langkau",
  "my-MM": "ကျော်ရန်",
  "nb-NO": "Hopp over",
  "nl-NL": "Overslaan",
  "pa": "ਛੱਡੋ",
  "pl-PL": "Pomiń",
  "pt-BR": "Pular",
  "pt-PT": "Ignorar",
  "ro-RO": "Omiteți",
  "ru-RU": "Пропустить",
  "sk-SK": "Preskočiť",
  "sl-SI": "Preskoči",
  "so-SO": "Ka bood",
  "sq-AL": "Anashkalo",
  "sr-RS": "Прескочи",
  "sv-SE": "Hoppa över",
  "sw-TZ": "Ruka",
  "ta-IN": "தவிர்",
  "te-IN": "దాటవేయి",
  "th-TH": "ข้าม",
  "tl": "Laktawan",
  "tr-TR": "Atla",
  "uk-UA": "Пропустити",
  "ur": "چھوڑیں",
  "vi-VN": "Bỏ qua",
};

test("regional permission dismiss labels retain audited Skip semantics", () => {
  const guide = ACCESSIBILITY_SETUP_COPY as Record<string, Record<string, string>>;
  const regionalLocales = Object.keys(guide).filter(
    (locale) => !["en", "zh-CN", "zh-HK", "zh-TW"].includes(locale),
  );
  expect(regionalLocales.sort()).toEqual(Object.keys(AUDITED_REGIONAL_SKIP_COPY).sort());
  for (const [locale, expected] of Object.entries(AUDITED_REGIONAL_SKIP_COPY)) {
    expect(guide[locale]?.later).toBe(expected);
  }
});
