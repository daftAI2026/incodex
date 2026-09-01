import { describe, expect, test } from "bun:test";
import {
  CAPTURE_CANDIDATE_SELECTOR,
  collectCaptureCandidates,
  markCodexPrivacyPlaceholders,
} from "./privacy.ts";

type FakeRect = {
  bottom: number;
  height: number;
  left: number;
  right: number;
  top: number;
  width: number;
};

class FakeElement {
  readonly children = new Set<FakeElement>();
  readonly hiddenFromCapture: boolean;
  readonly rect: FakeRect;
  readonly tagName: string;
  readonly textContent: string;
  readonly value: string;
  readonly visible: boolean;

  constructor(options: {
    hiddenFromCapture?: boolean;
    rect: FakeRect;
    tagName?: string;
    text?: string;
    value?: string;
    visible?: boolean;
  }) {
    this.hiddenFromCapture = options.hiddenFromCapture ?? false;
    this.rect = options.rect;
    this.tagName = options.tagName ?? "P";
    this.textContent = options.text ?? "Private text";
    this.value = options.value ?? "";
    this.visible = options.visible ?? true;
  }

  checkVisibility(): boolean {
    return this.visible;
  }

  closest(selector: string): FakeElement | null {
    return selector === "[data-incodex-capture-hide]" && this.hiddenFromCapture ? this : null;
  }

  contains(element: FakeElement): boolean {
    return this.children.has(element);
  }

  getBoundingClientRect(): FakeRect {
    return this.rect;
  }
}

function rect(left: number, top: number, width: number, height: number): FakeRect {
  return { bottom: top + height, height, left, right: left + width, top, width };
}

function fakeDocument(elements: FakeElement[]): Document {
  return {
    querySelectorAll(selector: string): FakeElement[] {
      expect(selector).toBe(CAPTURE_CANDIDATE_SELECTOR);
      return elements;
    },
  } as unknown as Document;
}

describe("capture redaction candidates", () => {
  test("collects visible text, image, and input candidates across the viewport", () => {
    const elements = [
      new FakeElement({ rect: rect(10, 20, 120, 24), tagName: "P", text: "Project name" }),
      new FakeElement({ rect: rect(300, 180, 240, 80), tagName: "IMG", text: "" }),
      new FakeElement({ rect: rect(420, 520, 260, 44), tagName: "TEXTAREA", value: "Secret" }),
    ];

    const candidates = collectCaptureCandidates(fakeDocument(elements), {
      height: 801,
      width: 1200,
    });

    expect(candidates).toEqual([
      { height: 24, id: "r:10:20:120:24", width: 120, x: 10, y: 20 },
      { height: 80, id: "r:300:180:240:80", width: 240, x: 300, y: 180 },
      { height: 44, id: "r:420:520:260:44", width: 260, x: 420, y: 520 },
    ]);
  });

  test("accepts any non-empty textarea value like the reference collector", () => {
    const candidates = collectCaptureCandidates(fakeDocument([
      new FakeElement({ rect: rect(20, 30, 120, 24), tagName: "TEXTAREA", value: "x" }),
    ]), { height: 801, width: 1200 });

    expect(candidates).toEqual([
      { height: 24, id: "r:20:30:120:24", width: 120, x: 20, y: 30 },
    ]);
  });

  test("rejects hidden, empty, tiny, offscreen, invisible, and nested candidates", () => {
    const parent = new FakeElement({ rect: rect(10, 20, 180, 60), text: "Parent content" });
    const child = new FakeElement({ rect: rect(20, 30, 100, 20), text: "Child content" });
    parent.children.add(child);
    const elements = [
      parent,
      child,
      new FakeElement({ hiddenFromCapture: true, rect: rect(10, 120, 100, 20) }),
      new FakeElement({ rect: rect(10, 160, 20, 20) }),
      new FakeElement({ rect: rect(10, 200, 100, 10) }),
      new FakeElement({ rect: rect(1300, 220, 100, 20) }),
      new FakeElement({ rect: rect(10, 260, 100, 20), text: "  " }),
      new FakeElement({ rect: rect(10, 300, 100, 20), tagName: "TEXTAREA", value: "" }),
      new FakeElement({ rect: rect(10, 340, 100, 20), visible: false }),
    ];

    const candidates = collectCaptureCandidates(fakeDocument(elements), {
      height: 801,
      width: 1200,
    });

    expect(candidates).toEqual([
      { height: 60, id: "r:10:20:180:60", width: 180, x: 10, y: 20 },
    ]);
  });

  test("clips candidates to the viewport and caps one capture at 150 regions", () => {
    const elements = Array.from({ length: 170 }, (_, index) =>
      new FakeElement({ rect: rect(-10, index * 4, 60, 20), text: `Candidate ${index}` }),
    );

    const candidates = collectCaptureCandidates(fakeDocument(elements), {
      height: 801,
      width: 1200,
    });

    expect(candidates).toHaveLength(150);
    expect(candidates[0]).toEqual({
      height: 20,
      id: "r:0:0:50:20",
      width: 50,
      x: 0,
      y: 0,
    });
  });
});

class FakePrivacyElement {
  readonly attributes = new Map<string, string>();
  readonly children: FakePrivacyElement[];
  readonly tagName: string;
  readonly textContent: string;

  constructor(options: {
    attributes?: Record<string, string>;
    children?: FakePrivacyElement[];
    tagName?: string;
    text?: string;
  } = {}) {
    this.children = options.children ?? [];
    this.tagName = (options.tagName ?? "DIV").toUpperCase();
    this.textContent = options.text ?? this.children.map((child) => child.textContent).join("");
    for (const [name, value] of Object.entries(options.attributes ?? {})) {
      this.attributes.set(name, value);
    }
  }

  getAttribute(name: string): string | null {
    return this.attributes.get(name) ?? null;
  }

  hasAttribute(name: string): boolean {
    return this.attributes.has(name);
  }

  querySelectorAll(): FakePrivacyElement[] {
    return this.children.flatMap((child) => [child, ...child.querySelectorAll()]);
  }

  removeAttribute(name: string): void {
    this.attributes.delete(name);
  }

  setAttribute(name: string, value: string): void {
    this.attributes.set(name, value);
  }
}

function privacyDocument(options: {
  composerProjects?: FakePrivacyElement[];
  emptyStateProjects?: FakePrivacyElement[];
  profiles?: FakePrivacyElement[];
  projects?: FakePrivacyElement[];
  threads?: FakePrivacyElement[];
}): Document {
  return {
    querySelectorAll(selector: string): FakePrivacyElement[] {
      if (selector.includes("sidebar-thread-row")) return options.threads ?? [];
      if (selector.includes("sidebar-project-row")) return options.projects ?? [];
      if (selector.includes("button.sidebar-item")) return options.profiles ?? [];
      if (selector.includes("workspace-project")) return options.composerProjects ?? [];
      if (selector.includes('data-feature="game-source"')) return options.emptyStateProjects ?? [];
      return [];
    },
  } as unknown as Document;
}

describe("Codex privacy placeholders", () => {
  test("marks thread, project, profile name, and avatar while leaving navigation copy intact", () => {
    const threadTitle = new FakePrivacyElement({ tagName: "SPAN", text: "Private thread" });
    const thread = new FakePrivacyElement({
      attributes: { "data-app-action-sidebar-thread-title": "Private thread" },
      children: [threadTitle],
    });
    const projectLabel = new FakePrivacyElement({ tagName: "SPAN", text: "secret-project" });
    const project = new FakePrivacyElement({
      attributes: { "data-app-action-sidebar-project-label": "secret-project" },
      children: [projectLabel],
    });
    const avatar = new FakePrivacyElement({ tagName: "IMG" });
    const profileName = new FakePrivacyElement({ tagName: "SPAN", text: "Kid" });
    const profile = new FakePrivacyElement({ children: [avatar, profileName], tagName: "BUTTON" });
    const navigation = new FakePrivacyElement({ tagName: "SPAN", text: "New chat" });

    const restore = markCodexPrivacyPlaceholders(
      privacyDocument({ profiles: [profile], projects: [project], threads: [thread] }),
    );

    expect(threadTitle.getAttribute("data-incodex-capture-redact")).toBe("text");
    expect(projectLabel.getAttribute("data-incodex-capture-redact")).toBe("text");
    expect(profileName.getAttribute("data-incodex-capture-redact")).toBe("text");
    expect(profile.getAttribute("data-incodex-capture-redact-profile")).toBe("");
    expect(navigation.hasAttribute("data-incodex-capture-redact")).toBe(false);

    restore();
    expect(threadTitle.hasAttribute("data-incodex-capture-redact")).toBe(false);
    expect(projectLabel.hasAttribute("data-incodex-capture-redact")).toBe(false);
    expect(profileName.hasAttribute("data-incodex-capture-redact")).toBe(false);
    expect(profile.hasAttribute("data-incodex-capture-redact-profile")).toBe(false);
  });

  test("masks project names repeated in the composer and empty-state heading", () => {
    const composerLabel = new FakePrivacyElement({ tagName: "SPAN", text: "cavalrycn" });
    const composerProject = new FakePrivacyElement({ children: [composerLabel], tagName: "BUTTON" });
    const emptyStateProject = new FakePrivacyElement({ tagName: "BUTTON", text: "cavalrycn" });

    const restore = markCodexPrivacyPlaceholders(
      privacyDocument({
        composerProjects: [composerProject],
        emptyStateProjects: [emptyStateProject],
      }),
    );

    expect(composerLabel.getAttribute("data-incodex-capture-redact")).toBe("project");
    expect(emptyStateProject.getAttribute("data-incodex-capture-redact")).toBe("project");

    restore();
    expect(composerLabel.hasAttribute("data-incodex-capture-redact")).toBe(false);
    expect(emptyStateProject.hasAttribute("data-incodex-capture-redact")).toBe(false);
  });

  test("restores pre-existing marker values instead of deleting host state", () => {
    const title = new FakePrivacyElement({
      attributes: { "data-incodex-capture-redact": "existing" },
      tagName: "SPAN",
      text: "Private thread",
    });
    const thread = new FakePrivacyElement({
      attributes: { "data-app-action-sidebar-thread-title": "Private thread" },
      children: [title],
    });

    const restore = markCodexPrivacyPlaceholders(privacyDocument({ threads: [thread] }));
    expect(title.getAttribute("data-incodex-capture-redact")).toBe("text");
    restore();
    expect(title.getAttribute("data-incodex-capture-redact")).toBe("existing");
  });

});
