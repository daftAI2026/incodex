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
  visibleFrameValue = frame();
  contentViewValue: FakeNative | null = null;
  layerValue: FakeNative | null = null;
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

  initWithRect$options$owner$userInfo$(value: Frame, options: unknown, owner: unknown, userInfo: unknown): FakeNative {
    this.record("initWithRect:options:owner:userInfo:", value, options, owner, userInfo);
    this.frameValue = copyFrame(value);
    return this;
  }

  initWithString$(value: string): FakeNative {
    this.values.set("string", value);
    return this;
  }

  init(): FakeNative {
    return this;
  }

  initWithPasteboardWriter$(value: FakeNative): FakeNative {
    this.values.set("pasteboardWriter", value);
    return this;
  }

  initWithContentsOfFile$(value: unknown): FakeNative {
    this.values.set("contentsOfFile", value);
    return this;
  }

  initWithSize$(value: unknown): FakeNative {
    this.values.set("size", value);
    return this;
  }

  addRepresentation$(value: unknown): void {
    this.values.set("representation", value);
  }

  count(): number {
    return Array.isArray(this.values.get("items")) ? (this.values.get("items") as unknown[]).length : 0;
  }

  objectAtIndex$(index: number): unknown {
    const items = this.values.get("items");
    return Array.isArray(items) ? items[index] : undefined;
  }

  frame(): Frame {
    return copyFrame(this.frameValue);
  }

  bounds(): Frame {
    return frame(this.frameValue.size.width, this.frameValue.size.height);
  }

  convertRect$toView$(value: Frame, _view: unknown): Frame {
    this.record("convertRect:toView:", value, _view);
    return copyFrame(value);
  }

  convertRectToScreen$(value: Frame): Frame {
    this.record("convertRectToScreen:", value);
    return copyFrame(value);
  }

  visibleFrame(): Frame {
    return copyFrame(this.visibleFrameValue.size.width || this.frameValue.size.width
      ? this.visibleFrameValue : this.frameValue);
  }

  displayIfNeeded(): void {}

  sizeToFit(): void {}

  layer(): FakeNative {
    if (!this.layerValue) this.layerValue = new FakeNative("CALayer", this.calls);
    return this.layerValue;
  }

  presentationLayer(): FakeNative {
    return this;
  }

  transform(): unknown {
    return this.values.get("transform");
  }

  settlingDuration(): number {
    return 0;
  }

  setFrame$(value: Frame): void {
    this.record("setFrame:", value);
    this.frameValue = copyFrame(value);
  }

  accessibilityDisplayShouldReduceMotion(): boolean { return false; }
  accessibilityDisplayShouldReduceTransparency(): boolean { return false; }

  effectiveAppearance(): FakeNative {
    return new FakeNative("NSAppearance", this.calls);
  }

  name(): string {
    return "NSAppearanceNameAqua";
  }

  absoluteString(): string {
    const path = String(this.values.get("fileURLPath") ?? "");
    return path.startsWith("file://") ? path : `file://${path}`;
  }

  setContentView$(value: FakeNative): void {
    this.record("setContentView:", value);
    this.contentViewValue = value;
    if (!this.subviews.includes(value)) this.subviews.push(value);
  }

  setDelegate$(value: FakeNative): void {
    this.values.set("delegate", value);
  }

  setHidesOnDeactivate$(value: unknown): void {
    this.values.set("hidesOnDeactivate", value);
  }

  setLevel$(value: unknown): void {
    this.values.set("level", value);
  }

  setOpaque$(value: unknown): void {
    this.values.set("opaque", value);
  }

  setBackgroundColor$(value: unknown): void {
    this.values.set("backgroundColor", value);
  }

  setHasShadow$(value: unknown): void {
    this.values.set("hasShadow", value);
  }

  setIgnoresMouseEvents$(value: unknown): void {
    this.values.set("ignoresMouseEvents", value);
  }

  setTitlebarAppearsTransparent$(value: unknown): void {
    this.values.set("titlebarAppearsTransparent", value);
  }

  setTitleVisibility$(value: unknown): void {
    this.values.set("titleVisibility", value);
  }

  setWantsLayer$(value: unknown): void {
    this.values.set("wantsLayer", value);
  }

  setMaterial$(value: unknown): void {
    this.values.set("material", value);
  }

  setBlendingMode$(value: unknown): void {
    this.values.set("blendingMode", value);
  }

  setState$(value: unknown): void {
    this.values.set("state", value);
  }

  setCornerRadius$(value: unknown): void {
    this.values.set("cornerRadius", value);
  }

  setBorderWidth$(value: unknown): void {
    this.values.set("borderWidth", value);
  }

  setBorderColor$(value: unknown): void {
    this.values.set("borderColor", value);
  }

  setShadowOpacity$(value: unknown): void {
    this.values.set("shadowOpacity", value);
  }

  setShadowRadius$(value: unknown): void {
    this.values.set("shadowRadius", value);
  }

  setShadowOffset$(value: unknown): void {
    this.values.set("shadowOffset", value);
  }

  setShadowColor$(value: unknown): void {
    this.values.set("shadowColor", value);
  }

  setGeometryFlipped$(value: unknown): void {
    this.values.set("geometryFlipped", value);
  }

  setMasksToBounds$(value: unknown): void {
    this.values.set("masksToBounds", value);
  }

  setAnchorPoint$(value: unknown): void {
    this.values.set("anchorPoint", value);
  }

  setTransform$(value: unknown): void {
    this.values.set("transform", value);
  }

  addAnimation$forKey$(value: unknown, key: unknown): void {
    this.values.set(`animation:${String(key)}`, value);
  }

  setFill(): void {}

  setStroke(): void {}

  setFrame$display$(value: Frame): void {
    this.setFrame$(value);
  }

  addChildWindow$ordered$(value: FakeNative): void {
    this.addSubview$(value);
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

  setValue$forKey$(value: unknown, key: string): void { this.values.set(key, value); }

  setStringValue$(value: unknown): void {
    this.values.set("stringValue", String(value));
  }

  setFont$(value: unknown): void {
    this.values.set("font", value);
  }

  setAlignment$(value: unknown): void {
    this.values.set("alignment", value);
  }

  setTextColor$(value: unknown): void {
    this.values.set("textColor", value);
  }

  setMaximumNumberOfLines$(value: unknown): void {
    this.values.set("maximumNumberOfLines", value);
  }

  setLineBreakMode$(value: unknown): void {
    this.values.set("lineBreakMode", value);
  }

  setKeyEquivalent$(value: unknown): void {
    this.values.set("keyEquivalent", value);
  }

  setEnabled$(value: unknown): void {
    this.values.set("enabled", value);
  }

  setBezelStyle$(value: unknown): void {
    this.values.set("bezelStyle", value);
  }

  setBordered$(value: unknown): void { this.values.set("bordered", value); }
  setContentTintColor$(value: unknown): void { this.values.set("contentTintColor", value); }

  setToolTip$(value: unknown): void {
    this.values.set("toolTip", value);
  }

  setHidden$(value: unknown): void {
    this.values.set("hidden", value);
  }

  addTrackingArea$(value: unknown): void {
    this.values.set("trackingArea", value);
  }

  setAnimatesToStartingPositionsOnCancelOrFail$(value: unknown): void {
    this.values.set("animatesToStartingPositionsOnCancelOrFail", value);
  }

  setMass$(value: unknown): void { this.values.set("mass", value); }
  setStiffness$(value: unknown): void { this.values.set("stiffness", value); }
  setDamping$(value: unknown): void { this.values.set("damping", value); }
  setInitialVelocity$(value: unknown): void { this.values.set("initialVelocity", value); }
  setDuration$(value: unknown): void { this.values.set("duration", value); }
  setFromValue$(value: unknown): void { this.values.set("fromValue", value); }
  setToValue$(value: unknown): void { this.values.set("toValue", value); }

  moveToPoint$(value: unknown): void { this.values.set("moveToPoint", value); }
  lineToPoint$(value: unknown): void { this.values.set("lineToPoint", value); }
  curveToPoint$controlPoint1$controlPoint2$(...value: unknown[]): void {
    this.values.set("curveToPoint", value);
  }
  closePath(): void { this.values.set("closedPath", true); }
  setLineWidth$(value: unknown): void { this.values.set("lineWidth", value); }
  setLineJoinStyle$(value: unknown): void { this.values.set("lineJoinStyle", value); }
  fill(): void { this.values.set("filled", true); }
  stroke(): void { this.values.set("stroked", true); }

  setBoxType$(value: unknown): void { this.values.set("boxType", value); }
  setBorderType$(value: unknown): void { this.values.set("borderType", value); }
  setFillColor$(value: unknown): void { this.values.set("fillColor", value); }

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

  center(): void {
    this.values.set("centered", true);
  }

  makeKeyAndOrderFront$(_value: unknown): void {
    this.visible = true;
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
    representation.values.set("capturedView", this);
  }

  setDataProvider$forTypes$(provider: FakeNative, types: unknown): void {
    this.values.set("dataProvider", provider);
    this.values.set("types", types);
  }

  setString$forType$(value: string, type: string): void {
    this.values.set(`pasteboard:${type}`, value);
  }

  beginDraggingSessionWithItems$event$source$(items: unknown, event: unknown, source: FakeNative): FakeNative {
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
      labelWithString$: (value: unknown) => {
        const field = object(type);
        field.setStringValue$(value);
        return field;
      },
      stringWithUTF8String$: (value: string) => value,
      fileURLWithPath$: (value: unknown) => {
        const url = object(type);
        url.values.set("fileURLPath", String(value));
        return url;
      },
      imageNamed$: (value: string) => value,
      sharedWorkspace: () => object(type),
      defaultCenter: () => object(type),
      screens: () => {
        const screen = object("NSScreen");
        screen.frameValue = frame(1440, 900);
        screen.visibleFrameValue = frame(1440, 860, 0, 0);
        const screens = object("NSArray", {
          count() { return 1; },
          objectAtIndex$(index: number) { return index === 0 ? screen : undefined; },
        });
        screens.values.set("items", [screen]);
        return screens;
      },
      arrayWithObject$: (value: unknown) => {
        const array = object("NSArray", {
          count() { return 1; },
          objectAtIndex$(index: number) { return index === 0 ? value : undefined; },
        });
        array.values.set("items", [value]);
        return array;
      },
      arrayWithArray$: (values: unknown) => {
        const items = values instanceof FakeNative ? values.values.get("items") : values;
        const array = object("NSArray", {
          count() { return Array.isArray(items) ? items.length : 0; },
          objectAtIndex$(index: number) { return Array.isArray(items) ? items[index] : undefined; },
        });
        array.values.set("items", Array.isArray(items) ? [...items] : []);
        return array;
      },
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
      get NSImage() { return library(this.framework).NSImage; }
      get NSTextField() { return library(this.framework).NSTextField; }
      get NSButton() { return library(this.framework).NSButton; }
      get NSFont() { return library(this.framework).NSFont; }
      get NSView() { return library(this.framework).NSView; }
      get NSBezierPath() { return library(this.framework).NSBezierPath; }
      get NSBox() { return library(this.framework).NSBox; }
      get NSScreen() { return library(this.framework).NSScreen; }
      get NSTrackingArea() { return library(this.framework).NSTrackingArea; }
      get NSWorkspace() { return library(this.framework).NSWorkspace; }
      get NSPasteboardItem() { return library(this.framework).NSPasteboardItem; }
      get NSDraggingItem() { return library(this.framework).NSDraggingItem; }
      get NSArray() { return library(this.framework).NSArray; }
      get NSURL() { return library(this.framework).NSURL; }
      get NSString() { return library(this.framework).NSString; }
      get NSColor() { return library(this.framework).NSColor; }
      get NSValue() { return library(this.framework).NSValue; }
      get CASpringAnimation() { return library(this.framework).CASpringAnimation; }
    },
    NobjcClass: {
      define(definition: { name: string; methods?: Record<string, { implementation: (...args: any[]) => unknown }> }) {
        const methods = Object.fromEntries(
          Object.entries(definition.methods ?? {}).map(([selector, value]) => [
            selector,
            function (this: FakeNative, ...args: any[]) {
              return value.implementation(this, ...args);
            },
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
      if (name === "CGColorCreateGenericRGB" && (args[0] as { returns?: string })?.returns === "@") return { color: args.slice(1) };
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

function arrayValues(value: unknown): unknown[] {
  if (value instanceof FakeNative) {
    const items = value.values.get("items");
    return Array.isArray(items) ? items : [];
  }
  return Array.isArray(value) ? value : [];
}

function flushNativeAsync(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

async function settleNativeAsync(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

type PollTimer = { id: number; delay: number; callback: () => void; active: boolean };

function installPollingClock() {
  const originalSetInterval = globalThis.setInterval;
  const originalClearInterval = globalThis.clearInterval;
  let nextId = 1;
  const timers: PollTimer[] = [];
  globalThis.setInterval = ((callback: TimerHandler, delay?: number) => {
    const timer = { id: nextId++, delay: Number(delay ?? 0), callback: callback as () => void, active: true };
    timers.push(timer);
    return timer.id as unknown as ReturnType<typeof setInterval>;
  }) as unknown as typeof setInterval;
  globalThis.clearInterval = ((id?: ReturnType<typeof setInterval>) => {
    const timer = timers.find((entry) => entry.id === Number(id));
    if (timer) timer.active = false;
  }) as unknown as typeof clearInterval;
  return {
    timers,
    restore() {
      globalThis.setInterval = originalSetInterval;
      globalThis.clearInterval = originalClearInterval;
    },
  };
}

function helperPanels(bridge: FakeBridge): FakeNative[] {
  return bridge.objects.filter((value) => value.type.includes("NonactivatingPanel"));
}

async function makeHarness(options: {
  onHandoff?: (payload: any) => void;
  onBack?: (payload: any) => { finished?: Promise<unknown>; dispose?: () => void } | void;
  locateSettings?: () => unknown;
} = {}) {
  const bridge = makeBridge();
  const api = await createNativeAccessibilitySetupWindow({
    appPath: APP_PATH,
    copy: COPY,
    loadObjcModule: async () => bridge.objc,
    locateSettings: options.locateSettings ?? (() => ({ x: 120, y: 140, width: 920, height: 700 })),
    onHandoff: options.onHandoff,
    onBack: options.onBack,
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
    await flushNativeAsync();

    expect(bridge.calls.some(({ selector }) => selector === "cacheDisplayInRect:toBitmapImageRep:")).toBe(true);
    expect(handoffs).toHaveLength(1);
    expect(handoffs[0].source.frame.size.width).toBeGreaterThan(0);
    expect(handoffs[0].source.frame.size.height).toBeGreaterThan(0);
    expect(handoffs[0].source.image).toBeDefined();
    const capture = handoffs[0].source.image.values.get("representation").values.get("capturedView") as FakeNative;
    // The transition must include the colored button background, not just its glyphs.
    expect(capture.subviews.some((view) => view.type === "NSBox")).toBe(true);
    expect(capture.subviews.some((view) => view.type === "NSButton")).toBe(true);
    expect(handoffs[0].target.frame.size).toEqual({ width: 452, height: 44 });
    expect(handoffs[0].target.radius).toBe(8);
    expect(handoffs[0].target.panel).toBeDefined();
    expect(handoffs[0].target.view?.hasSelector("mouseDown:")).toBe(true);
  });

  test("uses a native file URL drag source fixed to the official ChatGPT bundle", async () => {
    const { api, bridge } = await makeHarness();
    api.setState("awaiting-user");
    await flushNativeAsync();
    const row = bridge.objects.find((value) => value.hasSelector("mouseDown:"));
    if (!row) throw new Error("native helper drag row is missing");

    row.invoke("mouseDown:", {});

    const item = bridge.objects.find((value) => value.type === "NSPasteboardItem");
    if (!item) throw new Error("native drag pasteboard item is missing");
    const provider = item.values.get("dataProvider") as FakeNative | undefined;
    provider?.invoke("pasteboard:item:provideDataForType:", null, item, "public.file-url");
    expect(item.values.get("pasteboard:public.file-url")).toBe(`file://${APP_PATH}`);
    expect(arrayValues(item.values.get("types"))).toEqual(expect.arrayContaining(["public.file-url"]));
  });

  test("Back recaptures the original target and waits for a reverse handoff before cleanup", async () => {
    const reverses: any[] = [];
    let finish!: () => void;
    const finished = new Promise<void>((resolve) => { finish = resolve; });
    const { api, bridge, panel } = await makeHarness({
      onBack: (payload) => { reverses.push(payload); return { finished, dispose: () => {} }; },
    });
    try {
      const repair = objectWithTitle(panel, COPY.repair);
      if (!repair) throw new Error("native repair button is missing");
      repair.performClick$();
      await expect(api.choice).resolves.toBe("repair");
      api.setState("awaiting-user");
      await flushNativeAsync();

      const back = bridge.objects.find((value) => value.action === "later:");
      if (!back) throw new Error("native Back button is missing");
      back.performClick$();
      await settleNativeAsync();

      expect(reverses).toHaveLength(1);
      expect(reverses[0].reverse).toBe(true);
      expect(reverses[0].source.frame.size).toEqual({ width: 46, height: 20 });
      expect(reverses[0].source.image).toBeDefined();
      expect(reverses[0].target.frame.size).toEqual({ width: 452, height: 44 });
      expect(api.isDestroyed()).toBe(false);

      finish();
      await settleNativeAsync();
      expect(api.isDestroyed()).toBe(true);
    } finally {
      finish();
      api.close();
    }
  });

  test("disables helper hit testing for an active drag and restores it when the drag ends", async () => {
    const { api, bridge } = await makeHarness();
    api.setState("awaiting-user");
    await flushNativeAsync();
    const row = bridge.objects.find((value) => value.hasSelector("mouseDown:"));
    const helper = helperPanels(bridge).find((value) => value.frame().size.width === 532);
    if (!row || !helper) throw new Error("native drag helper is missing");

    row.invoke("draggingSession:willBeginAtPoint:", { x: 0, y: 0 });
    expect(helper.values.get("ignoresMouseEvents")).toBe(true);
    row.invoke("draggingSession:endedAtPoint:operation:", { x: 0, y: 0 }, 0);
    expect(helper.values.get("ignoresMouseEvents")).toBe(false);
    api.close();
  });

  test("closing during a drag prevents a late drag callback from reviving native UI", async () => {
    const { api, bridge } = await makeHarness();
    api.setState("awaiting-user");
    await flushNativeAsync();
    const row = bridge.objects.find((value) => value.hasSelector("mouseDown:"));
    const helper = helperPanels(bridge).find((value) => value.frame().size.width === 532);
    const appRow = row?.subviews.find((value) => value.type.includes("View"));
    if (!row || !helper || !appRow) throw new Error("native drag helper is missing");

    row.invoke("mouseDown:", {});
    row.invoke("draggingSession:willBeginAtPoint:", { x: 0, y: 0 });
    expect(appRow.values.get("hidden")).toBe(true);
    api.close();
    const callsAfterClose = bridge.calls.length;
    row.invoke("draggingSession:endedAtPoint:operation:", { x: 0, y: 0 }, 0);

    expect(helper.isDestroyed()).toBe(true);
    expect(appRow.values.get("hidden")).toBe(true);
    expect(bridge.calls.slice(callsAfterClose).some(({ selector }) => selector === "orderFront:")).toBe(false);
  });

  test("does not fabricate a helper when System Settings never appears", async () => {
    const clock = installPollingClock();
    let probes = 0;
    const { api, bridge, panel } = await makeHarness({
      locateSettings: () => {
        probes += 1;
        return null;
      },
    });
    try {
      api.setState("awaiting-user");
      await settleNativeAsync();
      const poll = clock.timers.find((timer) => timer.active);
      expect(poll?.delay).toBe(100);
      for (let index = 0; index < 50; index += 1) {
        poll?.callback();
        await settleNativeAsync();
      }

      expect(probes).toBeGreaterThanOrEqual(50);
      expect(helperPanels(bridge)).toHaveLength(0);
      expect(api.isDestroyed() || panel.isVisible()).toBe(true);
    } finally {
      api.close();
      clock.restore();
    }
  });

  test("keeps a missing Settings helper during a drag, then cleans it up after bounded probes", async () => {
    const clock = installPollingClock();
    let target: { x: number; y: number; width: number; height: number } | null = {
      x: 120,
      y: 140,
      width: 920,
      height: 700,
    };
    const { api, bridge, panel } = await makeHarness({
      locateSettings: () => target,
    });
    try {
      api.setState("awaiting-user");
      await settleNativeAsync();
      const poll = clock.timers.find((timer) => timer.active);
      expect(poll?.delay).toBe(100);
      const rows = bridge.objects.filter((value) => value.hasSelector("mouseDown:"));
      expect(rows).toHaveLength(1);
      const row = rows[0];
      row.invoke("draggingSession:willBeginAtPoint:", { x: 0, y: 0 });
      target = null;

      for (let index = 0; index < 10; index += 1) {
        poll?.callback();
        await settleNativeAsync();
      }
      expect(helperPanels(bridge).some((value) => value.visible && !value.destroyed)).toBe(true);

      row.invoke("draggingSession:endedAtPoint:operation:", { x: 0, y: 0 }, 0);
      for (let index = 0; index < 10; index += 1) {
        poll?.callback();
        await settleNativeAsync();
      }
      expect(helperPanels(bridge).some((value) => value.visible && !value.destroyed)).toBe(false);
      expect(api.isDestroyed() || panel.isVisible()).toBe(true);
    } finally {
      api.close();
      clock.restore();
    }
  });

  test("waits for the arrow return before scheduling the next native pulse", async () => {
    const { api } = await makeHarness();
    const originalSetTimeout = globalThis.setTimeout;
    const originalClearTimeout = globalThis.clearTimeout;
    let nextId = 1;
    const scheduled: Array<{ id: number; delay: number; callback: () => void }> = [];
    globalThis.setTimeout = ((callback: TimerHandler, delay?: number) => {
      const entry = { id: nextId++, delay: Number(delay ?? 0), callback: callback as () => void };
      scheduled.push(entry);
      return entry.id as unknown as ReturnType<typeof setTimeout>;
    }) as unknown as typeof setTimeout;
    globalThis.clearTimeout = ((id?: ReturnType<typeof setTimeout>) => {
      const numeric = Number(id);
      const index = scheduled.findIndex((entry) => entry.id === numeric);
      if (index >= 0) scheduled.splice(index, 1);
    }) as unknown as typeof clearTimeout;

    try {
      api.setState("awaiting-user");
      await Promise.resolve();
      await Promise.resolve();
      const pulse = scheduled.find((entry) => entry.delay === 500);
      expect(pulse).toBeDefined();
      pulse?.callback();
      expect(scheduled.some((entry) => entry.delay === 250)).toBe(true);
      expect(scheduled.some((entry) => entry.delay === 4000)).toBe(false);
      scheduled.find((entry) => entry.delay === 250)?.callback();
      expect(scheduled.some((entry) => entry.delay === 4000)).toBe(true);
    } finally {
      globalThis.setTimeout = originalSetTimeout;
      globalThis.clearTimeout = originalClearTimeout;
      api.close();
    }
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

test("Settings tracking continues while the forward flight is still running", async () => {
  const clock = installPollingClock();
  let x = 120;
  let locations = 0;
  let finish!: () => void;
  const finished = new Promise<void>((resolve) => { finish = resolve; });
  let api: Awaited<ReturnType<typeof createNativeAccessibilitySetupWindow>> | undefined;
  try {
    const harness = await makeHarness({
      locateSettings: () => { locations++; return { x, y: 140, width: 920, height: 700 }; },
      onHandoff: () => ({ finished, dispose: finish }),
    });
    api = harness.api;
    objectWithTitle(harness.panel, COPY.repair)?.performClick$();
    await api.choice;
    api.setState("awaiting-user");
    await settleNativeAsync();
    const helper = helperPanels(harness.bridge).find((panel) => panel.frame().size.width === 532);
    expect(helper).toBeDefined();
    const before = helper?.frame().origin.x;
    x += 200;
    clock.timers[0]?.callback();
    await settleNativeAsync();
    expect(locations).toBe(2);
    expect(helper?.frame().origin.x).not.toBe(before);
  } finally { finish(); api?.close(); clock.restore(); }
});
