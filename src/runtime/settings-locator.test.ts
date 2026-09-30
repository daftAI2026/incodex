import { describe, expect, test } from "bun:test";
import { createNativeSystemSettingsLocator } from "./incodex-dock-menu.cts";

const APP_PATH = "/Applications/ChatGPT.app";
const SETTINGS_BUNDLE_ID = "com.apple.systempreferences";

class FakeNumber {
  constructor(readonly value: number) {}

  doubleValue(): number {
    return this.value;
  }

  intValue(): number {
    return this.value;
  }

  valueOf(): number {
    return this.value;
  }

  toString(): string {
    return String(this.value);
  }
}

class FakeDictionary {
  constructor(private readonly values: Record<string, unknown>) {}

  objectForKey$(key: unknown): unknown {
    const name = String(key);
    return this.values[name];
  }

  objectForKey(key: unknown): unknown {
    return this.objectForKey$(key);
  }

  valueForKey$(key: unknown): unknown {
    return this.objectForKey$(key);
  }
}

class FakeArray {
  constructor(private readonly values: unknown[]) {}

  count(): number {
    return this.values.length;
  }

  objectAtIndex$(index: number): unknown {
    return this.values[index];
  }

  get length(): number {
    return this.values.length;
  }

  at(index: number): unknown {
    return this.values[index];
  }
}

function number(value: number): FakeNumber {
  return new FakeNumber(value);
}

function bounds(x: number, y: number, width: number, height: number): FakeDictionary {
  return new FakeDictionary({
    Height: number(height),
    Width: number(width),
    X: number(x),
    Y: number(y),
    height: number(height),
    width: number(width),
    x: number(x),
    y: number(y),
  });
}

function windowInfo(
  ownerPid: number,
  layer: number,
  windowBounds: FakeDictionary,
  onscreen?: boolean,
): FakeDictionary {
  return new FakeDictionary({
    kCGWindowBounds: windowBounds,
    kCGWindowLayer: number(layer),
    kCGWindowOwnerPID: number(ownerPid),
    kCGWindowIsOnscreen: onscreen,
  });
}

function fakeObjcModule(windows: FakeArray, calls: Array<{ name: string; args: unknown[] }>) {
  const settingsApp = {
    processIdentifier: () => number(4242),
    activateWithOptions$: (options: number) => { calls.push({ name: "activate", args: [options] }); return true; },
  };
  const NSString = {
    stringWithUTF8String$: (value: string) => value,
  };
  const NSRunningApplication = {
    runningApplicationsWithBundleIdentifier$: (bundleId: string) => {
      expect(bundleId).toBe(SETTINGS_BUNDLE_ID);
      return new FakeArray([settingsApp]);
    },
  };

  class FakeLibrary {
    readonly NSString = NSString;
    readonly NSRunningApplication = NSRunningApplication;

    constructor(readonly path: string) {}
  }

  return {
    NobjcLibrary: FakeLibrary,
    async loadObjcModule(): Promise<unknown> {
      return undefined;
    },
    callFunction(name: string, ...args: unknown[]): unknown {
      calls.push({ name, args });
      if (name === "CGWindowListCopyWindowInfo") return windows;
      if (name === "CFRelease") return undefined;
      throw new Error(`unexpected native function ${name}`);
    },
  };
}

describe("native System Settings window locator", () => {
  test.each([
    ["empty", [], false],
    ["visible", [windowInfo(4242, 0, bounds(0, 0, 740, 625), true)], false],
    ["hidden", [windowInfo(4242, 0, bounds(0, 0, 740, 625))], true],
    ["small", [windowInfo(4242, 0, bounds(0, 0, 600, 625), true)], true],
    ["other owner", [windowInfo(9999, 0, bounds(0, 0, 740, 625), true)], false],
  ] as const)("prepares a background Settings handoff only when needed: %s", async (_, windows, activate) => {
    const calls: Array<{ name: string; args: unknown[] }> = [];
    const objc = fakeObjcModule(new FakeArray([...windows]), calls);
    const locate = await createNativeSystemSettingsLocator({ loadObjcModule: async () => objc });
    await locate.prepareHandoff();
    expect(calls.filter(call => call.name === "activate").map(call => call.args)).toEqual(activate ? [[0]] : []);
    expect(calls.find(call => call.name === "CGWindowListCopyWindowInfo")?.args.at(-2)).toBe(16);
    expect(calls.filter(call => call.name === "CFRelease")).toHaveLength(1);
    calls.length = 0;
    locate();
    expect(calls.some(call => call.name === "activate")).toBe(false);
  });

  test("finds the first sufficiently large visible window owned by System Settings", async () => {
    const windows = new FakeArray([
      windowInfo(4242, 0, bounds(21.5, 32.25, 300.5, 200.75)),
      windowInfo(4242, 1, bounds(0, 0, 1920, 30)),
      windowInfo(9999, 0, bounds(0, 0, 5000, 5000)),
      windowInfo(4242, 0, bounds(100.25, 80.5, 900.75, 700.125)),
      windowInfo(4242, 0, bounds(0, 0, 1200, 900)),
      windowInfo(4242, 0, bounds(0, 0, 0, 900)),
    ]);
    const calls: Array<{ name: string; args: unknown[] }> = [];
    const objc = fakeObjcModule(windows, calls);

    const locate = await createNativeSystemSettingsLocator({
      appPath: APP_PATH,
      loadObjcModule: async () => objc,
    });

    expect(locate()).toEqual({
      height: 700.125,
      width: 900.75,
      x: 100.25,
      y: 80.5,
    });

    const listCall = calls.find(({ name }) => name === "CGWindowListCopyWindowInfo");
    expect(listCall?.args.at(-2)).toBe(17);
    expect(listCall?.args.at(-1)).toBe(0);
    expect(calls.filter(({ name }) => name === "CFRelease")).toHaveLength(1);
    expect(calls.find(({ name }) => name === "CFRelease")?.args.at(-1)).toBe(windows);
  });

  test("returns null and still releases the window list when no eligible window exists", async () => {
    const windows = new FakeArray([
      windowInfo(4242, 1, bounds(0, 0, 1920, 30)),
      windowInfo(9999, 0, bounds(0, 0, 1200, 900)),
    ]);
    const calls: Array<{ name: string; args: unknown[] }> = [];
    const objc = fakeObjcModule(windows, calls);

    const locate = await createNativeSystemSettingsLocator({
      appPath: APP_PATH,
      loadObjcModule: async () => objc,
    });

    expect(locate()).toBeNull();
    expect(calls.filter(({ name }) => name === "CFRelease")).toHaveLength(1);
  });

  test.each([
    [600, 625, false],
    [601, 469, false],
    [601, 470, true],
    [600.5, 470, true],
  ])("requires width > 600 and height >= 470: %s x %s", async (width, height, eligible) => {
    const calls: Array<{ name: string; args: unknown[] }> = [];
    const objc = fakeObjcModule(new FakeArray([
      windowInfo(4242, 0, bounds(10, 20, width, height)),
    ]), calls);
    const locate = await createNativeSystemSettingsLocator({ loadObjcModule: async () => objc });
    expect(locate()).toEqual(eligible ? { x: 10, y: 20, width, height } : null);
    expect(calls.filter(({ name }) => name === "CFRelease")).toHaveLength(1);
  });

  test("does not replace visible-window ordering with a layer filter", async () => {
    const calls: Array<{ name: string; args: unknown[] }> = [];
    const objc = fakeObjcModule(new FakeArray([
      windowInfo(4242, 1, bounds(10, 20, 740, 625)),
      windowInfo(4242, 0, bounds(30, 40, 900, 700)),
    ]), calls);
    const locate = await createNativeSystemSettingsLocator({ loadObjcModule: async () => objc });
    expect(locate()).toEqual({ x: 10, y: 20, width: 740, height: 625 });
  });

  test("does not load a framework until the locator factory is created", async () => {
    let loadCalls = 0;
    const calls: Array<{ name: string; args: unknown[] }> = [];
    const objc = fakeObjcModule(new FakeArray([]), calls);

    const locate = await createNativeSystemSettingsLocator({
      appPath: APP_PATH,
      loadObjcModule: async (path: string) => {
        loadCalls += 1;
        expect(path).toBe(APP_PATH);
        return objc;
      },
    });

    expect(loadCalls).toBe(1);
    expect(locate()).toBeNull();
  });
});
