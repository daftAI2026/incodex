import { expect, test } from "bun:test";
import { ACCESSIBILITY_SETUP_COPY } from "../src/runtime/incognito-copy.ts";
import { sharedPermissionCopy } from "../src/permission-shared-copy.ts";

test("the CLI guide keeps the installation reason and a distinct official-app reason in every locale", () => {
  const original = JSON.stringify(ACCESSIBILITY_SETUP_COPY);
  const copy = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY);
  expect(Object.keys(copy)).toHaveLength(65);
  for (const [locale, source] of Object.entries(ACCESSIBILITY_SETUP_COPY)) {
    expect(copy[locale].title).toBe(source.title);
    expect(typeof copy[locale].installedTitle, `${locale} installed title`).toBe("string");
    expect(copy[locale].installedTitle.trim().length, `${locale} installed title`).toBeGreaterThan(0);
    expect(copy[locale].installedTitle, `${locale} localized re-enable title`).not.toBe(source.title);
    expect(copy[locale].body).toBe(source.body);
    expect(copy[locale].body.length).toBeGreaterThan(0);
    expect(copy[locale].officialBody?.trim().length, `${locale} official app reason`).toBeGreaterThan(0);
    expect(copy[locale].officialBody).toContain("ChatGPT");
    expect(copy[locale].errorBody).not.toContain("incodex install");
    expect(copy[locale].errorBody).toContain("incodex accessibility");
    expect(copy[locale].dragInstruction).toBe(source.dragInstruction);
    expect(copy[locale].dragInstructionRuns).toBe(source.dragInstructionRuns);
  }
  expect(copy.en.title).toBe("Enable ChatGPT script control");
  expect(copy.en.installedTitle).toBe("Re-enable ChatGPT script control");
  expect(copy["zh-CN"].title).toBe("启用 ChatGPT 脚本控制");
  expect(copy["zh-CN"].installedTitle).toBe("重新启用 ChatGPT 脚本控制");
  expect(copy["zh-HK"].title).toBe("啟用 ChatGPT 腳本控制");
  expect(copy["zh-HK"].installedTitle).toBe("重新啟用 ChatGPT 腳本控制");
  expect(copy["zh-TW"].title).toBe("啟用 ChatGPT 腳本控制");
  expect(copy["zh-TW"].installedTitle).toBe("重新啟用 ChatGPT 腳本控制");
  expect(copy.en.officialBody).toBe("This is the official ChatGPT app. Allow it in System Settings to restore script control.");
  expect(copy["zh-CN"].officialBody).toBe("当前是官方 ChatGPT。请在系统设置中允许它的辅助功能权限，以恢复脚本控制。");
  expect(JSON.stringify(ACCESSIBILITY_SETUP_COPY)).toBe(original);
});

test("the permission error names the CLI action that reopens its guide", () => {
  const copy = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY);
  expect(copy.en.errorBody).toContain("or run incodex accessibility to reopen the permission guide");
  expect(copy["zh-CN"].errorBody).toContain("或运行 incodex accessibility 重新打开授权卡");
  expect(copy["zh-HK"].errorBody).toContain("或執行 incodex accessibility 重新開啟授權卡");
  expect(copy["zh-TW"].errorBody).toContain("或執行 incodex accessibility 重新開啟授權卡");
});

test("the Screen Recording row has independent localized copy and valid semantic runs", () => {
  const original = JSON.stringify(ACCESSIBILITY_SETUP_COPY);
  const copy = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY);
  const screenKeys = [
    "screenTitle",
    "screenDescription",
    "screenAddedTitle",
    "screenAddedBody",
    "screenDragInstruction",
    "screenDragInstructionRuns",
    "screenChecking",
    "done",
    "screenUnknown",
  ] as const;

  expect(Object.keys(copy)).toHaveLength(65);
  for (const [locale, source] of Object.entries(ACCESSIBILITY_SETUP_COPY)) {
    const screenCopy = copy[locale];
    for (const key of screenKeys) {
      expect(typeof screenCopy[key], `${locale} ${key}`).toBe("string");
      expect(screenCopy[key].trim().length, `${locale} ${key}`).toBeGreaterThan(0);
      if (locale !== "en") {
        expect(screenCopy[key], `${locale} ${key} must not fall back to English`).not.toBe(copy.en[key]);
      }
      expect(screenCopy[key].toLowerCase(), `${locale} ${key} must name the host, not Incodex or CUA`)
        .not.toContain("incodex");
      expect(screenCopy[key].toLowerCase(), `${locale} ${key} must not mention vendor CUA`)
        .not.toContain("cua");
      expect(screenCopy[key], `${locale} ${key} must not mention the private-window flow`)
        .not.toMatch(/incognito|无痕|無痕|隐身|隱身/i);
    }

    expect(screenCopy.screenTitle, `${locale} title must name the macOS permission`)
      .not.toBe(source.permissionTitle);
    expect(screenCopy.screenDragInstruction, `${locale} instruction must differ from the AX instruction`)
      .not.toBe(source.dragInstruction);
    expect(screenCopy.screenDragInstruction, `${locale} instruction must name the current ChatGPT host`)
      .toContain("/Applications/ChatGPT.app");

    let runs: unknown;
    try {
      runs = JSON.parse(screenCopy.screenDragInstructionRuns);
    } catch {
      throw new Error(`${locale} screenDragInstructionRuns is not valid JSON`);
    }
    expect(Array.isArray(runs), `${locale} screen instruction runs must be an array`).toBe(true);
    if (!Array.isArray(runs)) continue;
    expect(runs.length, `${locale} screen instruction must have semantic runs`).toBeGreaterThan(0);
    for (const run of runs) {
      expect(run && typeof run === "object", `${locale} screen instruction run`).toBe(true);
      if (!run || typeof run !== "object") continue;
      expect(Object.keys(run).sort(), `${locale} screen instruction run schema`).toEqual(["role", "text"]);
      expect(typeof run.text, `${locale} screen instruction run text`).toBe("string");
      expect(run.text.length, `${locale} screen instruction run text`).toBeGreaterThan(0);
      expect(["primary", "secondary"], `${locale} screen instruction run role`).toContain(run.role);
    }
    expect(runs.map(run => run.text).join(""), `${locale} semantic runs must preserve the instruction`)
      .toBe(screenCopy.screenDragInstruction);
    expect(runs.some(run => run.role === "primary" && run.text.includes("/Applications/ChatGPT.app")),
      `${locale} must emphasize the ChatGPT host path`).toBe(true);
  }

  expect(copy.en.screenTitle).toBe("Screen Recording");
  expect(copy["zh-CN"].screenTitle).toBe("屏幕录制");
  expect(copy.en.screenAddedBody).toBe(
    "Add or enable /Applications/ChatGPT.app in Screen Recording settings. If macOS asks, quit and reopen it normally. This line is complete when the permission is available.",
  );
  expect(copy["zh-CN"].screenAddedBody).toBe(
    "在屏幕录制设置中添加或启用 /Applications/ChatGPT.app。若系统要求，请正常退出并重新打开它。权限可用后，本行完成。",
  );
  expect(copy.en.screenUnknown).toBe("Screen Recording permission has not been verified.");
  expect(copy["zh-CN"].screenUnknown).toBe("尚未验证屏幕录制权限。");
  expect(JSON.stringify(ACCESSIBILITY_SETUP_COPY)).toBe(original);
});
