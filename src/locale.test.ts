import { describe, expect, test } from "bun:test";
import { resolveLocale, translate } from "./runtime/incognito-copy.ts";

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

  test("language-only tags pick a regional default when needed", () => {
    expect(resolveLocale("de")).toBe("de-DE");
    expect(resolveLocale("es")).toBe("es-419");
    expect(resolveLocale("fr")).toBe("fr-FR");
    expect(resolveLocale("no")).toBe("nb-NO");
    expect(resolveLocale("pt")).toBe("pt-BR");
    expect(resolveLocale("en-GB")).toBe("en");
  });

  test("regional locales keep the intentionally tight English body fallback", () => {
    expect(translate("fr-FR", "body")).toBe(translate("en", "body"));
  });
});
