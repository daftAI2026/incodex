import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ACCESSIBILITY_SETUP_COPY } from "../src/runtime/incognito-copy.ts";
import { attachAccessibilityDragInstructionRuns } from "../src/runtime/incognito-accessibility-copy-runs.ts";

type DragRun = {
  text: string;
  role: "primary" | "secondary";
};

type PermissionCopy = {
  addedBody: string;
  body: string;
  dragInstruction: string;
  dragInstructionRuns: string;
  errorBody: string;
  permissionTitle: string;
};

const guide = ACCESSIBILITY_SETUP_COPY as unknown as Record<string, PermissionCopy>;
const locales = Object.keys(guide).sort();

test("permission-only semantic copy is removed from the shared renderer bundle", () => {
  const renderer = readFileSync(join(import.meta.dir, "..", "dist", "incodex-inject.js"), "utf8");
  expect(renderer.includes("ACCESSIBILITY_DRAG_INSTRUCTION_RUNS"), "native-only table leaked into renderer").toBe(false);
  expect(renderer.includes(" to the list above to allow "), "native-only text leaked into renderer").toBe(false);
});

// Frozen from the authored copy table. This protects all 65 plain sentences
// independently of any same-source runs generated beside them. Five direct
// macOS pane references use the current local Settings label.
const DRAG_INSTRUCTION_BASELINE_SHA256 = "760d073837e32dcd18235e13824af649720d11a9b682867da7dfa025e5fa29f9";

const EXPECTED_RUNS: Record<string, DragRun[]> = {
  en: [
    { text: "Drag ", role: "secondary" },
    { text: "ChatGPT", role: "primary" },
    { text: " to the list above to allow ", role: "secondary" },
    { text: "Accessibility", role: "primary" },
  ],
  "zh-CN": [
    { text: "将 ", role: "secondary" },
    { text: "ChatGPT", role: "primary" },
    { text: " 拖到上方的“", role: "secondary" },
    { text: "无障碍", role: "primary" },
    { text: "”列表中，然后开启对应权限", role: "secondary" },
  ],
  "zh-HK": [
    { text: "將 ", role: "secondary" },
    { text: "ChatGPT", role: "primary" },
    { text: " 拖到上方的「", role: "secondary" },
    { text: "輔助使用", role: "primary" },
    { text: "」列表中，然後啟用權限", role: "secondary" },
  ],
  "zh-TW": [
    { text: "將 ", role: "secondary" },
    { text: "ChatGPT", role: "primary" },
    { text: " 拖曳到上方的「", role: "secondary" },
    { text: "輔助使用", role: "primary" },
    { text: "」列表中，然後啟用權限", role: "secondary" },
  ],
  "et-EE": [
    { text: "Juurdepääsetavuse", role: "primary" },
    { text: " lubamiseks lohista ", role: "secondary" },
    { text: "ChatGPT", role: "primary" },
    { text: " ülalolevasse loendisse", role: "secondary" },
  ],
  lt: [
    { text: "Nuvilkite ", role: "secondary" },
    { text: "ChatGPT", role: "primary" },
    { text: " į aukščiau esantį sąrašą, kad leistumėte ", role: "secondary" },
    { text: "Prieinamumą", role: "primary" },
  ],
  "tr-TR": [
    { text: "Erişilebilirliğe", role: "primary" },
    { text: " izin vermek için ", role: "secondary" },
    { text: "ChatGPT", role: "primary" },
    { text: "'yi yukarıdaki listeye sürükleyin", role: "secondary" },
  ],
};

const MACOS_ACCESSIBILITY_TERMS: Record<string, string> = {
  ar: "تسهيلات الاستخدام",
  "uk-UA": "Доступність",
  "zh-CN": "无障碍",
  "zh-HK": "輔助使用",
  "zh-TW": "輔助使用",
};

function parseRuns(locale: string): DragRun[] {
  const encoded = guide[locale]?.dragInstructionRuns;
  expect(typeof encoded, `${locale} must publish dragInstructionRuns as JSON`).toBe("string");
  if (typeof encoded !== "string") return [];

  let decoded: unknown;
  try {
    decoded = JSON.parse(encoded);
  } catch {
    throw new Error(`${locale} dragInstructionRuns is not valid JSON`);
  }
  expect(Array.isArray(decoded), `${locale} runs must be an array`).toBe(true);
  if (!Array.isArray(decoded)) return [];

  for (const run of decoded) {
    expect(run && typeof run === "object", `${locale} run must be an object`).toBe(true);
    if (!run || typeof run !== "object") continue;
    expect(Object.keys(run).sort(), `${locale} run schema`).toEqual(["role", "text"]);
    expect(typeof run.text, `${locale} run text`).toBe("string");
    expect(run.text.length, `${locale} run text must not be empty`).toBeGreaterThan(0);
    expect(["primary", "secondary"], `${locale} run role`).toContain(run.role);
  }
  return decoded as DragRun[];
}

test("preserves all 65 frozen dragInstruction sentences", () => {
  expect(locales).toHaveLength(65);
  const canonical = JSON.stringify(
    Object.fromEntries(locales.map(locale => [locale, guide[locale]?.dragInstruction])),
  );
  expect(createHash("sha256").update(canonical).digest("hex")).toBe(DRAG_INSTRUCTION_BASELINE_SHA256);
});

test("publishes exactly two semantic primary runs for every locale", () => {
  for (const locale of locales) {
    const copy = guide[locale];
    const runs = parseRuns(locale);
    expect(runs.map(run => run.text).join(""), `${locale} runs must concatenate to plain copy`).toBe(
      copy.dragInstruction,
    );

    const appRuns = runs.filter(run => run.text.includes("ChatGPT"));
    expect(appRuns, `${locale} ChatGPT must be one authored semantic run`).toHaveLength(1);
    expect(appRuns[0]?.role).toBe("primary");
    // The reference colors the dynamic application display name itself,
    // not following grammatical suffixes or postpositions.
    expect(appRuns[0]?.text).toBe("ChatGPT");

    const primaryRuns = runs.filter(run => run.role === "primary");
    expect(primaryRuns, `${locale} must mark app and local permission name`).toHaveLength(2);
    expect(primaryRuns.some(run => !run.text.includes("ChatGPT"))).toBe(true);
  }
});

test("missing optional styling metadata cannot break the Runtime copy catalog", () => {
  const plain = { future: { dragInstruction: "A future localized instruction", body: "Unchanged" } };
  const result = attachAccessibilityDragInstructionRuns(plain);
  expect(result.future.dragInstruction).toBe(plain.future.dragInstruction);
  expect(result.future.body).toBe("Unchanged");
  expect(result.future.dragInstructionRuns).toBe("");
});

test("keeps English and Chinese runs exact, including natural run order", () => {
  for (const [locale, expected] of Object.entries(EXPECTED_RUNS).slice(0, 4)) {
    expect(parseRuns(locale), locale).toEqual(expected);
    expect(parseRuns(locale).map(run => run.text).join(""), locale).toBe(guide[locale].dragInstruction);
  }
});

test("keeps inflected permission names as authored primary runs", () => {
  for (const locale of ["et-EE", "lt", "tr-TR"]) {
    expect(parseRuns(locale), locale).toEqual(EXPECTED_RUNS[locale]);
    expect(parseRuns(locale).map(run => run.text).join(""), locale).toBe(guide[locale].dragInstruction);
  }
});

test("uses the current macOS Accessibility label in direct navigation copy", () => {
  for (const [locale, term] of Object.entries(MACOS_ACCESSIBILITY_TERMS)) {
    const copy = guide[locale];
    expect(copy.permissionTitle, `${locale} card title`).toBe(term);
    expect(copy.addedBody, `${locale} Settings list`).toContain(term);
    expect(copy.dragInstruction, `${locale} drag target`).toContain(term);
    expect(copy.errorBody, `${locale} recovery breadcrumb`).toContain(`→ ${term}`);

    const permissionNameRuns = parseRuns(locale).filter(run => run.role === "primary" && run.text !== "ChatGPT");
    expect(permissionNameRuns, `${locale} permission-name emphasis`).toEqual([{ text: term, role: "primary" }]);
  }
  // This introductory Ukrainian sentence quotes the pane name; it is not
  // merely a generic explanation of Accessibility permission.
  expect(guide["uk-UA"].body).toContain("«Доступність»");
});
