import { expect, test } from "bun:test";
import { ACCESSIBILITY_SETUP_COPY } from "../src/runtime/incognito-copy.ts";
import { sharedPermissionCopy } from "../src/permission-shared-copy.ts";

test("the CLI guide keeps the installation reason and a distinct official-app reason in every locale", () => {
  const original = JSON.stringify(ACCESSIBILITY_SETUP_COPY);
  const copy = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY);
  expect(Object.keys(copy)).toHaveLength(65);
  for (const [locale, source] of Object.entries(ACCESSIBILITY_SETUP_COPY)) {
    expect(copy[locale].body).toBe(source.body);
    expect(copy[locale].body.length).toBeGreaterThan(0);
    expect(copy[locale].officialBody?.trim().length, `${locale} official app reason`).toBeGreaterThan(0);
    expect(copy[locale].officialBody).toContain("ChatGPT");
    expect(copy[locale].errorBody).not.toContain("incodex install");
    expect(copy[locale].errorBody).toContain("incodex accessibility");
    expect(copy[locale].dragInstruction).toBe(source.dragInstruction);
    expect(copy[locale].dragInstructionRuns).toBe(source.dragInstructionRuns);
  }
  expect(copy.en.officialBody).toBe("This is the official ChatGPT app. Allow it in System Settings to restore script control.");
  expect(copy["zh-CN"].officialBody).toBe("当前是官方 ChatGPT。请在系统设置中允许它的辅助功能权限，以恢复脚本控制。");
  expect(JSON.stringify(ACCESSIBILITY_SETUP_COPY)).toBe(original);
});
