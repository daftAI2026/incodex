import { describe, expect, test } from "bun:test";
import { createNativeAccessibilitySetupWindow } from "./incodex-accessibility-native.cts";

const APP_PATH = "/Applications/ChatGPT.app";

type Frame = {
  origin: { x: number; y: number };
  size: { width: number; height: number };
};

const COPY = {
  title: "Enable Codex Computer Use",
  body: "Codex Computer Use needs these permissions to use apps on your Mac.",
  permissionTitle: "Accessibility",
  permissionDescription: "Allows Codex to access app interfaces",
  repair: "Allow",
  later: "Later",
  addedTitle: "Allow Codex in System Settings",
  addedBody: "Drag Codex into Accessibility and wait for the automatic check.",
  openSettings: "Open Settings",
  checking: "Checking automatically",
  repairing: "Preparing System Settings…",
};

function frame(width = 0, height = 0, x = 0, y = 0): Frame {
  return { origin: { x, y }, size: { width, height } };
}

function copyFrame(value: Frame): Frame {
  return frame(value.size.width, value.size.height, value.origin.x, value.origin.y);
}

type NativeCall = { receiver: string; selector: string; args: unknown[] };

class FakeNative {
  readonly subviews: FakeNative[] = [];
  readonly calls: NativeCall[];
  readonly selectors = new Map<string, (...args: any[]) => unknown>();
  readonly values = new Map<string, unknown>();
  frameValue = frame();
  contentViewValue: FakeNative | null = null;
  target: FakeNative | null = null;
  action: string | null = null;
  visible = false;
  destroyed = false;

  constructor(
    readonly type: string,
    calls: NativeCall[],
    methods: Record<string, (...args: any[]) => unknown> = {},
  ) {
    this.calls = calls;
    for (const [selector, implementation] of Object.entries(methods)) {
      this.selectors.set(selector.replaceAll(":", "$"), implementation.bind(this));
    }
  }

  record(selector: string, ...args: unknown[]): void {
    this.calls.push({ receiver: this.type, selector, args });
  }

  invoke(selector: string, ...args: any[]): unknown {
    const method = this.selectors.get(selector.replaceAll(":", "$"));
    if (!method) throw new Error(`${this.type} does not implement ${selector}`);
    return method(...args);
  }

  hasSelector(selector: string): boolean {
    return this.selectors.has(selector.replaceAll(":", "$"));
  }

  initWithContentRect$styleMask$backing$defer$(value: Frame): FakeNative {
    this.record("initWithContentRect:styleMask:backing:defer:", value);
    this.frameValue = copyFrame(value);
    return this;
  }

  initWithFrame$(value: Frame): FakeNative {
    this.record("initWithFrame:", value);
    this.frameValue = copyFrame(value);
    return this;
  }

  initWithString$(value: string): FakeNative {
    this.values.set("string", value);
    return this;
  }

  frame(): Frame {
    return copyFrame(this.frameValue);
  }

  bounds(): Frame {
    return frame(this.frameValue.size.width, this.frameValue.size.height);
  }

  setFrame$(value: Frame): void {
    this.record("setFrame:", value);
    this.frameValue = copyFrame(value);
  }

  setContentView$(value: FakeNative): void {
    this.record("setContentView:", value);
    this.contentViewValue = value;
    if (!this.subviews.includes(value)) this.subviews.push(value);
  }

  contentView(): FakeNative | null {
    return this.contentViewValue;
  }

  addSubview$(value: FakeNative): void {
    this.record("addSubview:", value);
    this.subviews.push(value);
  }

  setTarget$(value: FakeNative): void {
    this.target = value;
  }

  setAction$(value: string): void {
    this.action = value;
  }

  performClick$(): void {
    if (!this.target || !this.action) throw new Error("button has no target/action");
    this.target.invoke(this.action, this);
  }

  setTitle$(value: unknown): void {
    this.values.set("title", String(value));
  }

  setStringValue$(value: unknown): void {
    this.values.set("stringValue", String(value));
  }

  setImage$(value: unknown): void {
    this.values.set("image", value);
  }

  setImageScaling$(value: unknown): void {
    this.values.set("imageScaling", value);
  }

  setFrameOrigin$(value: unknown): void {
    this.values.set("frameOrigin", value);
  }

  setReleasedWhenClosed$(value: boolean): void {
    this.values.set("releasedWhenClosed", value);
  }

  setStyleMask$(value: unknown): void {
    this.values.set("styleMask", value);
  }

  styleMask(): unknown {
    return this.values.get("styleMask");
  }

  setVisible$(value: boolean): void {
    this.visible = value;
  }

  orderFront$(): void {
    this.record("orderFront:");
    this.visible = true;
  }

  orderOut$(): void {
    this.record("orderOut:");
    this.visible = false;
  }

  show(): void {
    this.visible = true;
  }

  close(): void {
    this.record("close");
    this.visible = false;
    this.destroyed = true;
  }

  isVisible(): boolean {
    return this.visible;
  }

  isDestroyed(): boolean {
    return this.destroyed;
  }

  bitmapImageRepForCachingDisplayInRect$(value: Frame): FakeNative {
    this.record("bitmapImageRepForCachingDisplayInRect:", value);
    return new FakeNative("NSBitmapImageRep", this.calls);
  }

  cacheDisplayInRect$toBitmapImageRep$(value: Frame, representation: FakeNative): void {
    this.record("cacheDisplayInRect:toBitmapImageRep:", value, representation);
  }

  setDataProvider$forTypes$(provider: FakeNative, types: unknown): void {
    this.values.set("dataProvider", provider);
    this.values.set("types", types);
  }

  setString$forType$(value: string, type: string): void {
    this.values.set(`pasteboard:${type}`, value);
  }

  beginDraggingSessionWithItems$event$source$(items: FakeNative[], event: unknown, source: FakeNative): FakeNative {
    this.values.set("draggingItems", items);
    this.values.set("draggingEvent", event);
    this.values.set("draggingSource", source);
    return new FakeNative("NSDraggingSession", this.calls);
  }

  setDraggingFrame$contents$(value: Frame, image: unknown): void {
    this.values.set("draggingFrame", value);
    this.values.set("draggingImage", image);
  }
}

type FakeBridge = {
  objc: Record<string, unknown>;
  calls: NativeCall[];
  objects: FakeNative[];
};

function makeBridge(): FakeBridge {
  const calls: NativeCall[] = [];
  const objects: FakeNative[] = [];
  const definitions = new Map<string, Record<string, (...args: any[]) => unknown>>();

  function object(type: string, methods: Record<string, (...args: any[]) => unknown> = {}): FakeNative {
    const value = new FakeNative(type, calls, methods);
    objects.push(value);
    return value;
  }

  function classObject(type: string): any {
    const methods = definitions.get(type) ?? {};
    const cls: Record<string, unknown> = {
      alloc: () => object(type, methods),
      buttonWithTitle$target$action$: (title: unknown, target: FakeNative, action: string) => {
        const button = object(type);
        button.setTitle$(title);
        button.setTarget$(target);
        button.setAction$(action);
        return button;
      },
      buttonWithImage$target$action$: (image: unknown, target: FakeNative, action: string) => {
        const button = object(type);
        button.setImage$(image);
        button.setTarget$(target);
        button.setAction$(action);
        return button;
      },
      stringWithUTF8String$: (value: string) => value,
      imageNamed$: (value: string) => value,
      sharedWorkspace: () => object(type),
      defaultCenter: () => object(type),
    };
    return new Proxy(cls, {
      get(target, property) {
        if (property in target) return target[property as string];
        return (...args: unknown[]) => {
          const value = object(type);
          value.values.set(String(property), args.length <= 1 ? args[0] : args);
          return value;
        };
      },
    });
  }

  const libraryClasses = new Map<string, any>();
  const library = (framework: string): any => new Proxy({}, {
    get(_target, property: string) {
      if (!libraryClasses.has(`${framework}:${property}`)) {
        libraryClasses.set(`${framework}:${property}`, classObject(String(property)));
      }
      return libraryClasses.get(`${framework}:${property}`);
    },
  });

  const objc = {
    NobjcLibrary: class {
      constructor(readonly framework: string) {}
      get NSPanel() { return library(this.framework).NSPanel; }
      get NSVisualEffectView() { return library(this.framework).NSVisualEffectView; }
      get NSImageView() { return library(this.framework).NSImageView; }
      get NSTextField() { return library(this.framework).NSTextField; }
      get NSButton() { return library(this.framework).NSButton; }
      get NSWorkspace() { return library(this.framework).NSWorkspace; }
      get NSPasteboardItem() { return library(this.framework).NSPasteboardItem; }
      get NSDraggingItem() { return library(this.framework).NSDraggingItem; }
      get NSString() { return library(this.framework).NSString; }
      get NSColor() { return library(this.framework).NSColor; }
    },
    NobjcClass: {
      define(definition: { name: string; methods?: Record<string, { implementation: (...args: any[]) => unknown }> }) {
        const methods = Object.fromEntries(
          Object.entries(definition.methods ?? {}).map(([selector, value]) => [
            selector,
            (...args: any[]) => value.implementation(...args),
          ]),
        );
        definitions.set(definition.name, methods);
        return classObject(definition.name);
      },
      super(self: FakeNative): FakeNative {
        return self;
      },
    },
    typedBlock(_signature: unknown, callback: (...args: any[]) => unknown) {
      return callback;
    },
    callFunction(name: string, ...args: unknown[]) {
      calls.push({ receiver: "C", selector: name, args });
      if (name === "NSSelectorFromString") return String(args.at(-1));
      return undefined;
    },
    RunLoop: { run: () => () => {}, stop: () => {} },
  };

  return { objc, calls, objects };
}

function descendants(root: FakeNative): FakeNative[] {
  return [root, ...root.subviews.flatMap(descendants)];
}

function objectWithTitle(root: FakeNative, title: string): FakeNative | undefined {
  return descendants(root).find((value) =>
    value.values.get("title") === title || value.values.get("stringValue") === title,
  );
}

async function makeHarness(options: { onHandoff?: (payload: any) => void } = {}) {
  const bridge = makeBridge();
  const api = await createNativeAccessibilitySetupWindow({
    appPath: APP_PATH,
    copy: COPY,
    loadObjcModule: async () => bridge.objc,
    locateSettings: () => ({ x: 120, y: 140, width: 920, height: 700 }),
    onHandoff: options.onHandoff,
  });
  const panel = bridge.objects.find((value) => value.type === "NSPanel");
  if (!panel) throw new Error("native Accessibility panel was not created");
  return { api, bridge, panel };
}

describe("native Accessibility setup adapter", () => {
  test("uses the reference hierarchy in the initial AppKit panel with one centered icon and one permission card", async () => {
    const { panel } = await makeHarness();
    const tree = descendants(panel);
    const icon = tree.find((value) => value.type === "NSImageView");
    const content = panel.contentViewValue;
    if (!icon || !content) throw new Error("initial native panel content is missing");

    expect(panel.isVisible()).toBe(true);
    expect(icon.frame().size).toEqual({ width: 64, height: 64 });
    expect(icon.frame().origin.x + icon.frame().size.width / 2).toBeCloseTo(
      content.bounds().size.width / 2,
    );
    expect(objectWithTitle(panel, COPY.title)).toBeDefined();
    expect(objectWithTitle(panel, COPY.body)).toBeDefined();
    expect(objectWithTitle(panel, COPY.permissionTitle)).toBeDefined();
    expect(objectWithTitle(panel, COPY.permissionDescription)).toBeDefined();
    expect(objectWithTitle(panel, "Screenshots")).toBeUndefined();
    expect(objectWithTitle(panel, COPY.repair)).toBeDefined();
  });

  test("captures pending source geometry and native snapshot before entering the 532x112 helper", async () => {
    const handoffs: any[] = [];
    const { api, bridge, panel } = await makeHarness({
      onHandoff: (payload) => handoffs.push(payload),
    });
    const repair = objectWithTitle(panel, COPY.repair);
    if (!repair) throw new Error("native repair button is missing");

    repair.performClick$();
    await expect(api.choice).resolves.toBe("repair");
    api.setState("awaiting-user");

    expect(bridge.calls.some(({ selector }) => selector === "cacheDisplayInRect:toBitmapImageRep:")).toBe(true);
    expect(handoffs).toHaveLength(1);
    expect(handoffs[0].source.frame.size.width).toBeGreaterThan(0);
    expect(handoffs[0].source.frame.size.height).toBeGreaterThan(0);
    expect(handoffs[0].source.image).toBeDefined();
    expect(handoffs[0].target.frame.size).toEqual({ width: 532, height: 112 });
    expect(handoffs[0].target.panel).toBeDefined();
    expect(handoffs[0].target.view).toBeDefined();
  });

  test("uses a native file URL drag source fixed to the official ChatGPT bundle", async () => {
    const { api, bridge } = await makeHarness();
    api.setState("awaiting-user");
    const row = bridge.objects.find((value) => value.hasSelector("mouseDown:"));
    if (!row) throw new Error("native helper drag row is missing");

    row.invoke("mouseDown:", {});

    const item = bridge.objects.find((value) => value.type === "NSPasteboardItem");
    if (!item) throw new Error("native drag pasteboard item is missing");
    const provider = item.values.get("dataProvider") as FakeNative | undefined;
    provider?.invoke("pasteboard:item:provideDataForType:", null, item, "public.file-url");
    expect(item.values.get("pasteboard:public.file-url")).toBe(`file://${APP_PATH}`);
    expect(item.values.get("types")).toEqual(expect.arrayContaining(["public.file-url"]));
  });

  test("closes native panels once and keeps the controller lifecycle contract", async () => {
    const { api, panel } = await makeHarness();
    let closed = 0;
    api.onClose(() => { closed += 1; });

    api.close();
    api.close();

    expect(api.isDestroyed()).toBe(true);
    expect(panel.isDestroyed()).toBe(true);
    expect(closed).toBe(1);
  });
});
