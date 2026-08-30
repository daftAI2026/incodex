import { describe, expect, test } from "bun:test";
import {
  createDockMenuController,
  normalizeDockMenuLabel,
} from "./incodex-dock-menu.cts";

type MenuItemOptions = {
  click?: () => void;
  enabled?: boolean;
  id?: string;
  label?: string;
  type?: string;
};

class FakeMenuItem {
  click?: () => void;
  enabled: boolean;
  id?: string;
  label?: string;
  type?: string;

  constructor(options: MenuItemOptions) {
    this.click = options.click;
    this.enabled = options.enabled !== false;
    this.id = options.id;
    this.label = options.label;
    this.type = options.type;
  }
}

class FakeMenu {
  items: FakeMenuItem[] = [];

  insert(position: number, item: FakeMenuItem): void {
    this.items.splice(position, 0, item);
  }

  getMenuItemById(id: string): FakeMenuItem | null {
    return this.items.find((item) => item.id === id) ?? null;
  }
}

class FakeDock {
  current: FakeMenu | null = null;
  setCalls: FakeMenu[] = [];

  getMenu(): FakeMenu | null {
    return this.current;
  }

  setMenu(menu: FakeMenu): void {
    this.current = menu;
    this.setCalls.push(menu);
  }
}

function officialMenu(label: string): FakeMenu {
  const menu = new FakeMenu();
  menu.insert(0, new FakeMenuItem({ label }));
  return menu;
}

function itemIds(menu: FakeMenu): Array<string | undefined> {
  return menu.items.map((item) => item.id);
}

describe("macOS Dock menu decoration", () => {
  test("preserves the current official menu and launches through the existing action", () => {
    const dock = new FakeDock();
    dock.setMenu(officialMenu("Recent thread"));
    let opens = 0;
    const controller = createDockMenuController({
      dock,
      Menu: FakeMenu,
      MenuItem: FakeMenuItem,
      isIncognito: false,
      onOpen: () => {
        opens += 1;
      },
      log: () => {},
    });

    expect(controller.configure("Open incognito window")).toBe(true);
    expect(dock.current?.items.map((item) => item.label)).toEqual([
      "Open incognito window",
      undefined,
      "Recent thread",
    ]);
    expect(dock.current?.items[0]?.enabled).toBe(true);
    dock.current?.items[0]?.click?.();
    expect(opens).toBe(1);
  });

  test("decorates every later official replacement without duplicating its own items", () => {
    const dock = new FakeDock();
    const controller = createDockMenuController({
      dock,
      Menu: FakeMenu,
      MenuItem: FakeMenuItem,
      isIncognito: false,
      onOpen: () => {},
      log: () => {},
    });
    controller.configure("Open incognito window");

    const refreshed = officialMenu("Unread thread");
    dock.setMenu(refreshed);
    dock.setMenu(refreshed);
    controller.configure("Open incognito window");

    expect(refreshed.items.map((item) => item.label)).toEqual([
      "Open incognito window",
      undefined,
      "Unread thread",
    ]);
    expect(itemIds(refreshed).filter((id) => id === "incodex-open-incognito")).toHaveLength(1);
    expect(itemIds(refreshed).filter((id) => id === "incodex-menu-separator")).toHaveLength(1);
  });

  test("shows a disabled identity item in an incognito child", () => {
    const dock = new FakeDock();
    let opens = 0;
    const controller = createDockMenuController({
      dock,
      Menu: FakeMenu,
      MenuItem: FakeMenuItem,
      isIncognito: true,
      onOpen: () => {
        opens += 1;
      },
      log: () => {},
    });

    expect(controller.configure("Incognito window")).toBe(true);
    expect(dock.current?.items).toHaveLength(1);
    expect(dock.current?.items[0]).toMatchObject({
      enabled: false,
      id: "incodex-incognito-identity",
      label: "Incognito window",
    });
    dock.current?.items[0]?.click?.();
    expect(opens).toBe(0);
  });

  test("fails open when Electron menu construction is unavailable", () => {
    const dock = new FakeDock();
    const errors: string[] = [];
    const controller = createDockMenuController({
      dock,
      Menu: FakeMenu,
      MenuItem: class BrokenMenuItem {
        constructor() {
          throw new Error("menu unavailable");
        }
      },
      isIncognito: false,
      onOpen: () => {},
      log: (event: string) => errors.push(event),
    });
    controller.configure("Open incognito window");

    const official = officialMenu("Official item");
    expect(() => dock.setMenu(official)).not.toThrow();
    expect(dock.current).toBe(official);
    expect(official.items.map((item) => item.label)).toEqual(["Official item"]);
    expect(errors).toContain("dock-menu-decoration-failed");
  });
});

describe("Dock menu label validation", () => {
  test("accepts localized text and rejects control or unbounded input", () => {
    expect(normalizeDockMenuLabel("  打开无痕窗口  ")).toBe("打开无痕窗口");
    expect(normalizeDockMenuLabel("\nOpen incognito")).toBeNull();
    expect(normalizeDockMenuLabel("x".repeat(81))).toBeNull();
    expect(normalizeDockMenuLabel(42)).toBeNull();
  });
});
