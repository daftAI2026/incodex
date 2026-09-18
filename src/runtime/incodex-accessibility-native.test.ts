import { describe, expect, test } from "bun:test";
import { createNativeAccessibilitySetupWindow as createNativeAccessibilitySetupWindowOrDeferred } from "./incodex-accessibility-native.cts";

// Existing fixtures have no presentation gate and must produce a real guide.
// Keep that assertion explicit now that deferred creation can return null.
async function createNativeAccessibilitySetupWindow(
  options: Parameters<typeof createNativeAccessibilitySetupWindowOrDeferred>[0],
) {
  const guide = await createNativeAccessibilitySetupWindowOrDeferred(options);
  if (!guide) throw new Error("Expected a presentable native guide fixture");
  return guide;
}

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
  back: "Back",
  addedTitle: "Allow Codex in System Settings",
  addedBody: "Drag Codex into Accessibility and wait for the automatic check.",
  dragInstruction: "Drag Codex to the list above to allow Accessibility",
  dragInstructionRuns: JSON.stringify([
    { text: "Drag ", role: "secondary" },
    { text: "Codex", role: "primary" },
    { text: " to the list above to allow ", role: "secondary" },
    { text: "Accessibility", role: "primary" },
  ]),
  openSettings: "Open Settings",
  checking: "Checking automatically",
  completeInSettings: "COMPLETE IN SYSTEM SETTINGS",
  repairing: "Preparing System Settings…",
  errorTitle: "Unable to open Settings",
  errorBody: "Try opening System Settings again.",
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
  contentViewControllerValue: FakeNative | null = null;
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

  initWithContentRect$styleMask$backing$defer$(value: Frame, styleMask: number): FakeNative {
    this.values.set("styleMask", styleMask);
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

  configureWithCopy$appIcon$permissionIcon$actionTarget$(...args: unknown[]): unknown {
    return this.invoke("configureWithCopy:appIcon:permissionIcon:actionTarget:", ...args);
  }

  configureWithCopy$appIcon$actionTarget$(...args: unknown[]): unknown {
    return this.invoke("configureWithCopy:appIcon:actionTarget:", ...args);
  }

  setContentWithTitle$body$allowEnabled$settingsPlaceholder$(...args: unknown[]): unknown {
    return this.invoke("setContentWithTitle:body:allowEnabled:settingsPlaceholder:", ...args);
  }

  preferredContentSize(): unknown {
    return this.invoke("preferredContentSize");
  }

  permissionCardView(): unknown {
    return this.invoke("permissionCardView");
  }

  appRowFrame(): unknown {
    return this.invoke("appRowFrame");
  }

  appRowView(): unknown {
    return this.invoke("appRowView");
  }

  backingScaleFactor(): number { return Number(this.values.get("backingScaleFactor") ?? 2); }
  snapshotImageWithScale$(scale: number): unknown {
    this.record("snapshotImageWithScale:", scale);
    return this.values.get("foregroundSnapshot");
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

  safeAreaInsets(): { top: number; left: number; bottom: number; right: number } {
    return { top: Number(this.values.get("safeAreaTop") ?? 32), left: 0, bottom: 0, right: 0 };
  }

  fittingSize(): { width: number; height: number } {
    const configured = this.values.get("fittingSize");
    if (configured && typeof configured === "object") {
      const value = configured as { width?: unknown; height?: unknown };
      if (typeof value.width === "number" && typeof value.height === "number") {
        return { width: value.width, height: value.height };
      }
    }
    return { width: 378, height: 30 };
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

  sizeToFit(): void {
    if (this.type === "NSButton") {
      this.frameValue.size = this.values.get("bordered") === false
        ? { width: 27.5, height: 16 }
        : { width: 56.5, height: 24 };
    }
  }

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

  accessibilityDisplayShouldReduceMotion(): boolean { return Boolean(this.values.get("reduceMotion")); }
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

  setContentViewController$(value: FakeNative): void {
    this.record("setContentViewController:", value);
    this.contentViewControllerValue = value;
    const view = value.view();
    if (view) {
      this.contentViewValue = view;
      if (!this.subviews.includes(view)) this.subviews.push(view);
    }
  }

  setView$(value: FakeNative): void {
    this.values.set("view", value);
  }

  view(): FakeNative | null {
    const value = this.values.get("view");
    return value instanceof FakeNative ? value : null;
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

  setCollectionBehavior$(value: unknown): void {
    this.values.set("collectionBehavior", value);
  }

  setOpaque$(value: unknown): void {
    this.values.set("opaque", value);
  }

  setBackgroundColor$(value: unknown): void {
    this.values.set("backgroundColor", value);
  }

  colorWithAlphaComponent$(value: unknown): FakeNative {
    this.values.set("alpha", value);
    return this;
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

  setToolbarStyle$(value: unknown): void {
    this.values.set("toolbarStyle", value);
  }

  setMovableByWindowBackground$(value: unknown): void {
    this.values.set("movableByWindowBackground", value);
  }

  setMovable$(value: unknown): void {
    this.values.set("movable", value);
  }

  setWantsLayer$(value: unknown): void {
    this.values.set("wantsLayer", value);
  }

  setAppearance$(value: unknown): void { this.values.set("appearance", value); }

  setMaterial$(value: unknown): void {
    this.values.set("material", value);
  }

  setBlendingMode$(value: unknown): void {
    this.values.set("blendingMode", value);
  }

  setState$(value: unknown): void {
    this.values.set("state", value);
  }

  setClipsToBounds$(value: unknown): void { this.values.set("clipsToBounds", value); }

  addSublayer$(value: unknown): void { this.values.set("sublayer", value); }
  addObject$(value: unknown): void { const items = (this.values.get("items") ?? []) as unknown[]; items.push(value); this.values.set("items", items); }
  setLineDashPattern$(value: unknown): void { this.values.set("lineDashPattern", value); }
  setCornerCurve$(value: unknown): void { this.values.set("cornerCurve", value); }

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

  setObject$forKey$(value: unknown, key: unknown): void {
    this.values.set(String(key), value);
  }

  objectForKey$(key: unknown): unknown {
    return this.values.get(String(key));
  }

  setStringValue$(value: unknown): void {
    this.values.set("stringValue", String(value));
  }

  attributedStringValue(): FakeNative { return this; }

  mutableCopy(): FakeNative {
    const copy = new FakeNative("NSMutableAttributedString", this.calls);
    for (const [key, value] of this.values) copy.values.set(key, value);
    return copy;
  }

  length(): number { return String(this.values.get("stringValue") ?? "").length; }

  addAttribute$value$range$(name: unknown, value: unknown, range: unknown): void {
    this.values.set(String(name), value);
    this.values.set("attributeRange", range);
  }

  setAttributedStringValue$(value: FakeNative): void {
    this.values.set("attributedValue", value);
    this.values.set("stringValue", value.values.get("stringValue"));
  }

  setLineSpacing$(value: number): void { this.values.set("lineSpacing", value); }

  cell(): FakeNative { return this; }
  isHighlighted(): boolean { return Boolean(this.values.get("highlighted")); }
  setAccessibilityElement$(value: unknown): void { this.values.set("accessibilityElement", value); }

  cellSizeForBounds$(value: Frame): { width: number; height: number } {
    this.record("cellSizeForBounds:", value);
    return { width: value.size.width, height: Number(this.values.get("measuredHeight") ?? 32) };
  }

  setContentSize$(size: { width: number; height: number }): void {
    this.frameValue.size = { ...size };
  }

  setFont$(value: unknown): void {
    this.values.set("font", value);
  }

  setAlignment$(value: unknown): void {
    this.values.set("alignment", value);
  }

  colorUsingColorSpace$(_space: unknown): FakeNative { return this; }
  redComponent(): number { return 0; }
  greenComponent(): number { return 0; }
  blueComponent(): number { return 0; }
  alphaComponent(): number { return 1; }
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

  setAccessibilityLabel$(value: unknown): void {
    this.values.set("accessibilityLabel", value);
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

  animateToScaleX$scaleY$(x: number, y: number): void {
    this.record("animateToScaleX:scaleY:", x, y);
    this.values.set("scale", { x, y });
  }

  resetToIdentity(): void {
    this.record("resetToIdentity");
    this.values.set("scale", { x: 1, y: 1 });
  }

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
    this.values.set("keyCount", Number(this.values.get("keyCount") ?? 0) + 1);
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

function makeBridge(
  bodyHeight = 32,
  helperFittingSize = { width: 531, height: 110 },
  helperInstructionWidth = 408,
  helperInstructionText = COPY.addedBody,
  titleHeight = 30,
  helperInstructionHeight = 16,
): FakeBridge {
  const calls: NativeCall[] = [];
  const objects: FakeNative[] = [];
  let sharedWorkspace: FakeNative | undefined;
  const definitions = new Map<string, Record<string, (...args: any[]) => unknown>>();

  function object(type: string, methods: Record<string, (...args: any[]) => unknown> = {}): FakeNative {
    const value = new FakeNative(type, calls, methods);
    if (type.endsWith("_Material")) value.values.set("fittingSize", { ...helperFittingSize });
    objects.push(value);
    return value;
  }

  function classObject(type: string): any {
    const methods = definitions.get(type) ?? {};
    const cls: Record<string, unknown> = {
      alloc: () => object(type, methods),
      appearanceNamed$: (name: string) => name,
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
        const field = object(type, methods);
        field.setStringValue$(value);
        field.values.set("measuredHeight", String(value) === COPY.title || String(value) === COPY.errorTitle ? titleHeight : bodyHeight);
        if (String(value) === helperInstructionText) {
          field.values.set("fittingSize", { width: helperInstructionWidth, height: helperInstructionHeight });
          field.values.set("measuredHeight", helperInstructionHeight);
        }
        return field;
      },
      stringWithUTF8String$: (value: string) => value,
      fileURLWithPath$: (value: unknown) => {
        const url = object(type);
        url.values.set("fileURLPath", String(value));
        return url;
      },
      imageNamed$: (value: string) => value,
      sharedWorkspace: () => sharedWorkspace ??= object(type),
      defaultCenter: () => object(type),
      whiteColor: () => {
        const color = object(type);
        color.values.set("namedColor", "white");
        return color;
      },
      clearColor: () => {
        const color = object(type);
        color.values.set("namedColor", "clear");
        return color;
      },
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
      get NSMutableParagraphStyle() { return library(this.framework).NSMutableParagraphStyle; }
      get NSAppearance() { return library(this.framework).NSAppearance; }
      get NSPanel() { return library(this.framework).NSPanel; }
      get NSViewController() { return library(this.framework).NSViewController; }
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
      get NSMutableArray() { return library(this.framework).NSMutableArray; }
      get NSMutableDictionary() { return library(this.framework).NSMutableDictionary; }
      get CAShapeLayer() { return library(this.framework).CAShapeLayer; }
      get NSColorSpace() { return library(this.framework).NSColorSpace; }
      get NSNumber() { return library(this.framework).NSNumber; }
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
      if (name === "CGPathCreateMutable") return { path: [] };
      if (name === "CGPathCreateWithRoundedRect") return { path: args.slice(1) };
      if (name === "CGColorCreateGenericRGB" && (args[0] as { returns?: string })?.returns === "@") return { color: args.slice(1) };
      return undefined;
    },
    RunLoop: { run: () => () => {}, stop: () => {} },
  };

  return { objc, calls, objects };
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

type ArrowTimer = { id: number; delay: number; callback: () => void; active: boolean };

function installArrowClock() {
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  let nextId = 1;
  const timers: ArrowTimer[] = [];
  globalThis.setTimeout = ((callback: TimerHandler, delay?: number) => {
    const timer = {
      id: nextId++,
      delay: Number(delay ?? 0),
      callback: callback as () => void,
      active: true,
    };
    timers.push(timer);
    return timer.id as unknown as ReturnType<typeof setTimeout>;
  }) as unknown as typeof setTimeout;
  globalThis.clearTimeout = ((id?: ReturnType<typeof setTimeout>) => {
    const timer = timers.find((entry) => entry.id === Number(id));
    if (timer) timer.active = false;
  }) as unknown as typeof clearTimeout;
  return {
    timers,
    active() { return timers.filter((timer) => timer.active); },
    fire(timer: ArrowTimer) {
      if (!timer.active) return;
      timer.active = false;
      timer.callback();
    },
    restore() {
      globalThis.setTimeout = originalSetTimeout;
      globalThis.clearTimeout = originalClearTimeout;
    },
  };
}

function helperPanels(bridge: FakeBridge): FakeNative[] {
  // Both the frozen ordinary shell (0x8091) and its independent arrow child
  // (128) are nonactivating panels; do not identify the helper by the old
  // exact style mask.
  return bridge.objects.filter((value) => value.type === "NSPanel"
    && (Number(value.values.get("styleMask")) & 128) !== 0);
}

function swiftDelegate(harness: { swift?: ReturnType<typeof swiftPermissionViewsLibrary> }): FakeNative {
  const call = harness.swift?.calls.find((entry) => entry.selector === "configureWithCopy:appIcon:permissionIcon:actionTarget:");
  const target = call?.args[3];
  if (!(target instanceof FakeNative)) throw new Error("SwiftUI initial action target is missing");
  return target;
}

function performSwiftAction(harness: { swift?: ReturnType<typeof swiftPermissionViewsLibrary> }, selector: string): void {
  const target = swiftDelegate(harness);
  target.invoke(selector, target);
}

function swiftPermissionViewsLibrary() {
  const calls: NativeCall[] = [];
  const initialViews: FakeNative[] = [];
  const initialCards: FakeNative[] = [];
  const helperViews: FakeNative[] = [];
  const helperRows: FakeNative[] = [];
  const arrowViews: FakeNative[] = [];

  function initialClass() {
    const card = new FakeNative("SwiftPermissionCardView", calls);
    card.frameValue = frame(518, 80);
    const view = new FakeNative("IncodexPermissionInitialView", calls, {
      "configureWithCopy:appIcon:permissionIcon:actionTarget:": function (this: FakeNative, ...args: unknown[]) {
        this.record("configureWithCopy:appIcon:permissionIcon:actionTarget:", ...args);
        this.values.set("copy", args[0]);
        this.values.set("actionTarget", args[3]);
      },
      "setContentWithTitle:body:allowEnabled:settingsPlaceholder:": function (this: FakeNative, ...args: unknown[]) {
        this.record("setContentWithTitle:body:allowEnabled:settingsPlaceholder:", ...args);
        this.values.set("content", args);
      },
      preferredContentSize: () => ({ width: 600, height: 340 }),
      permissionCardView: () => card,
    });
    view.frameValue = frame(600, 340);
    view.subviews.push(new FakeNative("NSHostingView<Initial>", calls));
    initialCards.push(card);
    initialViews.push(view);
    return view;
  }

  function helperClass() {
    const row = new FakeNative("SwiftPermissionAppRowView", calls);
    row.frameValue = frame(459, 42);
    const view = new FakeNative("IncodexPermissionHelperView", calls, {
      "configureWithCopy:appIcon:actionTarget:": function (this: FakeNative, ...args: unknown[]) {
        this.record("configureWithCopy:appIcon:actionTarget:", ...args);
        this.values.set("copy", args[0]);
        this.values.set("actionTarget", args[2]);
      },
      preferredContentSize: () => ({ width: 531, height: 110 }),
      appRowFrame: () => frame(459, 42, 62, 48),
      appRowView: () => row,
    });
    view.frameValue = frame(531, 110);
    view.subviews.push(new FakeNative("NSHostingView<Helper>", calls));
    helperRows.push(row);
    helperViews.push(view);
    return view;
  }

  function arrowClass() {
    const view = new FakeNative("IncodexPermissionArrowView", calls, {
      "animateToScaleX:scaleY:": function (this: FakeNative, x: number, y: number) {
        this.animateToScaleX$scaleY$(x, y);
      },
      resetToIdentity: function (this: FakeNative) {
        this.resetToIdentity();
      },
    });
    view.frameValue = frame(28, 28, 36, 10);
    view.subviews.push(new FakeNative("NSHostingView<Arrow>", calls));
    arrowViews.push(view);
    return view;
  }

  return {
    library: {
      IncodexPermissionInitialView: { alloc: initialClass },
      IncodexPermissionHelperView: { alloc: helperClass },
      IncodexPermissionArrowView: { alloc: arrowClass },
    },
    calls,
    initialViews,
    initialCards,
    helperViews,
    helperRows,
    arrowViews,
  };
}

test("anchors the Accessibility helper to the Settings bottom and trailing edges", async () => {
  const clock = installPollingClock();
  let target = { x: 554, y: 160, width: 740, height: 625 };
  let api: Awaited<ReturnType<typeof createNativeAccessibilitySetupWindow>> | undefined;
  try {
    const harness = await makeHarness({ locateSettings: () => target });
    api = harness.api;
    api.setState("awaiting-user");
    await settleNativeAsync();
    const helper = helperPanels(harness.bridge).find((panel) => panel.frame().size.width === 531);
    // The fake primary screen is 900pt high. CUA's Accessibility branch uses
    // maxX - fittingWidth - 10, primaryHeight - maxY + 10 (AppKit coordinates).
    expect(helper?.frame()).toEqual(frame(531, 110, 753, 125));
    target = { ...target, x: target.x - 80, y: target.y - 60 };
    clock.timers.find((timer) => timer.active && timer.delay === 100)?.callback();
    await settleNativeAsync();
    expect(helper?.frame()).toEqual(frame(531, 110, 673, 185));
  } finally { api?.close(); clock.restore(); }
});

test("uses SwiftUI helper preferred size with AppKit drag and arrow geometry", async () => {
  const { api, bridge, swift } = await makeHarness({
    locateSettings: () => ({ x: 554, y: 160, width: 740, height: 625 }),
  });
  try {
    api.setState("awaiting-user");
    await flushNativeAsync();

    const helper = helperPanels(bridge).find((panel) => {
      const size = panel.frame().size;
      return size.width === 531 && size.height === 110;
    });
    if (!helper) throw new Error("measured native Accessibility helper is missing");
    expect(helper.frame()).toEqual(frame(531, 110, 753, 125));

    const view = helper.contentViewValue;
    if (!view || !swift?.helperViews[0]) throw new Error("measured native Accessibility helper content is missing");
    const nativeHelper = swift.helperViews[0];
    expect(nativeHelper.preferredContentSize()).toEqual({ width: 531, height: 110 });
    expect(nativeHelper.appRowFrame()).toEqual(frame(459, 42, 62, 48));
    expect(nativeHelper.appRowView()).toBe(swift.helperRows[0]);
    const row = view.subviews.find((value) => value.hasSelector("mouseDown:"));
    if (!row) throw new Error("AppKit drag overlay is missing");
    expect(row.frame()).toEqual(frame(459, 42, 62, 48));

    const arrowWindow = helperPanels(bridge).find((panel) => {
      const size = panel.frame().size;
      return size.width === 100 && size.height === 100;
    });
    expect(arrowWindow?.frame()).toEqual(frame(100, 100, 783, 185));
    const arrow = swift?.arrowViews[0];
    expect(arrow?.frame()).toEqual(frame(28, 28, 36, 10));
  } finally {
    api.close();
  }
});

test("gives the ordinary helper its native shadow while keeping the arrow shell shadowless", async () => {
  const { api, bridge } = await makeHarness({
    locateSettings: () => ({ x: 554, y: 160, width: 740, height: 625 }),
  });
  try {
    api.setState("awaiting-user");
    await flushNativeAsync();
    const helper = helperPanels(bridge).find((panel) => {
      const size = panel.frame().size;
      return size.width === 531 && size.height === 110;
    });
    const arrow = helperPanels(bridge).find((panel) => {
      const size = panel.frame().size;
      return size.width === 100 && size.height === 100;
    });
    if (!helper || !arrow) throw new Error("ordinary helper and arrow panels are missing");
    expect(helper.values.get("hasShadow")).toBe(true);
    expect(arrow.values.get("hasShadow")).toBe(false);
  } finally {
    api.close();
  }
});

test("uses the original ordinary helper panel shell without adding safe-area height", async () => {
  const { api, bridge } = await makeHarness({
    locateSettings: () => ({ x: 554, y: 160, width: 740, height: 625 }),
  });
  try {
    api.setState("awaiting-user");
    await flushNativeAsync();
    const helper = bridge.objects.find((panel) => {
      if (panel.type !== "NSPanel") return false;
      const size = panel.frame().size;
      return size.width === 531 && size.height === 110;
    });
    const arrow = bridge.objects.find((panel) => {
      if (panel.type !== "NSPanel") return false;
      const size = panel.frame().size;
      return size.width === 100 && size.height === 100;
    });
    if (!helper || !arrow) throw new Error("ordinary helper and arrow panels are missing");

    // Frozen original ordinary shell: titled utility nonactivating full-size
    // content panel, hidden title, transparent titlebar, toolbar style 3,
    // and neither kind of AppKit movability.
    expect(helper.values.get("styleMask")).toBe(0x8091);
    expect(helper.values.get("titleVisibility")).toBe(1);
    expect(helper.values.get("titlebarAppearsTransparent")).toBe(true);
    expect(helper.values.get("toolbarStyle")).toBe(3);
    expect(helper.values.get("movableByWindowBackground")).toBe(false);
    expect(helper.values.get("movable")).toBe(false);
    expect(helper.values.get("collectionBehavior")).toBe(0x24a);

    const background = helper.values.get("backgroundColor");
    expect(background).toBeInstanceOf(FakeNative);
    expect((background as FakeNative).values.get("namedColor")).toBe("white");
    expect(Number((background as FakeNative).values.get("alpha"))).toBeCloseTo(0.001, 6);

    // The ordinary panel owns a controller whose view is the SwiftUI helper;
    // direct panel contentView assignment is not an equivalent shell contract.
    const controller = helper.contentViewControllerValue;
    expect(controller).toBeInstanceOf(FakeNative);
    const helperView = controller?.view();
    expect(helperView).toBe(helper.contentViewValue);
    expect(helperView?.frame().size).toEqual({ width: 531, height: 110 });
    expect(helper.frame().size).toEqual({ width: 531, height: 110 });

    // The arrow remains the independent borderless/nonactivating child shell.
    expect(arrow.values.get("styleMask")).toBe(128);
  } finally {
    api.close();
  }
});

test("keeps the reference fixed helper width despite an unrelated host fitting size", async () => {
  const { api, bridge } = await makeHarness({
    helperFittingSize: { width: 600, height: 140 },
    locateSettings: () => ({ x: 554, y: 160, width: 740, height: 625 }),
  });
  try {
    api.setState("awaiting-user");
    await flushNativeAsync();

    const helper = helperPanels(bridge).find((panel) => {
      const size = panel.frame().size;
      return size.width === 531 && size.height === 110;
    });
    expect(helper?.frame()).toEqual(frame(531, 110, 753, 125));
    expect(helper?.contentViewValue?.frame()).toEqual(frame(531, 110));
  } finally {
    api.close();
  }
});

test("uses SwiftUI preferred helper size for a wider localized instruction", async () => {
  const swift = swiftPermissionViewsLibrary();
  swift.helperViews.length = 0;
  const originalHelper = swift.library.IncodexPermissionHelperView.alloc;
  swift.library.IncodexPermissionHelperView.alloc = () => {
    const view = originalHelper();
    view.values.set("preferred", { width: 531, height: 126 });
    view.selectors.set("preferredContentSize", () => ({ width: 531, height: 126 }));
    view.selectors.set("appRowFrame", () => frame(459, 42, 62, 64));
    return view;
  };
  const { api, bridge } = await makeHarness({
    nativeLibrary: swift.library,
    locateSettings: () => ({ x: 554, y: 160, width: 740, height: 625 }),
  });
  try {
    api.setState("awaiting-user");
    await flushNativeAsync();

    const helper = helperPanels(bridge).find((panel) => panel.frame().size.height === 126);
    if (!helper) throw new Error("localized Accessibility helper is missing");
    expect(helper.frame()).toEqual(frame(531, 126, 753, 125));

    const view = helper.contentViewValue;
    if (!view || !swift.helperViews[0]) throw new Error("localized native helper content is missing");
    const nativeHelper = swift.helperViews[0];
    expect(nativeHelper.preferredContentSize()).toEqual({ width: 531, height: 126 });
    expect(nativeHelper.appRowFrame()).toEqual(frame(459, 42, 62, 64));
    const row = view.subviews.find((value) => value.hasSelector("mouseDown:"));
    if (!row) throw new Error("localized AppKit drag overlay is missing");
    expect(row.frame()).toEqual(frame(459, 42, 62, 64));

    const arrowWindow = helperPanels(bridge).find((panel) => {
      const size = panel.frame().size;
      return size.width === 100 && size.height === 100;
    });
    // Natural bottom-aligned helper content keeps the arrow's bottom anchor
    // stable when long copy increases the shell height.
    expect(arrowWindow?.frame()).toEqual(frame(100, 100, 783, 185));
  } finally {
    api.close();
  }
});

test("keeps the initial permission window behind the helper during the forward handoff", async () => {
  const { api, panel } = await makeHarness({
    locateSettings: () => ({ x: 554, y: 160, width: 740, height: 625 }),
  });
  try {
    api.setState("awaiting-user");
    await flushNativeAsync();
    expect(panel.isVisible()).toBe(true);
  } finally {
    api.close();
  }
});

test("dispatches SwiftUI Skip through the guide action target", async () => {
  const harness = await makeHarness();
  const { api, panel, swift } = harness;
  expect(panel.frame().size.width).toBe(600);
  expect(panel.frame().size.height).toBeGreaterThanOrEqual(312);
  const initial = swift!.initialViews[0];
  const configure = swift!.calls.find((call) => call.selector === "configureWithCopy:appIcon:permissionIcon:actionTarget:");
  expect(configure?.args[0]).toBeDefined();
  expect(initial.preferredContentSize()).toEqual({ width: 600, height: 340 });
  expect(initial.permissionCardView()).toBe(swift!.initialCards[0]);

  performSwiftAction(harness, "skip:");
  await expect(api.choice).resolves.toBe("later");
  expect(api.isDestroyed()).toBe(true);
});

async function makeHarness(options: {
  onHandoff?: (payload: any) => void;
  onBack?: (payload: any) => { finished?: Promise<unknown>; dispose?: () => void } | undefined;
  locateSettings?: () => unknown;
  copy?: typeof COPY;
  reduceMotion?: boolean;
  bodyHeight?: number;
  titleHeight?: number;
  helperFittingSize?: { width: number; height: number };
  helperInstructionWidth?: number;
  helperInstructionHeight?: number;
  nativeLibrary?: any;
  layoutDirection?: "leftToRight" | "rightToLeft";
} = {}) {
  const swift = options.nativeLibrary ? undefined : swiftPermissionViewsLibrary();
  const bridge = makeBridge(
    options.bodyHeight,
    options.helperFittingSize,
    options.helperInstructionWidth,
    options.copy?.addedBody ?? COPY.addedBody,
    options.titleHeight,
    options.helperInstructionHeight,
  );
  const api = await createNativeAccessibilitySetupWindow({
    appPath: APP_PATH,
    copy: options.copy ?? COPY,
    loadObjcModule: async () => bridge.objc,
    nativeLibrary: options.nativeLibrary ?? swift?.library,
    layoutDirection: options.layoutDirection,
    locateSettings: options.locateSettings ?? (() => ({ x: 120, y: 140, width: 920, height: 700 })),
    onHandoff: options.onHandoff,
    onBack: options.onBack,
  });
  if (options.reduceMotion) {
    const workspace = bridge.objects.find((value) => value.type === "NSWorkspace")
      ?? new (bridge.objc as any).NobjcLibrary("/System/Library/Frameworks/AppKit.framework/AppKit").NSWorkspace.sharedWorkspace();
    workspace.values.set("reduceMotion", true);
  }
  const panel = bridge.objects.find((value) => value.type === "NSPanel");
  if (!panel) throw new Error("native Accessibility panel was not created");
  return { api, bridge, panel, swift };
}

test("does not steal focus when presentation becomes unavailable during bridge loading", async () => {
  const bridge = makeBridge();
  const swift = swiftPermissionViewsLibrary();
  let canPresent = true;
  let release!: (value: any) => void;
  const loading = new Promise<any>((resolve) => { release = resolve; });
  let focusCalls = 0;
  const pending = createNativeAccessibilitySetupWindowOrDeferred({
    appPath: APP_PATH,
    copy: COPY,
    loadObjcModule: () => loading,
    nativeLibrary: swift.library as any,
    canPresent: () => canPresent,
    electron: { app: { focus: () => { focusCalls++; } } } as any,
    locateSettings: () => null,
    onBack: undefined,
    onHandoff: undefined,
  });
  canPresent = false;
  release(bridge.objc);
  const api = await pending;
  try {
    expect(api).toBeNull();
    expect(focusCalls).toBe(0);
    expect(bridge.objects.filter((value) => value.type === "NSPanel")).toHaveLength(0);
  } finally {
    api?.close();
  }
});

test("passes the selected native layout direction to initial and helper copy dictionaries", async () => {
  const harness = await makeHarness({ layoutDirection: "rightToLeft" });
  const { api, swift } = harness;
  try {
    const initialConfigure = swift!.calls.find(
      (call) => call.selector === "configureWithCopy:appIcon:permissionIcon:actionTarget:",
    );
    const initialCopy = initialConfigure?.args[0] as FakeNative;
    expect(initialCopy.values.get("layoutDirection")).toBe("rightToLeft");

    api.setState("awaiting-user");
    await flushNativeAsync();
    const helperConfigure = [...swift!.calls].reverse().find(
      (call) => call.selector === "configureWithCopy:appIcon:actionTarget:",
    );
    const helperCopy = helperConfigure?.args[0] as FakeNative;
    expect(helperCopy.values.get("layoutDirection")).toBe("rightToLeft");
  } finally {
    api.close();
  }
});

test("defaults both native copy dictionaries to left-to-right", async () => {
  const harness = await makeHarness();
  const { api, swift } = harness;
  try {
    const initialConfigure = swift!.calls.find(
      (call) => call.selector === "configureWithCopy:appIcon:permissionIcon:actionTarget:",
    );
    const initialCopy = initialConfigure?.args[0] as FakeNative;
    expect(initialCopy.values.get("layoutDirection")).toBe("leftToRight");

    api.setState("awaiting-user");
    await flushNativeAsync();
    const helperConfigure = [...swift!.calls].reverse().find(
      (call) => call.selector === "configureWithCopy:appIcon:actionTarget:",
    );
    const helperCopy = helperConfigure?.args[0] as FakeNative;
    expect(helperCopy.values.get("layoutDirection")).toBe("leftToRight");
  } finally {
    api.close();
  }
});

test("mirrors only the helper arrow child-window x in RTL and preserves LTR placement", async () => {
  const target = { x: 554, y: 160, width: 740, height: 625 };
  const rtl = await makeHarness({ layoutDirection: "rightToLeft", locateSettings: () => target });
  try {
    rtl.api.setState("awaiting-user");
    await flushNativeAsync();
    const arrowWindow = helperPanels(rtl.bridge).find((panel) => {
      const size = panel.frame().size;
      return size.width === 100 && size.height === 100;
    });
    expect(arrowWindow?.frame()).toEqual(frame(100, 100, 1154, 185));
  } finally {
    rtl.api.close();
  }

  const ltr = await makeHarness({ layoutDirection: "leftToRight", locateSettings: () => target });
  try {
    ltr.api.setState("awaiting-user");
    await flushNativeAsync();
    const arrowWindow = helperPanels(ltr.bridge).find((panel) => {
      const size = panel.frame().size;
      return size.width === 100 && size.height === 100;
    });
    expect(arrowWindow?.frame()).toEqual(frame(100, 100, 783, 185));
  } finally {
    ltr.api.close();
  }
});

test("keeps the dynamic arrow glyph slot aligned with the SwiftUI helper slot in both directions", async () => {
  const target = { x: 554, y: 160, width: 740, height: 625 };
  const findArrowSlot = (bridge: FakeBridge, helper: FakeNative, swift: ReturnType<typeof swiftPermissionViewsLibrary>) => {
    const arrowWindow = helperPanels(bridge).find((panel) => {
      const size = panel.frame().size;
      return size.width === 100 && size.height === 100;
    });
    const arrow = swift.arrowViews[0];
    if (!arrowWindow || !arrow) throw new Error("arrow shell is missing");
    return arrowWindow.frame().origin.x + arrow.frame().origin.x - helper.frame().origin.x;
  };

  const rtlSwift = swiftPermissionViewsLibrary();
  const rtl = await makeHarness({ layoutDirection: "rightToLeft", locateSettings: () => target, nativeLibrary: rtlSwift.library });
  try {
    rtl.api.setState("awaiting-user");
    await flushNativeAsync();
    const helper = helperPanels(rtl.bridge).find((panel) => panel.frame().size.width === 531);
    if (!helper) throw new Error("RTL helper panel is missing");
    // PermissionHelperForeground's mirrored SwiftUI slot is x=437 in RTL.
    expect(findArrowSlot(rtl.bridge, helper, rtlSwift)).toBe(437);
  } finally {
    rtl.api.close();
  }

  const ltrSwift = swiftPermissionViewsLibrary();
  const ltr = await makeHarness({ layoutDirection: "leftToRight", locateSettings: () => target, nativeLibrary: ltrSwift.library });
  try {
    ltr.api.setState("awaiting-user");
    await flushNativeAsync();
    const helper = helperPanels(ltr.bridge).find((panel) => panel.frame().size.width === 531);
    if (!helper) throw new Error("LTR helper panel is missing");
    // PermissionHelperForeground's leading SwiftUI slot is x=66 in LTR.
    expect(findArrowSlot(ltr.bridge, helper, ltrSwift)).toBe(66);
  } finally {
    ltr.api.close();
  }
});

describe("native Accessibility setup adapter", () => {
  test("initial panel uses the injected SwiftUI view ABI for copy, content and card access", async () => {
    const swift = swiftPermissionViewsLibrary();
    const { api, panel } = await makeHarness({ nativeLibrary: swift.library });
    try {
      expect(swift.initialViews).toHaveLength(1);
      const initial = swift.initialViews[0];
      expect(panel.contentViewValue).toBe(initial);
      const configure = swift.calls.find((call) => call.selector === "configureWithCopy:appIcon:permissionIcon:actionTarget:");
      expect(configure).toBeDefined();
      const content = swift.calls.find((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:");
      expect(content?.args.slice(0, 2)).toEqual([COPY.title, COPY.body]);
      expect(typeof content?.args[2]).toBe("boolean");
      expect(typeof content?.args[3]).toBe("boolean");
      expect(initial.preferredContentSize()).toEqual({ width: 600, height: 340 });
      expect(initial.permissionCardView()).toBe(swift.initialCards[0]);
    } finally {
      api.close();
    }
  });

  test("helper panel uses the injected SwiftUI view ABI for copy, row frame and row view", async () => {
    const swift = swiftPermissionViewsLibrary();
    const { api } = await makeHarness({
      nativeLibrary: swift.library as any,
      locateSettings: () => ({ x: 554, y: 160, width: 740, height: 625 }),
    });
    try {
      api.setState("awaiting-user");
      await flushNativeAsync();
      expect(swift.helperViews).toHaveLength(1);
      const helper = swift.helperViews[0];
      const configure = swift.calls.find((call) => call.selector === "configureWithCopy:appIcon:actionTarget:");
      expect(configure).toBeDefined();
      expect(helper.preferredContentSize()).toEqual({ width: 531, height: 110 });
      expect(helper.appRowFrame()).toEqual(frame(459, 42, 62, 48));
      expect(helper.appRowView()).toBe(swift.helperRows[0]);
    } finally {
      api.close();
    }
  });

  test.each([
    "configureWithCopy$appIcon$permissionIcon$actionTarget$",
    "setContentWithTitle$body$allowEnabled$settingsPlaceholder$",
    "preferredContentSize",
  ])("closes the initial panel when SwiftUI initialization fails at %s", async (selector) => {
    const bridge = makeBridge();
    const swift = swiftPermissionViewsLibrary();
    const original = swift.library.IncodexPermissionInitialView.alloc;
    swift.library.IncodexPermissionInitialView.alloc = () => {
      const view = original();
      view.selectors.set(selector, () => {
        throw new Error("initial SwiftUI ABI failure");
      });
      return view;
    };
    await expect(createNativeAccessibilitySetupWindow({
      appPath: APP_PATH,
      copy: COPY,
      loadObjcModule: async () => bridge.objc,
      nativeLibrary: swift.library as any,
      locateSettings: () => null,
      onBack: undefined,
      onHandoff: undefined,
    })).rejects.toThrow("initial SwiftUI ABI failure");
    const panels = bridge.objects.filter((value) => value.type === "NSPanel");
    expect(panels).toHaveLength(1);
    expect(panels[0].isDestroyed()).toBe(true);
    expect(panels[0].isVisible()).toBe(false);
  });

  test("closes a helper panel when SwiftUI preferred size is invalid", async () => {
    const bridge = makeBridge();
    const swift = swiftPermissionViewsLibrary();
    const original = swift.library.IncodexPermissionHelperView.alloc;
    swift.library.IncodexPermissionHelperView.alloc = () => {
      const view = original();
      view.selectors.set("preferredContentSize", () => ({ width: 0, height: 0 }));
      return view;
    };
    const api = await createNativeAccessibilitySetupWindow({
      appPath: APP_PATH,
      copy: COPY,
      loadObjcModule: async () => bridge.objc,
      nativeLibrary: swift.library as any,
      locateSettings: () => ({ x: 120, y: 140, width: 920, height: 700 }),
      onBack: undefined,
      onHandoff: undefined,
    });
    try {
      api.setState("awaiting-user");
      await flushNativeAsync();
      const helper = helperPanels(bridge);
      expect(helper.length).toBeGreaterThan(0);
      expect(helper.every((value) => value.isDestroyed() && !value.isVisible())).toBe(true);
      expect(api.isDestroyed()).toBe(false);
    } finally {
      api.close();
    }
  });

  test("mounts the SwiftUI initial view and card host in the AppKit panel", async () => {
    const { panel, bridge, swift } = await makeHarness();
    const content = panel.contentViewValue;
    if (!content || !swift?.initialViews[0]) throw new Error("initial native panel content is missing");
    const initial = swift.initialViews[0];
    expect(panel.isVisible()).toBe(true);
    expect(content).toBe(initial);
    expect(content.frame().size.width).toBe(600);
    expect(initial.preferredContentSize()).toEqual({ width: 600, height: 340 });
    expect(initial.permissionCardView()).toBe(swift.initialCards[0]);
    expect(swift.initialCards[0].frame().size).toEqual({ width: 518, height: 80 });
    expect(bridge.objects.some(value => value.type === "NSVisualEffectView")).toBe(false);
    expect(bridge.objects.some(value => value.type === "NSTextField")).toBe(false);
    expect(bridge.objects.some(value => value.type === "NSButton")).toBe(false);
    const configure = swift.calls.find((call) => call.selector === "configureWithCopy:appIcon:permissionIcon:actionTarget:");
    expect(configure?.args[0]).toBeDefined();
    const configuredCopy = configure?.args[0];
    if (!(configuredCopy instanceof FakeNative)) throw new Error("native initial copy is missing");
    for (const key of ["title", "body", "permissionTitle", "permissionDescription", "repair", "later", "completeInSettings"]) {
      expect(configuredCopy.values.get(key)).toBe(COPY[key as keyof typeof COPY]);
    }
  });

  test.each(["error", "unknown"])("disables the non-retryable Allow action in the %s page", async (state) => {
    const { api, swift } = await makeHarness();
    try {
      api.setState(state);
      const content = swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1);
      expect(content?.args.slice(0, 4)).toEqual([COPY.errorTitle, COPY.errorBody, false, false]);
    } finally { api.close(); }
  });

  test("updates SwiftUI initial content when the controller enters error", async () => {
    const { api, swift } = await makeHarness();
    try {
      const contents = () => swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:");
      expect(contents().at(-1)?.args.slice(0, 2)).toEqual([COPY.title, COPY.body]);
      api.setState("error");
      expect(contents().at(-1)?.args.slice(0, 2)).toEqual([COPY.errorTitle, COPY.errorBody]);
      expect(contents().at(-1)?.args[2]).toBe(false);
      expect(contents().at(-1)?.args[3]).toBe(false);
    } finally { api.close(); }
  });

  test("keeps the native SwiftUI card host contract while Allow is enabled", async () => {
    const { api, swift } = await makeHarness();
    try {
      const content = swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1);
      expect(content?.args[0]).toBe(COPY.title);
      expect(content?.args[2]).toBe(true);
      expect(swift!.initialCards[0].frame().size).toEqual({ width: 518, height: 80 });
      expect(swift!.initialViews[0].permissionCardView()).toBe(swift!.initialCards[0]);
    } finally { api.close(); }
  });

  test("keeps the SwiftUI initial view at its preferred size", async () => {
    const { api, panel, swift } = await makeHarness({ bodyHeight: 32 });
    try {
      expect(panel.contentViewValue).toBe(swift!.initialViews[0]);
      expect(panel.contentViewValue?.frame().origin).toEqual({ x: 0, y: 0 });
      expect(swift!.initialViews[0].preferredContentSize()).toEqual({ width: 600, height: 340 });
      expect(swift!.initialViews[0].permissionCardView()).toBe(swift!.initialCards[0]);
    } finally { api.close(); }
  });

  test("passes initial title and body through the SwiftUI content selector", async () => {
    const { api, swift } = await makeHarness({ bodyHeight: 32 });
    try {
      const content = swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1);
      expect(content?.args[0]).toBe(COPY.title);
      expect(content?.args[1]).toBe(COPY.body);
      expect(swift!.initialCards[0].frame().size).toEqual({ width: 518, height: 80 });
    } finally { api.close(); }
  });

  test("adopts localized initial height from SwiftUI preferredContentSize", async () => {
    for (const bodyHeight of [16, 32, 64]) {
      const swift = swiftPermissionViewsLibrary();
      const originalInitial = swift.library.IncodexPermissionInitialView.alloc;
      swift.library.IncodexPermissionInitialView.alloc = () => {
        const view = originalInitial();
        view.selectors.set("preferredContentSize", () => ({ width: 600, height: 312 + bodyHeight }));
        return view;
      };
      const { api, panel } = await makeHarness({ nativeLibrary: swift.library, bodyHeight });
      try {
        expect(panel.frame().size).toEqual({ width: 600, height: 312 + bodyHeight });
        expect(swift.initialViews[0].preferredContentSize()).toEqual({ width: 600, height: 312 + bodyHeight });
        expect(swift.initialCards[0].frame().size).toEqual({ width: 518, height: 80 });
      } finally { api.close(); }
    }
  });

  test("keeps the SwiftUI initial width and fixed permission card size", async () => {
    const { api, panel, swift } = await makeHarness({ bodyHeight: 34 });
    try {
      expect(panel.frame().size.width).toBe(600);
      const preferred = swift!.initialViews[0].preferredContentSize() as { width: number };
      expect(preferred.width).toBe(600);
      expect(swift!.initialCards[0].frame().size).toEqual({ width: 518, height: 80 });
      expect(swift!.initialViews[0].permissionCardView()).toBe(swift!.initialCards[0]);
    } finally { api.close(); }
  });

  test("passes permission row copy to the SwiftUI initial view", async () => {
    const { api, swift } = await makeHarness();
    try {
      const configure = swift!.calls.find((call) => call.selector === "configureWithCopy:appIcon:permissionIcon:actionTarget:");
      const copy = configure?.args[0] as FakeNative;
      expect(copy.values.get("permissionTitle")).toBe(COPY.permissionTitle);
      expect(copy.values.get("permissionDescription")).toBe(COPY.permissionDescription);
      expect(swift!.initialCards[0].frame().size).toEqual({ width: 518, height: 80 });
    } finally { api.close(); }
  });

  test("does not recreate initial material or text controls in AppKit", async () => {
    const { api, bridge, swift } = await makeHarness({ bodyHeight: 34 });
    try {
      expect(bridge.objects.some(value => value.type === "NSVisualEffectView")).toBe(false);
      expect(bridge.objects.some(value => value.type === "NSTextField")).toBe(false);
      const configure = swift!.calls.find((call) => call.selector === "configureWithCopy:appIcon:permissionIcon:actionTarget:");
      expect(configure?.args[0]).toBeDefined();
      expect(swift!.initialViews[0].permissionCardView()).toBe(swift!.initialCards[0]);
    } finally { api.close(); }
  });

  test("captures pending source geometry and native snapshot before entering the 531x110 helper", async () => {
    const handoffs: any[] = [];
    const harness = await makeHarness({
      onHandoff: (payload) => handoffs.push(payload),
    });
    const { api, swift } = harness;
    performSwiftAction(harness, "allow:");
    await expect(api.choice).resolves.toBe("repair");
    api.setState("awaiting-user");
    await flushNativeAsync();

    expect(swift!.calls.some(({ selector }) => selector === "cacheDisplayInRect:toBitmapImageRep:")).toBe(true);
    expect(handoffs).toHaveLength(1);
    expect(handoffs[0].source.frame.size.width).toBeGreaterThan(0);
    expect(handoffs[0].source.frame.size.height).toBeGreaterThan(0);
    expect(handoffs[0].source.image).toBeDefined();
    const capture = handoffs[0].source.image.values.get("representation").values.get("capturedView") as FakeNative;
    // Original full-window recording: the entire permission card transforms
    // into the helper, and returns to the same card slot on Back.
    expect(handoffs[0].source.frame.size).toEqual({ width: 518, height: 80 });
    expect(handoffs[0].source.radius).toBe(24);
    expect(capture).toBe(swift!.initialCards[0]);
    expect(handoffs[0].target.frame.size).toEqual({ width: 531, height: 110 });
    // Original helper capture writes 14 to TransitionCapture.cornerRadius
    // (+0x28); this is not the live helper window's corner radius.
    expect(handoffs[0].target.radius).toBe(14);
    expect(handoffs[0].target.panel).toBeDefined();
    expect(handoffs[0].target.panel.contentViewValue.type).toBe("IncodexPermissionHelperView");
    expect(handoffs[0].target.view).toBe(handoffs[0].target.panel.contentViewValue);
  });

  test("helper flight capture reads the current native foreground at the panel's current scale", async () => {
    const handoffs: any[] = [];
    const harness = await makeHarness({ onHandoff: payload => handoffs.push(payload) });
    const { api, swift } = harness;
    try {
      performSwiftAction(harness, "allow:");
      await expect(api.choice).resolves.toBe("repair");
      api.setState("awaiting-user");
      await flushNativeAsync();
      expect(handoffs).toHaveLength(1);
      const target = handoffs[0].target;
      expect(typeof target.captureImage).toBe("function");
      const image1 = {}, image2 = {};
      swift!.helperViews[0].values.set("foregroundSnapshot", image1);
      expect(target.captureImage()).toBe(image1);
      target.panel.values.set("backingScaleFactor", 1);
      swift!.helperViews[0].values.set("foregroundSnapshot", image2);
      expect(target.captureImage()).toBe(image2);
      expect(swift!.calls.filter(call => call.selector === "snapshotImageWithScale:").map(call => call.args)).toEqual([[2], [1]]);
    } finally { api.close(); }
  });

  test("uses a native file URL drag source fixed to the official ChatGPT bundle", async () => {
    const harness = await makeHarness();
    const { api, bridge } = harness;
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

  test("converts the inner NSBox content bounds into the drag source coordinates", async () => {
    const harness = await makeHarness();
    const { api, bridge, swift } = harness;
    try {
      api.setState("awaiting-user");
      await flushNativeAsync();
      const source = bridge.objects.find(value => value.hasSelector("mouseDown:"));
      const content = swift!.helperRows[0];
      if (!source || !content) throw new Error("helper drag content is missing");
      content.frameValue = frame(449, 30, 5, 6);
      const conversions: unknown[][] = [];
      content.convertRect$toView$ = (bounds, target) => {
        conversions.push([bounds, target]);
        return frame(449, 30, 5, 6);
      };
      source.invoke("mouseDown:", {});
      const item = bridge.objects.find(value => value.type === "NSDraggingItem");
      expect(conversions.length).toBe(1);
      expect(conversions[0]?.[0]).toEqual(frame(449, 30));
      expect(conversions[0]?.[1] === source).toBe(true);
      expect(item?.values.get("draggingFrame")).toEqual(frame(449, 30, 5, 6));
    } finally { api.close(); }
  });

  test("passes localized Back label into native config", async () => {
    const harness = await makeHarness({ copy: { ...COPY, later: "稍後", back: "返回" } });
    const { api, swift } = harness;
    try {
      api.setState("awaiting-user");
      await flushNativeAsync();
      const configure = swift!.calls.find((call) => call.selector === "configureWithCopy:appIcon:actionTarget:");
      const configuredCopy = configure?.args[0];
      if (!(configuredCopy instanceof FakeNative)) throw new Error("native helper copy is missing");
      expect(configuredCopy.values.get("back")).toBe("返回");
      expect(swift!.helperViews[0].type).toBe("IncodexPermissionHelperView");
    } finally {
      api.close();
    }
  });

  test("Back recaptures the original target and waits for a reverse handoff before cleanup", async () => {
    const reverses: any[] = [];
    let finish!: () => void;
    const finished = new Promise<void>((resolve) => { finish = resolve; });
    const harness = await makeHarness({
      onBack: (payload) => { reverses.push(payload); return { finished, dispose: () => {} }; },
    });
    const { api, panel } = harness;
    try {
      performSwiftAction(harness, "allow:");
      await expect(api.choice).resolves.toBe("repair");
      api.setState("awaiting-user");
      await flushNativeAsync();

      performSwiftAction(harness, "later:");
      await settleNativeAsync();

      expect(reverses).toHaveLength(1);
      expect(reverses[0].reverse).toBe(true);
      expect(reverses[0].source.frame.size).toEqual({ width: 518, height: 80 });
      expect(reverses[0].source.image).toBeDefined();
      expect(reverses[0].target.frame.size).toEqual({ width: 531, height: 110 });
      expect(reverses[0].target.radius).toBe(14);
      expect(api.isDestroyed()).toBe(false);

      const priorKeyCount = Number(panel.values.get("keyCount"));
      finish();
      await settleNativeAsync();
      expect(api.isDestroyed()).toBe(false);
      expect(Number(panel.values.get("keyCount"))).toBe(priorKeyCount + 1);
    } finally {
      finish();
      api.close();
    }
  });

  test("Back restores the initial page and a later Allow dispatches onRetry without resolving choice again", async () => {
    const handoffs: any[] = [];
    const retries: string[] = [];
    let finish!: () => void;
    const finished = new Promise<void>((resolve) => { finish = resolve; });
    let initialPanel: FakeNative | undefined;
    let initialWasVisibleAtBack = false;
    const harness = await makeHarness({
      onHandoff: (payload) => handoffs.push(payload),
      onBack: () => {
        initialWasVisibleAtBack = Boolean(initialPanel?.isVisible());
        return { finished, dispose: () => {} };
      },
    });
    const { api, bridge, panel } = harness;
    initialPanel = panel;
    const unsubscribe = api.onRetry(() => {
      retries.push("retry");
      // The controller opens Settings and then drives the adapter back into its
      // existing awaiting-user state after that async step succeeds.
      api.setState("awaiting-user");
    });
    try {
      performSwiftAction(harness, "allow:");
      await expect(api.choice).resolves.toBe("repair");
      api.setState("awaiting-user");
      await flushNativeAsync();

      performSwiftAction(harness, "later:");
      await settleNativeAsync();

      const contentCalls = () => harness.swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:");
      expect(initialWasVisibleAtBack).toBe(true);
      expect(panel.isVisible()).toBe(true);
      expect(contentCalls().at(-1)?.args[2]).toBe(true);

      finish();
      await settleNativeAsync();
      expect(api.isDestroyed()).toBe(false);
      expect(panel.isVisible()).toBe(true);
      expect(contentCalls().at(-1)?.args[1]).toBe(COPY.body);
      expect(contentCalls().at(-1)?.args[2]).toBe(true);
      expect(helperPanels(bridge).some((value) => value.visible && !value.destroyed)).toBe(false);

      performSwiftAction(harness, "allow:");
      await flushNativeAsync();
      await settleNativeAsync();
      expect(retries).toEqual(["retry"]);
      expect(handoffs).toHaveLength(2);
      await expect(api.choice).resolves.toBe("repair");
    } finally {
      unsubscribe?.();
      finish();
      api.close();
    }
  });

  test("Back handoff failure returns to an interactive initial page", async () => {
    const harness = await makeHarness({
      onBack: () => { throw new Error("reverse handoff failed"); },
    });
    const { api, bridge, panel } = harness;
    try {
      performSwiftAction(harness, "allow:");
      await expect(api.choice).resolves.toBe("repair");
      api.setState("awaiting-user");
      await flushNativeAsync();

      performSwiftAction(harness, "later:");
      await settleNativeAsync();

      const content = harness.swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1);
      expect(api.isDestroyed()).toBe(false);
      expect(panel.isVisible()).toBe(true);
      expect(content?.args[2]).toBe(true);
      expect(helperPanels(bridge).some((value) => value.visible && !value.destroyed)).toBe(false);
    } finally {
      api.close();
    }
  });

  test("reduced motion returns to the initial page without closing the guide", async () => {
    const harness = await makeHarness({ reduceMotion: true });
    const { api, bridge, panel, swift } = harness;
    try {
      performSwiftAction(harness, "allow:");
      await expect(api.choice).resolves.toBe("repair");
      api.setState("awaiting-user");
      await flushNativeAsync();

      performSwiftAction(harness, "later:");
      await settleNativeAsync();

      const content = harness.swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1);
      expect(api.isDestroyed()).toBe(false);
      expect(panel.isVisible()).toBe(true);
      expect(content?.args[2]).toBe(true);
      expect(helperPanels(bridge).some((value) => value.visible && !value.destroyed)).toBe(false);
      expect(swift?.arrowViews[0].calls.some(({ selector }) => selector === "animateToScaleX:scaleY:")).toBe(false);
      expect(swift?.arrowViews[0].calls.some(({ selector }) => selector === "resetToIdentity")).toBe(true);
    } finally {
      api.close();
    }
  });

  test("a stalled reverse handoff times out back to the initial page", async () => {
    let finish!: () => void;
    const finished = new Promise<void>((resolve) => { finish = resolve; });
    let disposed = 0;
    const harness = await makeHarness({
      onBack: () => ({ finished, dispose: () => { disposed += 1; } }),
    });
    const { api, bridge, panel } = harness;
    const originalSetTimeout = globalThis.setTimeout;
    const originalClearTimeout = globalThis.clearTimeout;
    let timeoutCallback: (() => void) | undefined;
    const timeoutToken = {};
    globalThis.setTimeout = ((callback: TimerHandler, delay?: number) => {
      if (Number(delay) === 5000) {
        timeoutCallback = callback as () => void;
        return timeoutToken as unknown as ReturnType<typeof setTimeout>;
      }
      return originalSetTimeout(callback, delay);
    }) as typeof setTimeout;
    globalThis.clearTimeout = ((value?: ReturnType<typeof setTimeout>) => {
      if (value === timeoutToken) return;
      return originalClearTimeout(value);
    }) as typeof clearTimeout;
    try {
      performSwiftAction(harness, "allow:");
      await expect(api.choice).resolves.toBe("repair");
      api.setState("awaiting-user");
      await flushNativeAsync();

      performSwiftAction(harness, "later:");
      await settleNativeAsync();
      if (!timeoutCallback) throw new Error("reverse timeout was not scheduled");
      timeoutCallback();
      await settleNativeAsync();

      const content = harness.swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1);
      expect(disposed).toBe(1);
      expect(api.isDestroyed()).toBe(false);
      expect(panel.isVisible()).toBe(true);
      expect(content?.args[2]).toBe(true);
      expect(helperPanels(bridge).some((value) => value.visible && !value.destroyed)).toBe(false);
      finish();
    } finally {
      globalThis.setTimeout = originalSetTimeout;
      globalThis.clearTimeout = originalClearTimeout;
      finish();
      api.close();
    }
  });

  test("a synchronous forward handoff failure cleans up the helper and stops tracking", async () => {
    const clock = installPollingClock();
    const harness = await makeHarness({
      onHandoff: () => { throw new Error("forward handoff failed"); },
    });
    const { api, bridge, panel } = harness;
    try {
      performSwiftAction(harness, "allow:");
      await expect(api.choice).resolves.toBe("repair");
      api.setState("awaiting-user");
      await settleNativeAsync();

      const poll = clock.timers.find((timer) => timer.delay === 100);
      if (!poll) throw new Error("native Settings tracking timer is missing");
      await settleNativeAsync();

      expect(panel.isVisible()).toBe(true);
      expect(helperPanels(bridge).some((value) => !value.destroyed)).toBe(false);
      expect(poll.active).toBe(false);
    } finally {
      api.close();
      clock.restore();
    }
  });

  test("disables helper hit testing for an active drag and restores it when the drag ends", async () => {
    const { api, bridge } = await makeHarness();
    api.setState("awaiting-user");
    await flushNativeAsync();
    const row = bridge.objects.find((value) => value.hasSelector("mouseDown:"));
    const helper = helperPanels(bridge).find((value) => value.frame().size.width === 531);
    if (!row || !helper) throw new Error("native drag helper is missing");

    row.invoke("draggingSession:willBeginAtPoint:", { x: 0, y: 0 });
    expect(helper.values.get("ignoresMouseEvents")).toBe(true);
    row.invoke("draggingSession:endedAtPoint:operation:", { x: 0, y: 0 }, 0);
    expect(helper.values.get("ignoresMouseEvents")).toBe(false);
    api.close();
  });

  test("closing during a drag prevents a late drag callback from reviving native UI", async () => {
    const harness = await makeHarness();
    const { api, bridge, swift } = harness;
    api.setState("awaiting-user");
    await flushNativeAsync();
    const row = bridge.objects.find((value) => value.hasSelector("mouseDown:"));
    const helper = helperPanels(bridge).find((value) => value.frame().size.width === 531);
    const appRow = swift?.helperRows[0];
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

  test("schedules the next arrow pulse when the 250ms return phase starts", async () => {
    const { api, swift } = await makeHarness();
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
      expect(swift?.arrowViews[0].calls.at(-1)).toMatchObject({
        selector: "animateToScaleX:scaleY:",
        args: [1.15, 1.6],
      });
      expect(scheduled.some((entry) => entry.delay === 250)).toBe(true);
      expect(scheduled.some((entry) => entry.delay === 4000)).toBe(false);
      // This checks when return starts, not when the native spring settles.
      scheduled.find((entry) => entry.delay === 250)?.callback();
      expect(swift?.arrowViews[0].calls.at(-1)).toMatchObject({
        selector: "animateToScaleX:scaleY:",
        args: [1, 1],
      });
      expect(scheduled.some((entry) => entry.delay === 4000)).toBe(true);
      api.close();
      expect(swift?.arrowViews[0].calls.at(-1)).toMatchObject({ selector: "resetToIdentity", args: [] });
    } finally {
      globalThis.setTimeout = originalSetTimeout;
      globalThis.clearTimeout = originalClearTimeout;
      api.close();
    }
  });

  test("keeps one arrow timer through hover replacement and drag start/end", async () => {
    const harness = await makeHarness();
    const { api, bridge } = harness;
    const clock = installArrowClock();
    try {
      api.setState("awaiting-user");
      await settleNativeAsync();
      const row = bridge.objects.find((value) => value.hasSelector("draggingSession:willBeginAtPoint:"));
      const tracker = bridge.objects.find((value) => value.hasSelector("mouseEntered:"));
      if (!row || !tracker) throw new Error("native arrow timer fixtures are missing");

      expect(clock.active().map((timer) => timer.delay)).toEqual([500]);
      row.invoke("draggingSession:willBeginAtPoint:", { x: 0, y: 0 });
      expect(clock.active()).toHaveLength(0);

      row.invoke("draggingSession:endedAtPoint:operation:", { x: 0, y: 0 }, 0);
      expect(clock.active().map((timer) => timer.delay)).toEqual([4000]);

      tracker.invoke("mouseEntered:", {});
      expect(clock.active().map((timer) => timer.delay)).toEqual([250]);
      tracker.invoke("mouseEntered:", {});
      expect(clock.active().map((timer) => timer.delay)).toEqual([250]);

      row.invoke("draggingSession:willBeginAtPoint:", { x: 0, y: 0 });
      expect(clock.active()).toHaveLength(0);
      row.invoke("draggingSession:endedAtPoint:operation:", { x: 0, y: 0 }, 0);
      expect(clock.active().map((timer) => timer.delay)).toEqual([4000]);
    } finally {
      try {
        api.close();
        expect(clock.active()).toHaveLength(0);
      } finally {
        clock.restore();
      }
    }
  });

  test("reduced motion keeps the arrow timer queue empty", async () => {
    const harness = await makeHarness({ reduceMotion: true });
    const { api, bridge } = harness;
    const clock = installArrowClock();
    try {
      api.setState("awaiting-user");
      await settleNativeAsync();
      const row = bridge.objects.find((value) => value.hasSelector("draggingSession:willBeginAtPoint:"));
      const tracker = bridge.objects.find((value) => value.hasSelector("mouseEntered:"));
      if (!row || !tracker) throw new Error("native arrow timer fixtures are missing");

      expect(clock.active()).toHaveLength(0);
      tracker.invoke("mouseEntered:", {});
      expect(clock.active()).toHaveLength(0);
      row.invoke("draggingSession:willBeginAtPoint:", { x: 0, y: 0 });
      row.invoke("draggingSession:endedAtPoint:operation:", { x: 0, y: 0 }, 0);
      expect(clock.active()).toHaveLength(0);
    } finally {
      try {
        api.close();
        expect(clock.active()).toHaveLength(0);
      } finally {
        clock.restore();
      }
    }
  });

  test("close drains arrow timers and stale callbacks cannot revive a pulse", async () => {
    const harness = await makeHarness();
    const { api, swift } = harness;
    const clock = installArrowClock();
    try {
      api.setState("awaiting-user");
      await settleNativeAsync();
      const pulse = clock.active().find((timer) => timer.delay === 500);
      if (!pulse) throw new Error("initial arrow pulse timer is missing");
      clock.fire(pulse);
      const returnTimer = clock.active().find((timer) => timer.delay === 250);
      if (!returnTimer) throw new Error("arrow return timer is missing");

      api.close();
      expect(clock.active()).toHaveLength(0);
      const callsAtClose = swift?.arrowViews[0].calls.length ?? 0;
      pulse.callback();
      returnTimer.callback();
      expect(clock.active()).toHaveLength(0);
      expect(swift?.arrowViews[0].calls).toHaveLength(callsAtClose);
    } finally {
      try {
        api.close();
        expect(clock.active()).toHaveLength(0);
      } finally {
        clock.restore();
      }
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

test.each(["flight", "panel-order-out", "panel-close", "close-handler"])("guide teardown continues after a %s cleanup failure", async (failure) => {
  const clock = installPollingClock();
  let finish!: () => void;
  const finished = new Promise<void>(resolve => { finish = resolve; });
  let disposed = 0;
  const harness = await makeHarness({ onHandoff: () => ({ finished, dispose: () => {
    disposed++;
    finish();
    if (failure === "flight") throw new Error("injected flight cleanup error");
  } }) });
  const { api, bridge } = harness;
  try {
    performSwiftAction(harness, "allow:");
    await api.choice;
    api.setState("awaiting-user");
    await settleNativeAsync();
    const panels = bridge.objects.filter(value => value.type === "NSPanel");
    expect(panels.length).toBeGreaterThan(1);
    const first = panels[0];
    if (failure === "panel-order-out") first.orderOut$ = () => { throw new Error("injected orderOut error"); };
    if (failure === "panel-close") {
      const close = first.close.bind(first);
      first.close = () => { close(); throw new Error("injected panel close error"); };
    }
    let callbacks = 0;
    api.onClose(() => { if (failure === "close-handler") throw new Error("injected close handler error"); });
    api.onClose(() => { callbacks++; });
    expect(() => api.close()).not.toThrow();
    api.close();
    await settleNativeAsync();
    expect(disposed).toBe(1);
    expect(callbacks).toBe(1);
    expect(api.isDestroyed()).toBe(true);
    expect(panels.every(panel => panel.isDestroyed())).toBe(true);
    expect(panels.every(panel => !panel.visible)).toBe(true);
  } finally { finish(); api.close(); clock.restore(); }
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
    performSwiftAction(harness, "allow:");
    await api.choice;
    api.setState("awaiting-user");
    await settleNativeAsync();
    const helper = helperPanels(harness.bridge).find((panel) => panel.frame().size.width === 531);
    expect(helper).toBeDefined();
    const before = helper?.frame().origin.x;
    x += 200;
    clock.timers[0]?.callback();
    await settleNativeAsync();
    expect(locations).toBe(2);
    expect(helper?.frame().origin.x).not.toBe(before);
  } finally { finish(); api?.close(); clock.restore(); }
});

test("Back freezes the helper target geometry while the reverse flight is pending", async () => {
  const clock = installPollingClock();
  let target = { x: 120, y: 140, width: 920, height: 700 };
  let reverse: any;
  let finish!: () => void;
  const finished = new Promise<void>((resolve) => { finish = resolve; });
  let api: Awaited<ReturnType<typeof createNativeAccessibilitySetupWindow>> | undefined;
  try {
    const harness = await makeHarness({
      locateSettings: () => target,
      onBack: (payload) => { reverse = payload; return { finished, dispose: () => {} }; },
    });
    api = harness.api;
    performSwiftAction(harness, "allow:");
    await api.choice;
    api.setState("awaiting-user");
    await settleNativeAsync();
    const poll = clock.timers.find((timer) => timer.active && timer.delay === 100);
    if (!poll) throw new Error("native Back tracking harness is missing");

    performSwiftAction(harness, "later:");
    await settleNativeAsync();
    const frozen = {
      origin: { ...reverse.target.frame.origin },
      size: { ...reverse.target.frame.size },
    };
    const frozenPanelFrame = reverse.target.panel.frame();
    target = { ...target, x: target.x + 280 };
    poll.callback();
    await settleNativeAsync();

    expect(reverse.target.frame).toEqual(frozen);
    expect(reverse.target.panel.frame()).toEqual(frozenPanelFrame);
    finish();
    await settleNativeAsync();
  } finally {
    finish();
    api?.close();
    clock.restore();
  }
});

test("Back keeps the reverse flight alive when Settings disappears", async () => {
  const clock = installPollingClock();
  let target: { x: number; y: number; width: number; height: number } | null = {
    x: 120,
    y: 140,
    width: 920,
    height: 700,
  };
  let disposed = 0;
  let finish!: () => void;
  const finished = new Promise<void>((resolve) => { finish = resolve; });
  let api: Awaited<ReturnType<typeof createNativeAccessibilitySetupWindow>> | undefined;
  try {
    const harness = await makeHarness({
      locateSettings: () => target,
      onBack: () => ({ finished, dispose: () => { disposed += 1; } }),
    });
    api = harness.api;
    performSwiftAction(harness, "allow:");
    await api.choice;
    api.setState("awaiting-user");
    await settleNativeAsync();
    const poll = clock.timers.find((timer) => timer.active && timer.delay === 100);
    if (!poll) throw new Error("native Back tracking harness is missing");

    performSwiftAction(harness, "later:");
    await settleNativeAsync();
    target = null;
    for (let index = 0; index < 10; index += 1) {
      poll.callback();
      await settleNativeAsync();
    }

    expect(disposed).toBe(0);
    expect(api.isDestroyed()).toBe(false);
    finish();
    await settleNativeAsync();
    expect(api.isDestroyed()).toBe(false);
  } finally {
    finish();
    api?.close();
    clock.restore();
  }
});


  test("Back keeps the SwiftUI card host hidden until the reverse flight lands", async () => {
  let finish!: () => void;
  const finished = new Promise<void>(resolve => { finish = resolve; });
  const harness = await makeHarness({ onBack: () => ({ finished, dispose() {} }) });
  const { api, swift } = harness;
  try {
    const card = swift!.initialCards[0];
    performSwiftAction(harness, "allow:");
    await api.choice;
    api.setState("awaiting-user");
    await flushNativeAsync();
    performSwiftAction(harness, "later:");
    await settleNativeAsync();
    expect(card.values.get("hidden")).toBe(true);
    expect(swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1)?.args[3]).toBe(false);
    finish();
    await settleNativeAsync();
    expect(card.values.get("hidden")).toBe(false);
    expect(swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1)?.args[3]).toBe(false);
  } finally { finish(); api.close(); }
});

test("handoff toggles SwiftUI settingsPlaceholder and Back restores the card host", async () => {
  const harness = await makeHarness({ reduceMotion: true });
  const { api, swift } = harness;
  try {
    const card = swift!.initialCards[0];
    performSwiftAction(harness, "allow:");
    api.setState("awaiting-user");
    await flushNativeAsync();
    const awaiting = swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1);
    expect(awaiting?.args[1]).toBe(COPY.body);
    expect(awaiting?.args[3]).toBe(true);
    expect(card.values.get("hidden")).toBe(true);
    performSwiftAction(harness, "later:");
    await flushNativeAsync();
    expect(card.values.get("hidden")).toBe(false);
    const restored = swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1);
    expect(restored?.args[1]).toBe(COPY.body);
    expect(restored?.args[3]).toBe(false);
  } finally { api.close(); }
});

test("awaiting Settings updates SwiftUI placeholder state and Back clears it", async () => {
  const harness = await makeHarness({ reduceMotion: true });
  const { api, swift } = harness;
  try {
    performSwiftAction(harness, "allow:");
    api.setState("awaiting-user");
    await flushNativeAsync();
    const awaiting = swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1);
    expect(awaiting?.args[3]).toBe(true);
    performSwiftAction(harness, "later:");
    await flushNativeAsync();
    const restored = swift!.calls.filter((call) => call.selector === "setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1);
    expect(restored?.args[3]).toBe(false);
  } finally { api.close(); }
});


test("adopts a localized SwiftUI initial preferred height", async () => {
  const swift = swiftPermissionViewsLibrary();
  const originalInitial = swift.library.IncodexPermissionInitialView.alloc;
  swift.library.IncodexPermissionInitialView.alloc = () => {
    const view = originalInitial();
    view.selectors.set("preferredContentSize", () => ({ width: 600, height: 372 }));
    return view;
  };
  const { api, panel } = await makeHarness({ nativeLibrary: swift.library, titleHeight: 60, bodyHeight: 32 });
  try {
    expect(panel.frame().size).toEqual({ width: 600, height: 372 });
  } finally {api.close();}
});


test("dispatches SwiftUI resumeSettings without creating a new repair choice", async () => {
  const harness=await makeHarness({reduceMotion:true});
  const {api,bridge,swift}=harness;
  let retries=0;
  api.onRetry(()=>{retries++;api.setState("awaiting-user");});
  try {
    performSwiftAction(harness,"allow:");
    await expect(api.choice).resolves.toBe("repair");
    api.setState("awaiting-user");await flushNativeAsync();
    const awaiting=swift!.calls.filter((call)=>call.selector==="setContentWithTitle:body:allowEnabled:settingsPlaceholder:").at(-1);
    expect(awaiting?.args[3]).toBe(true);
    expect(bridge.objects.some((value)=>value.type==="NSButton")).toBe(false);
    performSwiftAction(harness,"resumeSettings:");await flushNativeAsync();
    expect(retries).toBe(1);
    await expect(api.choice).resolves.toBe("repair");
    performSwiftAction(harness,"later:");
    await flushNativeAsync();
    performSwiftAction(harness,"resumeSettings:");
    expect(retries).toBe(1);
  } finally {api.close();}
});


test("passes helper dragInstruction through the SwiftUI copy dictionary", async () => {
  const harness=await makeHarness({reduceMotion:true});
  const {api,bridge,swift}=harness;
  try {
    performSwiftAction(harness,"allow:");api.setState("awaiting-user");await flushNativeAsync();
    const configure=swift!.calls.find((call)=>call.selector==="configureWithCopy:appIcon:actionTarget:");
    const copy=configure?.args[0] as FakeNative;
    expect(copy.values.get("dragInstruction")).toBe(COPY.dragInstruction ?? COPY.addedBody);
    expect(copy.values.get("dragInstructionRuns")).toBe(COPY.dragInstructionRuns);
    expect(bridge.objects.some((value)=>value.type==="NSTextField")).toBe(false);
  } finally {api.close();}
});


test("initial permission window yields to Settings while the helper remains above it", async () => {
  const harness=await makeHarness({reduceMotion:true});
  const {api,bridge,panel}=harness;
  try {
    expect(panel.values.get("level")).toBe(3);
    performSwiftAction(harness,"allow:");
    api.setState("repairing");expect(panel.values.get("level")).toBe(0);
    api.setState("awaiting-user");await flushNativeAsync();
    expect(panel.values.get("level")).toBe(0);
    const helper=helperPanels(bridge).find(v=>!v.destroyed)!;
    expect(helper.values.get("level")).toBe(3);
    const count=Number(panel.values.get("keyCount")||0);
    performSwiftAction(harness,"later:");await flushNativeAsync();
    expect(Number(panel.values.get("keyCount"))).toBeGreaterThan(count);
    expect(panel.values.get("level")).toBe(3);
  }finally{api.close();}
});
