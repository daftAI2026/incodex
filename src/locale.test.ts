import { describe, expect, test } from "bun:test";
import { resolveLocale, translate } from "./runtime/incognito-copy";

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
    expect(translate("zh-CN", "body")).toContain("~/.codex");
    expect(translate("zh-TW", "body")).toContain("~/.codex");
  });

  test("language-only tags pick a regional default when needed", () => {
    expect(resolveLocale("pt")).toMatch(/^pt/);
    expect(resolveLocale("en-GB")).toBe("en");
  });
});
