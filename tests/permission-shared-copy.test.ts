/**
 * [INPUT]: 依赖 65 语言 AX/录屏文案目录与 sharedPermissionCopy 合并器。
 * [OUTPUT]: 验证双权限统一模板、65 语言覆盖、系统名插槽与语义 runs。
 * [POS]: 权限卡小卡文案回归；不启动宿主或访问 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { ACCESSIBILITY_SETUP_COPY } from "../src/runtime/incognito-copy.ts";
import { sharedPermissionCopy } from "../src/permission-shared-copy.ts";

test("the CLI guide keeps the installation reason and a distinct official-app reason in every locale", () => {
  const original = JSON.stringify(ACCESSIBILITY_SETUP_COPY);
  const copy = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY);
  expect(Object.keys(copy)).toHaveLength(65);
  for (const [locale, source] of Object.entries(ACCESSIBILITY_SETUP_COPY)) {
    const template = copy[locale].permissionListInstruction;
    expect(copy[locale].title).toBe(source.title);
    expect(typeof copy[locale].installedTitle, `${locale} installed title`).toBe("string");
    expect(copy[locale].installedTitle.trim().length, `${locale} installed title`).toBeGreaterThan(0);
    expect(copy[locale].installedTitle, `${locale} localized re-enable title`).not.toBe(source.title);
    expect(copy[locale].body).toBe(source.body);
    expect(copy[locale].body).toContain("Incodex");
    expect(copy[locale].body, `${locale} installation reason covers both permissions`).not.toContain(source.permissionTitle);
    expect(copy[locale].officialBody, `${locale} official reason covers both permissions`).not.toContain(source.permissionTitle);
    expect(copy[locale].officialBody?.trim().length, `${locale} official app reason`).toBeGreaterThan(0);
    expect(copy[locale].officialBody).toContain("ChatGPT");
    expect(copy[locale].errorBody).not.toContain("incodex install");
    expect(copy[locale].errorBody).toContain("incodex accessibility");
    expect(copy[locale].dragInstruction).not.toBe(source.dragInstruction);
    expect(copy[locale].dragInstruction).toBe(
      copy[locale].permissionListInstruction.replace("{permissionTitle}", source.permissionTitle),
    );
    const accessibilityRuns = JSON.parse(copy[locale].dragInstructionRuns);
    expect(accessibilityRuns.map((run: { text: string }) => run.text).join(""))
      .toBe(copy[locale].dragInstruction);
    expect(accessibilityRuns.filter((run: { text: string; role: string }) =>
      run.role === "primary" && run.text === "ChatGPT")).toHaveLength(1);
    expect(accessibilityRuns.filter((run: { text: string; role: string }) =>
      run.role === "primary" && run.text === source.permissionTitle)).toHaveLength(1);
    expect(copy[locale].dragInstruction).toBe(
      template.replace("{permissionTitle}", source.permissionTitle),
    );
  }
  expect(copy.en.title).toBe("Set up ChatGPT permissions");
  expect(copy.en.installedTitle).toBe("Set up ChatGPT permissions again");
  expect(copy["zh-CN"].title).toBe("设置 ChatGPT 权限");
  expect(copy["zh-CN"].installedTitle).toBe("重新设置 ChatGPT 权限");
  expect(copy["zh-HK"].title).toBe("設定 ChatGPT 權限");
  expect(copy["zh-HK"].installedTitle).toBe("重新設定 ChatGPT 權限");
  expect(copy["zh-TW"].title).toBe("設定 ChatGPT 權限");
  expect(copy["zh-TW"].installedTitle).toBe("重新設定 ChatGPT 權限");
  expect(copy.en.body).toBe("After installing Incodex, confirm these permissions again to use apps on your Mac.");
  expect(copy["zh-CN"].body).toBe("安装 Incodex 后，需要重新确认这些权限才能使用 Mac 上的应用。");
  expect(copy.en.addedBody).toBe("Drag the ChatGPT icon above into the Accessibility list and enable it.");
  expect(copy["zh-CN"].addedBody).toBe("将上方 ChatGPT 图标拖入“无障碍”列表，并开启权限。");
  expect(copy.en.dragInstruction).toBe(
    "Drag the ChatGPT icon into the “Accessibility” list above and enable the permission.",
  );
  expect(copy["zh-CN"].dragInstruction).toBe(
    "将 ChatGPT 图标拖入上方的“无障碍”列表，并开启权限",
  );
  expect(copy.en.officialBody).toBe("This is the official ChatGPT app. Confirm these permissions to use apps on your Mac.");
  expect(copy["zh-CN"].officialBody).toBe("当前是官方 ChatGPT。请确认这些权限，以便使用 Mac 上的应用。");
  expect(JSON.stringify(ACCESSIBILITY_SETUP_COPY)).toBe(original);
});

test("the permission error names the CLI action that reopens its guide", () => {
  const copy = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY);
  expect(copy.en.errorBody).toContain("or run incodex accessibility to reopen the permission guide");
  expect(copy["zh-CN"].errorBody).toContain("或运行 incodex accessibility 重新打开授权卡");
  expect(copy["zh-HK"].errorBody).toContain("或執行 incodex accessibility 重新開啟授權卡");
  expect(copy["zh-TW"].errorBody).toContain("或執行 incodex accessibility 重新開啟授權卡");
});

test("platform presentation receives explicit localized app-name templates", () => {
  const original = JSON.stringify(ACCESSIBILITY_SETUP_COPY);
  const copy = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY);
  for (const [locale, value] of Object.entries(copy)) {
    expect(value.permissionLocale).toBe(locale);
    const instruction = value.permissionListInstruction;
    const runs = JSON.parse(value.permissionListInstructionRuns);
    expect(instruction, `${locale} AX template names the visible app`).toContain("ChatGPT");
    expect(instruction, `${locale} AX template has no bundle path`).not.toContain("/Applications/ChatGPT.app");
    expect(instruction, `${locale} AX template has one explicit permission slot`).toContain("{permissionTitle}");
    expect(instruction.match(/\{permissionTitle\}/g), `${locale} AX template slot count`).toHaveLength(1);
    expect(instruction).not.toContain(value.screenTitle);
    expect(runs.map((run: { text: string }) => run.text).join("")).toBe(instruction);
    expect(runs.filter((run: { text: string; role: string }) =>
      run.role === "primary" && run.text === "ChatGPT")).toHaveLength(1);
    expect(runs.filter((run: { text: string; role: string }) =>
      run.role === "primary" && run.text === "{permissionTitle}")).toHaveLength(1);

    const screenInstruction = value.screenDragInstruction;
    const screenRuns = JSON.parse(value.screenDragInstructionRuns);
    expect(screenInstruction, `${locale} screen instruction names the visible app`).toContain("ChatGPT");
    expect(screenInstruction, `${locale} screen instruction uses the localized permission name`)
      .toContain(value.screenTitle);
    expect(screenInstruction, `${locale} screen instruction has no bundle path`)
      .not.toContain("/Applications/ChatGPT.app");
    expect(screenRuns.map((run: { text: string }) => run.text).join("")).toBe(screenInstruction);
    expect(screenRuns.filter((run: { text: string; role: string }) =>
      run.role === "primary" && run.text === "ChatGPT")).toHaveLength(1);
    expect(screenRuns.filter((run: { text: string; role: string }) =>
      run.role === "primary" && run.text === value.screenTitle)).toHaveLength(1);
  }
  expect(copy.en.permissionListInstruction).toBe(
    "Drag the ChatGPT icon into the “{permissionTitle}” list above and enable the permission.",
  );
  expect(copy["zh-CN"].permissionListInstruction).toBe(
    "将 ChatGPT 图标拖入上方的“{permissionTitle}”列表，并开启权限",
  );
  expect(copy["fr-FR"].permissionListInstruction).toBe(
    "Faites glisser l’icône de ChatGPT dans la liste « {permissionTitle} » ci-dessus et activez l’autorisation.",
  );
  expect(copy.en.screenDragInstruction).toBe(
    "Drag the ChatGPT icon into the “Screen & System Audio Recording” list above and enable the permission.",
  );
  expect(copy["zh-CN"].screenDragInstruction).toBe(
    "将 ChatGPT 图标拖入上方的“录屏与系统录音”列表，并开启权限",
  );
  expect(copy["zh-HK"].dragInstruction).toBe(
    "將 ChatGPT 圖示拖入上方的「輔助使用」列表，並啟用權限",
  );
  expect(copy["zh-TW"].screenDragInstruction).toBe(
    "將 ChatGPT 圖示拖曳至上方的「螢幕與系統錄音」列表，並啟用權限",
  );
  expect(JSON.stringify(ACCESSIBILITY_SETUP_COPY)).toBe(original);
});

test("macOS 27 system labels replace only the explicit permission slot", () => {
  const copy = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY);
  const labels = {
    ar: "تسهيلات الاستخدام",
    "uk-UA": "Доступність",
    "zh-CN": "无障碍",
    "zh-HK": "輔助使用",
    "zh-TW": "輔助使用",
  };
  for (const [locale, label] of Object.entries(labels)) {
    const instruction = copy[locale].permissionListInstruction.replace("{permissionTitle}", label);
    const runs = JSON.parse(copy[locale].permissionListInstructionRuns).map(
      (run: { text: string; role: string }) => ({
        ...run,
        text: run.text.replace("{permissionTitle}", label),
      }),
    );
    expect(instruction, `${locale} system label`).toContain(label);
    expect(instruction, `${locale} system label instruction`).not.toContain("/Applications/ChatGPT.app");
    expect(runs.map((run: { text: string }) => run.text).join("")).toBe(instruction);
    expect(runs.filter((run: { text: string; role: string }) =>
      run.role === "primary" && run.text === label)).toHaveLength(1);
  }
});

test.each(["zh-CN", "zh-HK", "zh-TW"])("%s keeps Chinese sentences adjacent without an ASCII space", locale => {
  const copy = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY);
  expect(copy[locale].screenAddedBody).not.toMatch(/。 +[\u3400-\u9fff]/);
});

test.each([
  ["zh-CN", "ChatGPT 通过截图识别屏幕内容和点击位置"],
  ["zh-HK", "ChatGPT 透過螢幕截圖識別畫面內容和點擊位置"],
  ["zh-TW", "ChatGPT 透過螢幕截圖辨識畫面內容和點擊位置"],
  ["ja-JP", "ChatGPT はスクリーンショットで画面の内容やクリックする位置を認識します"],
])("%s omits the trailing period from the Screen Recording row description", (locale, description) => {
  const copy = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY);
  expect(copy[locale].screenDescription).toBe(description);
  expect(copy[locale].screenDescription).not.toMatch(/[。.]$/);
  expect(copy[locale].permissionDescription).not.toMatch(/[。.]$/);
  expect(copy.en.screenDescription).toBe("ChatGPT uses screenshots to identify screen content and where to click.");
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
      expect(screenCopy[key], `${locale} ${key} must not mention vendor CUA`)
        .not.toMatch(/\b(?:CUA|Cavalry)\b/i);
      expect(screenCopy[key], `${locale} ${key} must not mention the private-window flow`)
        .not.toMatch(/incognito|无痕|無痕|隐身|隱身/i);
    }

    expect(screenCopy.screenTitle, `${locale} title must name the macOS permission`)
      .not.toBe(source.permissionTitle);
    expect(screenCopy.screenDescription, `${locale} description must name ChatGPT`)
      .toContain("ChatGPT");
    expect(screenCopy.screenAddedTitle, `${locale} added title must name ChatGPT`)
      .toContain("ChatGPT");
    expect(screenCopy.screenAddedBody, `${locale} added body must name the current app bundle`)
      .toContain("/Applications/ChatGPT.app");
    expect(screenCopy.screenDragInstruction, `${locale} instruction must differ from the AX instruction`)
      .not.toBe(source.dragInstruction);
    expect(screenCopy.screenDragInstruction, `${locale} instruction must name the visible ChatGPT app`)
      .toContain("ChatGPT");
    for (const key of ["screenDescription", "screenAddedTitle", "screenAddedBody", "screenDragInstruction", "screenChecking", "screenUnknown"]) {
      expect(screenCopy[key], `${locale} ${key} must not reuse the AX permission label`)
        .not.toContain(source.permissionTitle);
    }

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
    expect(runs.filter(run => run.role === "primary" && run.text === "ChatGPT"),
      `${locale} must emphasize the visible app name`).toHaveLength(1);
    expect(runs.filter(run => run.role === "primary" && run.text === screenCopy.screenTitle),
      `${locale} must emphasize the system permission name`).toHaveLength(1);
  }

  expect(copy.en.screenTitle).toBe("Screen & System Audio Recording");
  expect(copy["zh-CN"].screenTitle).toBe("录屏与系统录音");
  expect(copy.en.screenDescription).toBe(
    "ChatGPT uses screenshots to identify screen content and where to click.",
  );
  expect(copy["zh-CN"].screenDescription).toBe(
    "ChatGPT 通过截图识别屏幕内容和点击位置",
  );
  expect(copy.en.screenAddedBody).toBe(
    "Add /Applications/ChatGPT.app to the list above and enable it. If macOS asks, quit and reopen ChatGPT normally.",
  );
  expect(copy["zh-CN"].screenAddedBody).toBe(
    "将 /Applications/ChatGPT.app 添加到上方列表，并开启权限。若 macOS 要求，请正常退出并重新打开 ChatGPT。",
  );
  expect(copy.en.screenUnknown).toBe("Screen & System Audio Recording permission has not been verified.");
  expect(copy["zh-CN"].screenUnknown).toBe("尚未确认录屏与系统录音权限。");
  expect(JSON.stringify(ACCESSIBILITY_SETUP_COPY)).toBe(original);
});
