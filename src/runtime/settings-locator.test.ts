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
): FakeDictionary {
  return new FakeDictionary({
    kCGWindowBounds: windowBounds,
    kCGWindowLayer: number(layer),
    kCGWindowOwnerPID: number(ownerPid),
  });
}

function fakeObjcModule(windows: FakeArray, calls: Array<{ name: string; args: unknown[] }>) {
  const settingsApp = {
    processIdentifier: () => number(4242),
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
  test("finds the largest visible layer-0 window owned by System Settings", async () => {
    const windows = new FakeArray([
      windowInfo(4242, 0, bounds(21.5, 32.25, 300.5, 200.75)),
      windowInfo(4242, 1, bounds(0, 0, 4000, 4000)),
      windowInfo(9999, 0, bounds(0, 0, 5000, 5000)),
      windowInfo(4242, 0, bounds(100.25, 80.5, 900.75, 700.125)),
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
      windowInfo(4242, 1, bounds(0, 0, 800, 600)),
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
