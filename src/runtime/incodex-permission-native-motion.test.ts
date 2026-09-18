import { expect, test } from "bun:test";
import { createNativeReplicants, runNativePermissionHandoff } from "./incodex-permission-native-motion.cts";

type Rect = { origin: { x: number; y: number }; size: { width: number; height: number } };

const rect = (x: number, y: number, width: number, height: number): Rect => ({
  origin: { x, y }, size: { width, height },
});

function nativeMotionBridge(screenSpecs: Array<{ frame: Rect; scale: number }>, options: { throwOnSwiftSurfaceAdd?: boolean } = {}) {
  const objects: any[] = [];
  const calls: Array<{ selector: string; args: any[] }> = [];
  let currentScreens = screenSpecs.map((spec) => {
    const screen: any = nativeObject("NSScreen");
    screen.frameValue = spec.frame;
    screen.scaleValue = spec.scale;
    screen.frame = () => screen.frameValue;
    screen.backingScaleFactor = () => screen.scaleValue;
    return screen;
  });

  function copyRect(value: Rect): Rect {
    return rect(value.origin.x, value.origin.y, value.size.width, value.size.height);
  }

  function nativeObject(type: string): any {
    const state: any = {
      type,
      values: new Map<string, any>(),
      subviews: [],
      sublayers: [],
      frameValue: rect(0, 0, 0, 0),
      visible: false,
      closed: false,
    };
    const proxy = new Proxy(state, {
      get(target, property: string | symbol, receiver) {
        if (property === "then") return undefined;
        if (property in target) {
          const value = target[property as keyof typeof target];
          return typeof value === "function" ? value.bind(receiver) : value;
        }
        if (typeof property !== "string") return undefined;
        return (...args: any[]) => invoke(receiver, property, args);
      },
    });
    objects.push(proxy);
    state.layer = () => {
      if (!state.layerValue) state.layerValue = nativeObject("CALayer");
      return state.layerValue;
    };
    return proxy;
  }

  function invoke(object: any, selector: string, args: any[]): any {
    calls.push({ selector, args });
    const key = selector;
    if (selector.startsWith("initWithContentRect")) {
      object.frameValue = copyRect(args[0]);
      return object;
    }
    if (selector === "initWithFrame$") {
      object.frameValue = copyRect(args[0]);
      return object;
    }
    if (selector === "initWithSize$") {
      object.values.set("size", args[0]);
      return object;
    }
    if (selector === "size") return object.values.get("size");
    if (selector === "frame") return copyRect(object.frameValue);
    if (selector === "bounds") return rect(0, 0, object.frameValue.size.width, object.frameValue.size.height);
    if (selector === "layer") {
      if (!object.layerValue) object.layerValue = nativeObject("CALayer");
      return object.layerValue;
    }
    if (selector === "addSublayer$") {
      (object.sublayers ||= []).push(args[0]); return;
    }
    if (selector === "addSubview$") {
      if (options.throwOnSwiftSurfaceAdd && args[0]?.type === "IncodexPermissionFlightView") {
        throw new Error("injected SwiftUI surface construction failure");
      }
      object.subviews.push(args[0]);
      return undefined;
    }
    if (selector === "setContentView$") {
      object.contentViewValue = args[0];
      return undefined;
    }
    if (selector === "setFrame$") {
      object.frameValue = copyRect(args[0]);
      return undefined;
    }
    if (selector === "setContentsScale$") {
      object.values.set("contentsScale", args[0]);
      return undefined;
    }
    if (selector === "setValue$forKey$") {
      object.values.set(String(args[1]), args[0]);
      return undefined;
    }
    if (selector === "orderFront$") {
      object.visible = true;
      return undefined;
    }
    if (selector === "orderOut$") {
      object.visible = false;
      return undefined;
    }
    if (selector === "close") {
      object.visible = false;
      object.closed = true;
      return undefined;
    }
    if (selector === "bitmapImageRepForCachingDisplayInRect$") return nativeObject("NSBitmapImageRep");
    if (selector === "cacheDisplayInRect$toBitmapImageRep$") return undefined;
    if (selector === "count") return Array.isArray(object.values.get("items")) ? object.values.get("items").length : 0;
    if (selector === "objectAtIndex$") return object.values.get("items")?.[args[0]];
    if (selector === "displayIfNeeded") return undefined;
    if (selector === "accessibilityDisplayShouldReduceTransparency") return false;
    if (selector === "arrayWithObject$") {
      const array = nativeObject("NSArray");
      array.values.set("items", [args[0]]);
      return array;
    }
    if (selector === "numberWithDouble$") return args[0];
    if (selector === "stringWithUTF8String$") return String(args[0]);
    if (selector === "filterWithName$") return nativeObject("CIFilter");
    if (selector === "begin" || selector === "commit" || selector === "setDisableActions$") return undefined;
    object.values.set(key, args.length <= 1 ? args[0] : args);
    return undefined;
  }

  function classObject(type: string): any {
    const target: any = {
      alloc: () => nativeObject(type),
    };
    const proxy = new Proxy(target, {
      get(value, property: string | symbol) {
        if (property in value) return value[property as keyof typeof value];
        if (typeof property !== "string") return undefined;
        return (...args: any[]) => {
          if (type === "NSScreen" && property === "screens") {
            const array = nativeObject("NSArray");
            array.values.set("items", currentScreens);
            return array;
          }
          if (type === "NSWorkspace" && property === "sharedWorkspace") return nativeObject("NSWorkspace");
          if (type === "NSColor" && property === "clearColor") return nativeObject("NSColor");
          if (type === "NSArray" && property === "arrayWithObject$") return invoke(proxy, property, args);
          if (type === "NSString" && property === "stringWithUTF8String$") return String(args[0]);
          if (type === "NSNumber" && property === "numberWithDouble$") return args[0];
          if (type === "CIFilter" && property === "filterWithName$") return nativeObject("CIFilter");
          if ((type === "CALayer" || type === "CAShapeLayer") && property === "layer") return nativeObject(type);
          if (type === "CATransaction") return undefined;
          return undefined;
        };
      },
    });
    return proxy;
  }

  const objc: any = {
    NobjcLibrary: new Proxy(function NobjcLibrary() {}, {
      construct: () => new Proxy({}, { get: (_target, property: string | symbol) =>
        typeof property === "string" ? classObject(property) : undefined }),
    }),
    NobjcClass: {
      define(definition: { name: string }) { return classObject(definition.name); },
    },
    callFunction(name: string, signature: any, ...args: any[]) {
      calls.push({ selector: name, args: [signature, ...args] });
      if (name === "CGColorCreateGenericRGB" || name === "CGPathCreateWithRoundedRect" || name === "CGPathCreateMutable") { const value=nativeObject(name); value.args=args; return value; }
      return undefined;
    },
  };
  return {
    objc,
    objects,
    calls,
    screens(next: Array<{ frame: Rect; scale: number }>) {
      currentScreens = next.map((spec) => {
        const screen: any = nativeObject("NSScreen");
        screen.frameValue = spec.frame;
        screen.scaleValue = spec.scale;
        screen.frame = () => screen.frameValue;
        screen.backingScaleFactor = () => screen.scaleValue;
        return screen;
      });
    },
    targetView(bounds: Rect, emptyRep = false) {
      const view = nativeObject("TargetView");
      view.frameValue = bounds;
      view.bitmapImageRepForCachingDisplayInRect$ = () => emptyRep ? null : nativeObject("NSBitmapImageRep");
      return view;
    },
    panels() { return objects.filter((value) => value.type === "NSPanel"); },
  };
}

function swiftUIFlightLibrary() {
  const instances: any[] = [];
  function copyRect(value: Rect): Rect {
    return rect(value.origin.x, value.origin.y, value.size.width, value.size.height);
  }
  function makeView() {
    const layerState: any = { type: "SwiftUIFlightLayer", values: new Map<string, any>() };
    const layer = new Proxy(layerState, {
      get(target, property: string | symbol, receiver) {
        if (property in target) {
          const value = target[property as keyof typeof target];
          return typeof value === "function" ? value.bind(receiver) : value;
        }
        if (typeof property !== "string") return undefined;
        return (...args: any[]) => { target.values.set(property, args[0]); };
      },
    });
    const state: any = {
      type: "IncodexPermissionFlightView",
      frameValue: rect(0, 0, 0, 0),
      calls: [] as Array<{ selector: string; args: any[] }>,
      imagePair: null,
      layer: () => layer,
    };
    const view = new Proxy(state, {
      get(target, property: string | symbol, receiver) {
        if (property === "then") return undefined;
        if (property in target) {
          const value = target[property as keyof typeof target];
          return typeof value === "function" ? value.bind(receiver) : value;
        }
        if (typeof property !== "string") return undefined;
        return (...args: any[]) => {
          target.calls.push({ selector: property, args });
          if (property === "initWithFrame$") {
            target.frameValue = copyRect(args[0]);
            return receiver;
          }
          if (property === "setFrame$") {
            target.frameValue = copyRect(args[0]);
            return undefined;
          }
          if (property === "setFrameSize$") {
            target.frameValue.size = { width: args[0].width, height: args[0].height };
            return undefined;
          }
          if (property === "setSourceImage$targetImage$") {
            target.imagePair = args;
            return undefined;
          }
          return undefined;
        };
      },
    });
    instances.push(view);
    return view;
  }
  return {
    library: { IncodexPermissionFlightView: { alloc: makeView } },
    instances,
  };
}

function harness(reducedMotion = false, reverse = false) {
  let time = 0;
  let closed = false;
  const pending = new Map<number, () => void>();
  let id = 0;
  const frames: any[] = [];
  let disposed = 0;
  let created = 0;
  const flight = runNativePermissionHandoff({
    source: { frame: { origin: { x: 10, y: 400 }, size: { width: 80, height: 28 } }, image: {}, radius: 14 },
    target: { frame: { origin: { x: 300, y: 20 }, size: { width: 532, height: 112 } }, view: {}, panel: {}, radius: 12 },
    reducedMotion,
    reverse,
    isClosed: () => closed,
    now: () => time,
    schedule: (callback: () => void) => { pending.set(++id, callback); return id; },
    cancel: (key: number) => pending.delete(key),
    createReplicants: () => {
      created++;
      return { render: (frame: any) => frames.push(frame), dispose: () => disposed++ };
    },
  });
  return { flight, frames, pending, created: () => created, disposed: () => disposed,
    closeHost() { closed = true; },
    advance(ms: number) { time += ms; const tasks = [...pending.values()]; pending.clear(); tasks.forEach(task => { task(); }); } };
}

test("both flight directions use the helper's foreground image provider instead of caching its material host", () => {
  for (const reverse of [false, true]) {
    const bridge = nativeMotionBridge([{ frame: rect(0, 0, 1440, 900), scale: 2 }]);
    const swift = swiftUIFlightLibrary();
    const sourceImage = { name: "original-card" };
    const helperImage = { name: "native-foreground" };
    let captures = 0;
    const replicas = createNativeReplicants({
      objc: bridge.objc, nativeLibrary: swift.library, reverse,
      source: { image: sourceImage },
      target: {
        view: bridge.targetView(rect(0, 0, 531, 110)),
        captureImage: () => { captures++; return helperImage; },
      },
    });
    try {
      expect(captures).toBe(1);
      expect(swift.instances[0].imagePair).toEqual(reverse ? [helperImage, sourceImage] : [sourceImage, helperImage]);
      expect(bridge.calls.some(call => call.selector === "cacheDisplayInRect$toBitmapImageRep$")).toBe(false);
    } finally { replicas.dispose(); }
  }
});

test("a failed native foreground image must not silently fall back to the material-bearing helper", () => {
  const bridge = nativeMotionBridge([{ frame: rect(0, 0, 1440, 900), scale: 2 }]);
  const swift = swiftUIFlightLibrary();
  expect(() => createNativeReplicants({
    objc: bridge.objc, nativeLibrary: swift.library, source: { image: {} },
    target: { view: bridge.targetView(rect(0, 0, 531, 110)), captureImage: () => null },
  })).toThrow("foreground snapshot");
  expect(bridge.panels()).toHaveLength(0);
});

test("native flight preserves AppKit screen coordinates at both endpoints", async () => {
  const h = harness();
  expect(h.frames[0].bounds).toEqual({ x: 10, y: 400, width: 80, height: 28 });
  for (let tick = 0; tick < 180; tick++) h.advance(1000 / 60);
  await h.flight.finished;
  expect(h.frames.at(-1).bounds).toEqual({ x: 300, y: 20, width: 532, height: 112 });
  expect(h.frames.at(-1).targetOpacity).toBe(1);
  expect(h.disposed()).toBe(1);
  expect(h.pending.size).toBe(0);
});

test("native handoff prefers the replicant display source and stops it on dispose", async () => {
  let emit: ((timestamp: number) => void) | undefined;
  let starts = 0;
  let stops = 0;
  let timerSchedules = 0;
  const frames: any[] = [];
  const frameSource = {
    start(callback: (timestamp: number) => void) {
      starts++;
      emit = callback;
      return true;
    },
    stop() { stops++; },
  };
  const flight = runNativePermissionHandoff({
    objc: {}, reducedMotion: false, now: () => 0,
    source: { frame: rect(10, 400, 80, 28) },
    target: { frame: rect(300, 20, 532, 112) },
    schedule: () => { timerSchedules++; return 1; }, cancel: () => {},
    createReplicants: () => ({ frameSource, render: (frame: any) => frames.push(frame), dispose() {} }),
  });
  expect(starts).toBe(1);
  expect(timerSchedules).toBe(0);
  emit?.(100);
  emit?.(100.25);
  expect(frames.at(-1).progress).toBeGreaterThan(0);
  flight.dispose();
  await flight.finished;
  expect(stops).toBe(1);
});

test("native handoff falls back when the display source declines", async () => {
  let pending: (() => void) | undefined;
  let schedules = 0;
  const frames: any[] = [];
  const flight = runNativePermissionHandoff({
    objc: {}, reducedMotion: false, now: () => 0,
    source: { frame: rect(10, 400, 80, 28) }, target: { frame: rect(300, 20, 532, 112) },
    schedule: (callback: () => void) => { schedules++; pending = callback; return schedules; }, cancel: () => {},
    createReplicants: () => ({
      frameSource: { start: () => false, stop: () => { throw new Error("declined source must not stop"); } },
      render: (frame: any) => frames.push(frame), dispose() {},
    }),
  });
  expect(schedules).toBe(1);
  pending?.();
  expect(schedules).toBe(2);
  flight.dispose();
  await flight.finished;
  expect(frames.length).toBeGreaterThan(1);
});

test("native display-source startup failure still disposes its replicants", async () => {
  let disposed = 0;
  let reported: unknown;
  const flight = runNativePermissionHandoff({
    objc: {}, reducedMotion: false, now: () => 0,
    source: { frame: rect(10, 400, 80, 28) }, target: { frame: rect(300, 20, 532, 112) },
    createReplicants: () => ({
      frameSource: { start: () => { throw new Error("display source unavailable"); }, stop() {} },
      render() {}, dispose() { disposed++; },
    }),
    onError: (error: unknown) => { reported = error; },
  });
  await flight.finished;
  expect((reported as Error).message).toBe("display source unavailable");
  expect(disposed).toBe(1);
});

test("native display-link startup exceptions invalidate a partially installed link", () => {
  const bridge = nativeMotionBridge([{ frame: rect(0, 0, 1440, 900), scale: 2 }]);
  const swift = swiftUIFlightLibrary();
  let invalidations = 0;
  const displayLink = {
    init() { return this; },
    startForWindow$handler$() { throw new Error("display-link install failed"); },
    invalidate() { invalidations++; },
    displayLinked() { return true; },
  };
  (swift.library as any).IncodexPermissionDisplayLink = { alloc: () => displayLink };
  bridge.objc.typedBlock = (_signature: any, callback: Function) => ({ callback });
  const replicas = createNativeReplicants({
    objc: bridge.objc,
    nativeLibrary: swift.library,
    source: { image: {} },
    target: { view: bridge.targetView(rect(0, 0, 531, 110)) },
  });
  try {
    expect(() => replicas.frameSource.start(() => {})).toThrow("display-link install failed");
    expect(invalidations).toBe(1);
  } finally {
    replicas.dispose();
  }
});

test("render failure from a display callback disposes the source and replicants", async () => {
  let emit: ((timestamp: number) => void) | undefined;
  let stopped = 0;
  let disposed = 0;
  let reported: unknown;
  let renders = 0;
  const flight = runNativePermissionHandoff({
    objc: {}, reducedMotion: false, now: () => 0,
    source: { frame: rect(10, 400, 80, 28) }, target: { frame: rect(300, 20, 532, 112) },
    createReplicants: () => ({
      frameSource: {
        start(callback: (timestamp: number) => void) { emit = callback; return true; },
        stop() { stopped++; },
      },
      render() { renders++; if (renders > 1) throw new Error("render failed"); },
      dispose() { disposed++; },
    }),
    onError: (error: unknown) => { reported = error; },
  });
  emit?.(1);
  emit?.(1.1);
  await flight.finished;
  expect((reported as Error).message).toBe("render failed");
  expect(stopped).toBe(1);
  expect(disposed).toBe(1);
  const renderCount = renders;
  emit?.(2);
  expect(renders).toBe(renderCount);
});

test("native dispose closes replicants even when the display source stop throws", async () => {
  let disposed = 0;
  const flight = runNativePermissionHandoff({
    objc: {}, reducedMotion: false, now: () => 0,
    source: { frame: rect(10, 400, 80, 28) }, target: { frame: rect(300, 20, 532, 112) },
    createReplicants: () => ({
      frameSource: { start: () => true, stop: () => { throw new Error("stop failed"); } },
      render() {}, dispose() { disposed++; },
    }),
  });
  expect(() => flight.dispose()).toThrow("stop failed");
  await flight.finished;
  expect(disposed).toBe(1);
  expect(() => flight.dispose()).not.toThrow();
});

test("native Back is a new helper-to-card flight with forward decoration progress", async () => {
  const h = harness(false, true);
  expect(h.frames[0].progress).toBe(0);
  expect(h.frames[0].bounds).toEqual({ x: 300, y: 20, width: 532, height: 112 });
  expect(h.frames[0].sourceOpacity).toBe(1);
  expect(h.frames[0].targetOpacity).toBe(0);
  for (let tick = 0; tick < 180; tick++) h.advance(1000 / 60);
  await h.flight.finished;
  expect(h.frames.at(-1).progress).toBe(1);
  expect(h.frames.at(-1).bounds).toEqual({ x: 10, y: 400, width: 80, height: 28 });
  expect(h.frames.at(-1).sourceOpacity).toBe(0);
  expect(h.frames.at(-1).targetOpacity).toBe(1);
});

test("native flight forwards the injected SwiftUI library to its replicant factory", async () => {
  const bridge = nativeMotionBridge([{ frame: rect(0, 0, 1440, 900), scale: 2 }]);
  const nativeLibrary = { IncodexPermissionFlightView: {} };
  let received: any;
  const flight = runNativePermissionHandoff({
    objc: bridge.objc,
    nativeLibrary,
    source: { frame: rect(10, 400, 80, 28), image: {} },
    target: { frame: rect(300, 20, 532, 112), view: {} },
    reducedMotion: false,
    createReplicants: (options: any) => {
      received = options;
      return { render() {}, dispose() {} };
    },
  });
  expect(received.nativeLibrary).toBe(nativeLibrary);
  flight.dispose();
  await flight.finished;
});

test("SwiftUI flight uses its public image/progress ABI instead of AppKit image simulation", () => {
  const bridge = nativeMotionBridge([{ frame: rect(0, 0, 1440, 900), scale: 2 }]);
  const swift = swiftUIFlightLibrary();
  const sourceImage = { size: () => ({ width: 518, height: 80 }) };
  const replicas = createNativeReplicants({
    objc: bridge.objc,
    nativeLibrary: swift.library,
    source: { image: sourceImage },
    target: { view: bridge.targetView(rect(0, 0, 531, 110)) },
  });
  let view: any;
  try {
    replicas.render({
      bounds: { x: 100, y: 200, width: 526, height: 104 },
      progress: .5,
      cornerRadius: 18,
      sourceOpacity: .5,
      targetOpacity: .5,
      sourceBlur: 6,
      targetBlur: 6,
    });
    expect(swift.instances).toHaveLength(1);
    view = swift.instances[0];
    expect(view.calls.some((call: any) => call.selector === "setSourceImage$targetImage$")).toBe(true);
    expect(view.calls.some((call: any) => call.selector === "updateProgress$cornerRadius$reduceTransparency$" &&
      call.args[0] === .5 && call.args[1] === 18 && call.args[2] === false)).toBe(true);
    expect(view.calls.some((call: any) => call.selector === "setFrame$")).toBe(true);
    expect(bridge.objects.some((value) => ["NSVisualEffectView", "NSImageView", "CIFilter"].includes(value.type))).toBe(false);
  } finally {
    replicas.dispose();
    if (view) {
      const clear = view.calls.at(-1);
      expect(clear?.selector).toBe("setSourceImage$targetImage$");
      expect(clear?.args).toEqual([null, null]);
    }
  }
});

test("Back sends the outgoing helper before the incoming original card to SwiftUI", () => {
  const bridge = nativeMotionBridge([{ frame: rect(0, 0, 1440, 900), scale: 2 }]);
  const swift = swiftUIFlightLibrary();
  const originalImage = { size: () => ({ width: 518, height: 80 }) };
  const replicas = createNativeReplicants({ objc: bridge.objc,
    nativeLibrary: swift.library, source: { image: originalImage },
    target: { view: bridge.targetView(rect(0, 0, 531, 126)) }, reverse: true });
  try {
    replicas.render({ bounds: { x: 100, y: 200, width: 526, height: 104 },
      progress: .25, cornerRadius: 15, sourceOpacity: .75, targetOpacity: .25, sourceBlur: 3, targetBlur: 9 });
    const view = swift.instances[0];
    const setImages = view.calls.find((call: any) => call.selector === "setSourceImage$targetImage$");
    expect(setImages.args[0]).not.toBe(originalImage);
    expect(setImages.args[1]).toBe(originalImage);
    const update = view.calls.find((call: any) => call.selector === "updateProgress$cornerRadius$reduceTransparency$");
    expect(update.args).toEqual([.25, 15, false]);
    expect(view.frameValue).toEqual(rect(30, 30, 526, 104));
    expect(bridge.objects.some(value => ["NSVisualEffectView", "NSImageView", "CIFilter"].includes(value.type))).toBe(false);
  } finally {
    replicas.dispose();
    expect(swift.instances[0].calls.at(-1)?.args).toEqual([null, null]);
  }
});

test("closing during native flight reaps panels and settles without late frames", async () => {
  const h = harness();
  h.flight.dispose();
  h.flight.dispose();
  await h.flight.finished;
  const count = h.frames.length;
  h.advance(3000);
  expect(h.frames).toHaveLength(count);
  expect(h.pending.size).toBe(0);
  expect(h.disposed()).toBe(1);
});

test("Reduce Motion avoids creating any snapshot panels", async () => {
  const h = harness(true);
  await h.flight.finished;
  expect(h.created()).toBe(0);
  expect(h.pending.size).toBe(0);
});

test("host closure observed inside a frame leaves no timer behind", async () => {
  const h = harness();
  h.closeHost();
  h.advance(16);
  await h.flight.finished;
  expect(h.pending.size).toBe(0);
  expect(h.disposed()).toBe(1);
});

test("native replicants rebuild for a changed screen topology and backing scale on the next frame", () => {
  const bridge = nativeMotionBridge([
    { frame: rect(0.25, 0, 1440, 900), scale: 2 },
  ]);
  const swift = swiftUIFlightLibrary();
  const source = { image: { size: () => ({ width: 80, height: 28 }) }, frame: rect(10, 400, 80, 28) };
  const targetView = bridge.targetView(rect(0, 0, 452, 44));
  const replicas = createNativeReplicants({
    objc: bridge.objc,
    nativeLibrary: swift.library,
    source,
    target: { view: targetView },
  });
  const sample = {
    bounds: { x: 100.25, y: 100.25, width: 80.25, height: 28.25 },
    cornerRadius: 12, progress: 0.2, sourceOpacity: 0.8, targetOpacity: 0.2,
    sourceBlur: 2.4, targetBlur: 9.6,
  };

  replicas.render(sample);
  const firstPanel = bridge.panels()[0];
  expect(firstPanel).toBeDefined();
  expect(firstPanel.frameValue.origin.x).toBe(0.5);
  expect(firstPanel.contentViewValue.subviews[0].type).toBe("IncodexPermissionFlightView");
  expect(firstPanel.contentViewValue.subviews[0].layer().values.get("setContentsScale$")).toBe(2);

  bridge.screens([{ frame: rect(0.25, 0, 1440, 900), scale: 1 }]);
  replicas.render(sample);
  const panels = bridge.panels();
  expect(panels).toHaveLength(2);
  expect(firstPanel.closed).toBe(true);
  expect(panels[1].contentViewValue.subviews[0].layer().values.get("setContentsScale$")).toBe(1);
  replicas.dispose();
});

test("flight keeps the SwiftUI surface centered in the changing card", () => {
  const bridge = nativeMotionBridge([{ frame: rect(0, 0, 1440, 900), scale: 2 }]);
  const swift = swiftUIFlightLibrary();
  const replicas = createNativeReplicants({
    objc: bridge.objc,
    nativeLibrary: swift.library,
    source: { image: { size: () => ({ width: 518, height: 80 }) } },
    target: { view: bridge.targetView(rect(0, 0, 531, 126)) },
  });
  try {
    for (const bounds of [
      { x: 100, y: 200, width: 518, height: 80 },
      { x: 160, y: 250, width: 526, height: 104 },
      { x: 200, y: 300, width: 531, height: 126 },
    ]) {
      replicas.render({ bounds, progress: .5, cornerRadius: 18,
        sourceOpacity: .5, targetOpacity: .5, sourceBlur: 6, targetBlur: 6 });
      const view = swift.instances[0];
      expect(view.frameValue).toEqual(rect(30, 30, bounds.width, bounds.height));
      const update = view.calls.filter((call: any) => call.selector === "updateProgress$cornerRadius$reduceTransparency$").at(-1);
      expect(update.args).toEqual([.5, 18, false]);
    }
  } finally {
    replicas.dispose();
    expect(swift.instances[0].calls.at(-1)?.args).toEqual([null, null]);
  }
});

test("flight delegates material, clipping, and blur to SwiftUI without clipping shadows", () => {
  const bridge = nativeMotionBridge([{ frame: rect(0, 0, 1440, 900), scale: 2 }]);
  const swift = swiftUIFlightLibrary();
  const replicas = createNativeReplicants({
    objc: bridge.objc,
    nativeLibrary: swift.library,
    source: { image: { size: () => ({ width: 518, height: 80 }) } },
    target: { view: bridge.targetView(rect(0, 0, 531, 126)) },
  });
  try {
    for (const cornerRadius of [24, 18, 12]) {
      replicas.render({ bounds: { x: 100, y: 200, width: 526, height: 104 },
        progress: .5, cornerRadius, sourceOpacity: .5, targetOpacity: .5, sourceBlur: 6, targetBlur: 6 });
      const root = bridge.panels()[0].contentViewValue;
      const surface = root.subviews.find((view: any) => view.type === "IncodexPermissionFlightView");
      expect(surface).toBe(swift.instances[0]);
      const update = surface.calls.filter((call: any) => call.selector === "updateProgress$cornerRadius$reduceTransparency$").at(-1);
      expect(update.args).toEqual([.5, cornerRadius, false]);
      expect(root.layerValue.values.get("setMasksToBounds$")).toBe(false);
      expect(bridge.objects.some((value) => ["NSVisualEffectView", "NSImageView", "CIFilter"].includes(value.type))).toBe(false);
      for (const shadow of root.layerValue.sublayers) {
        expect(shadow.values.get("setMasksToBounds$")).toBe(false);
        expect(shadow.values.get("setMask$").values.get("setFillRule$")).toBe("even-odd");
      }
    }
  } finally {
    replicas.dispose();
    expect(swift.instances[0].calls.at(-1)?.args).toEqual([null, null]);
  }
});

test("flight keeps one SwiftUI material-and-image surface above the AppKit root", () => {
  const bridge = nativeMotionBridge([{ frame: rect(0, 0, 1440, 900), scale: 2 }]);
  const swift = swiftUIFlightLibrary();
  const replicas = createNativeReplicants({ objc: bridge.objc,
    nativeLibrary: swift.library,
    source: { image: { size: () => ({ width: 518, height: 80 }) } },
    target: { view: bridge.targetView(rect(0, 0, 531, 110)) } });
  try {
    for (const [progress, cornerRadius] of [[0, 24], [.5, 18], [1, 12]]) {
      replicas.render({ bounds: { x: 100, y: 200, width: 526, height: 104 },
        progress, cornerRadius, sourceOpacity: 1 - progress, targetOpacity: progress,
        sourceBlur: 12 * progress, targetBlur: 12 * (1 - progress) });
      const root = bridge.panels()[0].contentViewValue;
      const surface = root.subviews[0];
      expect(surface).toBe(swift.instances[0]);
      expect(root.subviews).toHaveLength(2);
      const update = surface.calls.filter((call: any) => call.selector === "updateProgress$cornerRadius$reduceTransparency$").at(-1);
      expect(update.args).toEqual([progress, cornerRadius, false]);
      expect(bridge.objects.some((value) => ["NSVisualEffectView", "NSImageView", "CIFilter"].includes(value.type))).toBe(false);
    }
  } finally {
    replicas.dispose();
    expect(swift.instances[0].calls.at(-1)?.args).toEqual([null, null]);
  }
});

test("flight shadow and cutout retain the full radius while only the stroke is inset", () => {
  const bridge = nativeMotionBridge([{ frame: rect(0, 0, 1440, 900), scale: 2 }]);
  const swift = swiftUIFlightLibrary();
  const replicas = createNativeReplicants({ objc: bridge.objc, nativeLibrary: swift.library,
    source: { image: {} }, target: { view: bridge.targetView(rect(0, 0, 531, 110)) } });
  try {
    // Shadow and even-odd cutout use the clipping shape. The half-line-width
    // inset belongs only to the stroke, including at small radii.
    for (const radius of [24, 21, 18, 15, 12, .1, 0]) {
      const before = bridge.calls.length;
      replicas.render({ bounds: { x: 100, y: 200, width: 526, height: 104 }, progress: .5, cornerRadius: radius });
      const root = bridge.panels()[0].contentViewValue;
      for (const shadow of root.layerValue.sublayers) {
        expect(shadow.values.get("shadowPath").args).toEqual([rect(30, 30, 526, 104), radius, radius, null]);
      }
      const paths = bridge.calls.slice(before).filter(call => call.selector === "CGPathCreateWithRoundedRect");
      expect(paths).toHaveLength(7);
      expect(paths[0].args.slice(1)).toEqual([rect(.25, .25, 525.5, 103.5), Math.max(0, radius - .25), Math.max(0, radius - .25), null]);
      for (const path of paths.slice(1)) {
        expect(path.args.slice(1)).toEqual([rect(30, 30, 526, 104), radius, radius, null]);
      }
    }
  } finally { replicas.dispose(); }
});

test("native snapshot rejects an empty view before creating flight panels", () => {
  const bridge = nativeMotionBridge([
    { frame: rect(0, 0, 1440, 900), scale: 2 },
  ]);
  const swift = swiftUIFlightLibrary();
  expect(() => createNativeReplicants({
    objc: bridge.objc,
    nativeLibrary: swift.library,
    source: { image: {}, frame: rect(10, 400, 80, 28) },
    target: { view: bridge.targetView(rect(0, 0, 0, 0)) },
  })).toThrow(/empty/i);
  expect(bridge.panels()).toHaveLength(0);
});

test("SwiftUI construction failure clears images before closing its owned panel", () => {
  const bridge = nativeMotionBridge([
    { frame: rect(0, 0, 1440, 900), scale: 2 },
  ], { throwOnSwiftSurfaceAdd: true });
  const swift = swiftUIFlightLibrary();
  expect(() => createNativeReplicants({
    objc: bridge.objc,
    nativeLibrary: swift.library,
    source: { image: { size: () => ({ width: 518, height: 80 }) } },
    target: { view: bridge.targetView(rect(0, 0, 531, 110)) },
  })).toThrow(/construction failure/i);
  expect(swift.instances).toHaveLength(1);
  expect(swift.instances[0].calls.at(-1)?.selector).toBe("setSourceImage$targetImage$");
  expect(swift.instances[0].calls.at(-1)?.args).toEqual([null, null]);
  expect(bridge.panels()[0].closed).toBe(true);
  expect(bridge.panels()[0].visible).toBe(false);
});

test("panel close errors do not stop remaining SwiftUI surfaces from clearing and closing", () => {
  const bridge = nativeMotionBridge([
    { frame: rect(0, 0, 1440, 900), scale: 2 },
    { frame: rect(1440, 0, 1440, 900), scale: 2 },
  ]);
  const swift = swiftUIFlightLibrary();
  const replicas = createNativeReplicants({
    objc: bridge.objc,
    nativeLibrary: swift.library,
    source: { image: { size: () => ({ width: 518, height: 80 }) } },
    target: { view: bridge.targetView(rect(0, 0, 531, 110)) },
  });
  replicas.render({ bounds: { x: 100, y: 200, width: 526, height: 104 },
    progress: .5, cornerRadius: 18, sourceOpacity: .5, targetOpacity: .5, sourceBlur: 6, targetBlur: 6 });
  const panels = bridge.panels();
  expect(panels).toHaveLength(2);
  const originalClose = panels[0].close;
  panels[0].close = () => { originalClose.call(panels[0]); throw new Error("injected close failure"); };
  expect(() => replicas.dispose()).toThrow(/close failure/i);
  expect(swift.instances).toHaveLength(2);
  expect(swift.instances.every(view => view.calls.at(-1)?.args?.[0] === null && view.calls.at(-1)?.args?.[1] === null)).toBe(true);
  expect(panels.every(panel => panel.closed)).toBe(true);
  expect(panels.every(panel => !panel.visible)).toBe(true);
});


test("flight shadows follow the reference 30pt container, masks and dynamic rounded path", () => {
  const bridge=nativeMotionBridge([{frame:rect(0,0,1440,900),scale:2}]);
  const swift=swiftUIFlightLibrary();
  const replicas=createNativeReplicants({objc:bridge.objc,nativeLibrary:swift.library,source:{image:{size:()=>({width:518,height:80})}},target:{view:bridge.targetView(rect(0,0,531,110))}});
  try {
    replicas.render({bounds:{x:100,y:200,width:518,height:80},cornerRadius:24,progress:.25,
      sourceOpacity:.75,targetOpacity:.25,sourceBlur:3,targetBlur:9});
    const panel=bridge.panels()[0],root=panel.contentViewValue;
    expect(panel.values.get("setLevel$")).toBe(25);
    expect(root.frameValue).toEqual(rect(70,170,578,140));
    expect(root.subviews[0].frameValue).toEqual(rect(30,30,518,80));
    const shadows=root.layerValue.sublayers;
    expect(shadows).toHaveLength(3);
    expect(shadows.map((v:any)=>[v.values.get("setShadowOpacity$"),v.values.get("setShadowRadius$"),v.values.get("setShadowOffset$")]))
      .toEqual([[.06,2,{width:0,height:-3}],[.09,15,{width:0,height:-5}],[.20,3,{width:0,height:0}]]);
    expect(shadows[0].values.get("setOpacity$")).toBe(.25);
    for(const shadow of shadows) {
      expect(shadow.frameValue).toEqual(rect(0,0,578,140));
      expect(shadow.values.get("shadowPath").args).toEqual([rect(30,30,518,80),24,24,null]);
      const mask=shadow.values.get("setMask$");
      expect(mask.values.get("setFillRule$")).toBe("even-odd");
      expect(mask.frameValue).toEqual(rect(0,0,578,140));
      expect(mask.values.get("path")).toBeDefined();
      expect(shadow.values.has("setZPosition$")).toBe(false);
    }
    const stroke=bridge.objects.find(v=>v.type==="CAShapeLayer" && v.values.get("setLineWidth$")===.5);
    expect(stroke.values.get("setOpacity$")).toBeCloseTo(.0375);
    expect(stroke.values.get("path").args).toEqual([rect(.25,.25,517.5,79.5),23.75,23.75,null]);
  } finally {replicas.dispose();}
});


test("flight uses reference integral point bounds before adding the 30pt margin", () => {
  const bridge=nativeMotionBridge([{frame:rect(0,0,1440,900),scale:2}]);
  const swift=swiftUIFlightLibrary();
  const replicas=createNativeReplicants({objc:bridge.objc,nativeLibrary:swift.library,source:{image:{size:()=>({width:518,height:80})}},target:{view:bridge.targetView(rect(0,0,531,110))}});
  try {
    replicas.render({bounds:{x:100.25,y:200.25,width:518.25,height:80.25},cornerRadius:24,progress:.25,
      sourceOpacity:.75,targetOpacity:.25,sourceBlur:3,targetBlur:9});
    expect(bridge.panels()[0].contentViewValue.frameValue).toEqual(rect(70,170,579,141));
    expect(bridge.panels()[0].contentViewValue.subviews[0].frameValue).toEqual(rect(30,30,519,81));
  }finally{replicas.dispose();}
});
