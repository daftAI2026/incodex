import { describe, expect, test } from "bun:test";
import { ACCESSIBILITY_SETUP_COPY, resolveLocale, translate } from "./runtime/incognito-copy.ts";

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
      "Installing Incodex modifies ChatGPT, so grant Accessibility access again to let it continue controlling other apps.",
    );
    expect(accessibilityCopy["en"].back).toBe("Back");
    expect(accessibilityCopy["zh-CN"].body).toBe(
      "安装 Incodex 会修改 ChatGPT，因此需要重新授予辅助功能权限，才能继续操作其他应用。",
    );
    expect(accessibilityCopy["zh-CN"].back).toBe("返回");
    expect(accessibilityCopy["zh-HK"].body).toBe(
      "安裝 Incodex 會修改 ChatGPT，因此需要重新授予輔助功能權限，才能繼續操作其他應用程式。",
    );
    expect(accessibilityCopy["zh-HK"].back).toBe("返回");
    expect(accessibilityCopy["zh-HK"].addedBody).toContain("確認取得權限後");
    expect(accessibilityCopy["zh-TW"].body).toBe(
      "安裝 Incodex 會修改 ChatGPT，因此需要重新授予輔助功能權限，才能繼續操作其他應用程式。",
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

  test("regional locales keep the intentionally tight English body fallback", () => {
    expect(translate("fr-FR", "body")).toBe(translate("en", "body"));
  });
});
