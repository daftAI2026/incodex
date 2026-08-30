import { describe, expect, test } from "bun:test";
import {
  createStatusMenuController,
  normalizeStatusMenuLabel,
} from "./incodex-status-menu.cts";

type FakeItem = {
  enabled?: boolean;
  id?: string;
  label?: string;
  onSelect?: () => void;
  type?: "normal" | "separator";
};

type FakeMenu = {
  codex: boolean;
  items: FakeItem[];
};

function createFakeBridge() {
  let menuOpened: ((menu: FakeMenu) => void) | null = null;
  let disposed = false;

  return {
    bridge: {
      findItem(menu: FakeMenu, id: string): FakeItem | null {
        return menu.items.find((item) => item.id === id) ?? null;
      },
      insertItem(menu: FakeMenu, index: number, item: FakeItem): void {
        menu.items.splice(index, 0, { ...item });
      },
      isCodexStatusMenu(menu: FakeMenu): boolean {
        return menu.codex;
      },
      itemCount(menu: FakeMenu): number {
        return menu.items.length;
      },
      observeMenuOpen(handler: (menu: FakeMenu) => void): () => void {
        menuOpened = handler;
        return () => {
          disposed = true;
          menuOpened = null;
        };
      },
      updateItem(item: FakeItem, update: FakeItem): void {
        Object.assign(item, update);
      },
    },
    disposeCalled(): boolean {
      return disposed;
    },
    open(menu: FakeMenu): void {
      menuOpened?.(menu);
    },
  };
}

function officialMenu(...labels: string[]): FakeMenu {
  return {
    codex: true,
    items: labels.map((label) => ({ label, type: "normal" })),
  };
}

describe("macOS status menu decoration", () => {
  test("adds one localized action to the existing Codex status menu", async () => {
    const fake = createFakeBridge();
    let opens = 0;
    const controller = createStatusMenuController({
      loadBridge: async () => fake.bridge,
      isIncognito: false,
      onOpen: () => {
        opens += 1;
      },
      log: () => {},
    });

    expect(await controller.configure("打开无痕窗口")).toBe(true);
    const menu = officialMenu("New Chat", "Quit ChatGPT");
    fake.open(menu);

    expect(menu.items.map((item) => item.label)).toEqual([
      "打开无痕窗口",
      undefined,
      "New Chat",
      "Quit ChatGPT",
    ]);
    menu.items[0]?.onSelect?.();
    expect(opens).toBe(1);
  });

  test("ignores unrelated application menus", async () => {
    const fake = createFakeBridge();
    const controller = createStatusMenuController({
      loadBridge: async () => fake.bridge,
      isIncognito: false,
      onOpen: () => {},
      log: () => {},
    });
    await controller.configure("Open incognito window");
    const menu = { codex: false, items: [{ label: "Quit ChatGPT" }] };

    fake.open(menu);

    expect(menu.items).toEqual([{ label: "Quit ChatGPT" }]);
  });

  test("updates later menus without duplicating its action or separator", async () => {
    const fake = createFakeBridge();
    const controller = createStatusMenuController({
      loadBridge: async () => fake.bridge,
      isIncognito: false,
      onOpen: () => {},
      log: () => {},
    });
    await controller.configure("Open incognito window");
    const menu = officialMenu("Quit ChatGPT");

    fake.open(menu);
    await controller.configure("打开无痕窗口");
    fake.open(menu);

    expect(menu.items.filter((item) => item.id === "incodex-open-incognito")).toHaveLength(1);
    expect(menu.items.filter((item) => item.id === "incodex-status-menu-separator")).toHaveLength(1);
    expect(menu.items[0]?.label).toBe("打开无痕窗口");
  });

  test("shows a disabled identity in an incognito child", async () => {
    const fake = createFakeBridge();
    let opens = 0;
    const controller = createStatusMenuController({
      loadBridge: async () => fake.bridge,
      isIncognito: true,
      onOpen: () => {
        opens += 1;
      },
      log: () => {},
    });
    await controller.configure("无痕窗口");
    const menu = officialMenu("Quit ChatGPT");

    fake.open(menu);
    menu.items[0]?.onSelect?.();

    expect(menu.items[0]).toMatchObject({
      enabled: false,
      id: "incodex-incognito-identity",
      label: "无痕窗口",
    });
    expect(opens).toBe(0);
  });

  test("fails open when native menu decoration throws", async () => {
    const fake = createFakeBridge();
    const errors: string[] = [];
    fake.bridge.insertItem = () => {
      throw new Error("native menu unavailable");
    };
    const controller = createStatusMenuController({
      loadBridge: async () => fake.bridge,
      isIncognito: false,
      onOpen: () => {},
      log: (event: string) => errors.push(event),
    });
    await controller.configure("Open incognito window");
    const menu = officialMenu("Quit ChatGPT");

    expect(() => fake.open(menu)).not.toThrow();
    expect(menu.items.map((item) => item.label)).toEqual(["Quit ChatGPT"]);
    expect(errors).toContain("status-menu-decoration-failed");
  });

  test("reports unavailable bridges and releases native observation", async () => {
    const unavailable = createStatusMenuController({
      loadBridge: async () => {
        throw new Error("objc unavailable");
      },
      isIncognito: false,
      onOpen: () => {},
      log: () => {},
    });
    expect(await unavailable.configure("Open incognito window")).toBe(false);

    const fake = createFakeBridge();
    const controller = createStatusMenuController({
      loadBridge: async () => fake.bridge,
      isIncognito: false,
      onOpen: () => {},
      log: () => {},
    });
    expect(await controller.configure("Open incognito window")).toBe(true);
    controller.dispose();
    expect(fake.disposeCalled()).toBe(true);
  });
});

describe("status menu label validation", () => {
  test("accepts localized text and rejects control or unbounded input", () => {
    expect(normalizeStatusMenuLabel("  打开无痕窗口  ")).toBe("打开无痕窗口");
    expect(normalizeStatusMenuLabel("\nOpen incognito")).toBeNull();
    expect(normalizeStatusMenuLabel("x".repeat(81))).toBeNull();
    expect(normalizeStatusMenuLabel(42)).toBeNull();
  });
});
