// @ts-nocheck
"use strict";

const path = require("node:path");
const { pathToFileURL } = require("node:url");

const OPEN_ITEM_ID = "incodex-open-incognito";
const IDENTITY_ITEM_ID = "incodex-incognito-identity";
const DOCK_SEPARATOR_ITEM_ID = "incodex-menu-separator";
const STATUS_SEPARATOR_ITEM_ID = "incodex-status-menu-separator";
const MAX_LABEL_LENGTH = 80;
const STATUS_ITEM_CLASS_HINT = "StatusItem";
const APP_KIT_PATH = "/System/Library/Frameworks/AppKit.framework/AppKit";
const FOUNDATION_PATH = "/System/Library/Frameworks/Foundation.framework/Foundation";
const CORE_FOUNDATION_PATH = "/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation";
const CORE_GRAPHICS_PATH = "/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics";
const SETTINGS_BUNDLE_ID = "com.apple.systempreferences";
const CG_WINDOW_LIST_ON_SCREEN_ONLY = 1;
const CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS = 1 << 4;
const CG_WINDOW_LIST_OPTIONS =
  CG_WINDOW_LIST_ON_SCREEN_ONLY | CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS;

function normalizeMenuLabel(value) {
  if (typeof value !== "string") return null;
  if (/[\u0000-\u001f\u007f-\u009f]/u.test(value)) return null;
  const label = value.trim();
  if (!label || [...label].length > MAX_LABEL_LENGTH) return null;
  return label;
}

function normalizeDockMenuLabel(value) {
  return normalizeMenuLabel(value);
}

function normalizeStatusMenuLabel(value) {
  return normalizeMenuLabel(value);
}

function createDockMenuController(options) {
  const { dock, Menu, MenuItem, isIncognito, onOpen, log } = options;
  const originalSetMenu = typeof dock?.setMenu === "function" ? dock.setMenu.bind(dock) : null;
  let label = null;
  let installed = false;

  function report(error) {
    try {
      log("dock-menu-decoration-failed", { error: String(error) });
    } catch {
      /* Logging cannot interfere with the official Dock menu. */
    }
  }

  function ownItemId() {
    return isIncognito ? IDENTITY_ITEM_ID : OPEN_ITEM_ID;
  }

  function findItem(menu, id) {
    if (typeof menu?.getMenuItemById === "function") return menu.getMenuItemById(id);
    return menu?.items?.find?.((item) => item?.id === id) ?? null;
  }

  function decorate(menu) {
    if (!menu || !label) return menu;
    const existing = findItem(menu, ownItemId());
    if (existing) {
      existing.label = label;
      return menu;
    }

    const hasOfficialItems = Array.isArray(menu.items) && menu.items.length > 0;
    if (hasOfficialItems && !findItem(menu, DOCK_SEPARATOR_ITEM_ID)) {
      menu.insert(0, new MenuItem({ id: DOCK_SEPARATOR_ITEM_ID, type: "separator" }));
    }
    menu.insert(
      0,
      new MenuItem({
        id: ownItemId(),
        label,
        enabled: !isIncognito,
        click: isIncognito ? undefined : onOpen,
      }),
    );
    return menu;
  }

  function setDecoratedMenu(menu) {
    if (!originalSetMenu) return false;
    let decorated = menu;
    let decorationSucceeded = true;
    try {
      decorated = decorate(menu);
    } catch (error) {
      decorationSucceeded = false;
      report(error);
    }
    try {
      originalSetMenu(decorated);
      return decorationSucceeded;
    } catch (error) {
      report(error);
      return false;
    }
  }

  if (originalSetMenu) {
    try {
      dock.setMenu = function setIncodexDockMenu(menu) {
        setDecoratedMenu(menu);
      };
      installed = true;
    } catch (error) {
      report(error);
    }
  }

  function configure(value) {
    const nextLabel = normalizeDockMenuLabel(value);
    if (!installed || !nextLabel || typeof Menu !== "function") return false;
    label = nextLabel;
    let menu = null;
    try {
      menu = typeof dock.getMenu === "function" ? dock.getMenu() : null;
      if (!menu) menu = new Menu();
      return setDecoratedMenu(menu);
    } catch (error) {
      report(error);
      return false;
    }
  }

  return { configure };
}

function createStatusMenuController(options) {
  const { loadBridge, isIncognito, onOpen, log } = options;
  let bridge = null;
  let label = null;
  let loading = null;
  let stopObserving = null;
  let disposed = false;

  function report(event, error) {
    try {
      log(event, { error: String(error) });
    } catch {
      /* 日志不能干扰官方菜单。 */
    }
  }

  function openIncognito() {
    if (isIncognito) return;
    try {
      const result = onOpen();
      if (result && typeof result.catch === "function") {
        result.catch((error) => report("status-menu-open-failed", error));
      }
    } catch (error) {
      report("status-menu-open-failed", error);
    }
  }

  function ownItemId() {
    return isIncognito ? IDENTITY_ITEM_ID : OPEN_ITEM_ID;
  }

  function decorate(menu) {
    if (!bridge || !label || !bridge.isCodexStatusMenu(menu)) return;
    try {
      const item = {
        id: ownItemId(),
        label,
        enabled: !isIncognito,
        onSelect: isIncognito ? undefined : openIncognito,
        type: "normal",
      };
      const existing = bridge.findItem(menu, item.id);
      if (existing) {
        bridge.updateItem(existing, item);
        return;
      }
      if (
        bridge.itemCount(menu) > 0 &&
        !bridge.findItem(menu, STATUS_SEPARATOR_ITEM_ID)
      ) {
        bridge.insertItem(menu, 0, {
          id: STATUS_SEPARATOR_ITEM_ID,
          type: "separator",
        });
      }
      bridge.insertItem(menu, 0, item);
    } catch (error) {
      report("status-menu-decoration-failed", error);
    }
  }

  async function install() {
    if (bridge) return true;
    if (disposed) return false;
    if (!loading) {
      loading = Promise.resolve()
        .then(() => loadBridge())
        .then((loaded) => {
          if (disposed || !loaded) return false;
          bridge = loaded;
          const releaseObservers = [];
          try {
            releaseObservers.push(bridge.observeMenuOpen(decorate));
            releaseObservers.push(bridge.observeMenuMutation(decorate));
          } catch (error) {
            try {
              for (const release of releaseObservers) release();
            } finally {
              bridge = null;
              stopObserving = null;
            }
            throw error;
          }
          stopObserving = () => {
            for (const release of releaseObservers) release();
          };
          return true;
        })
        .catch((error) => {
          report("status-menu-unavailable", error);
          return false;
        })
        .finally(() => {
          loading = null;
        });
    }
    return loading;
  }

  async function configure(value) {
    const nextLabel = normalizeStatusMenuLabel(value);
    if (!nextLabel || disposed) return false;
    label = nextLabel;
    return install();
  }

  function dispose() {
    disposed = true;
    try {
      stopObserving?.();
    } catch (error) {
      report("status-menu-dispose-failed", error);
    }
    stopObserving = null;
    bridge = null;
  }

  return { configure, dispose };
}

function nativeString(value) {
  try {
    return value?.toString?.() ?? "";
  } catch {
    return "";
  }
}

function nativeClassName(value) {
  try {
    return nativeString(value?.className?.());
  } catch {
    return "";
  }
}

function menuItems(menu) {
  const count = Number(menu?.numberOfItems?.() ?? 0);
  const items = [];
  for (let index = 0; index < count; index += 1) {
    items.push(menu.itemAtIndex$(index));
  }
  return items;
}

function isCodexStatusMenu(menu) {
  const candidates = [menu?.delegate?.()];
  for (const item of menuItems(menu)) {
    candidates.push(item?.target?.(), item?.view?.());
  }
  return candidates.some((candidate) =>
    nativeClassName(candidate).includes(STATUS_ITEM_CLASS_HINT),
  );
}

async function loadObjcModule(appPath) {
  const modulePath = path.join(appPath, "node_modules", "objc-js", "dist", "index.js");
  return import(pathToFileURL(modulePath).href);
}

function nativeNumber(value, method) {
  if (value === null || value === undefined) return null;
  try {
    const converted = typeof value?.[method] === "function" ? value[method]() : value;
    const number = Number(converted);
    return Number.isFinite(number) ? number : null;
  } catch {
    return null;
  }
}

function nativeInteger(value) {
  return nativeNumber(value, "intValue") ?? nativeNumber(value, "longLongValue") ?? nativeNumber(value, "doubleValue");
}

function nativeDouble(value) {
  return nativeNumber(value, "doubleValue");
}

function nativeDictionaryValue(dictionary, key) {
  for (const methodName of ["objectForKey$", "objectForKey", "valueForKey$"]) {
    try {
      const method = dictionary?.[methodName];
      if (typeof method !== "function") continue;
      const value = method.call(dictionary, key);
      if (value !== null && value !== undefined) return value;
    } catch {
      /* A CF dictionary can expose only one of the toll-free methods. */
    }
  }
  return null;
}

function nativeCollectionItems(collection) {
  if (!collection) return [];
  if (Array.isArray(collection)) return collection;

  const count = nativeInteger(
    typeof collection?.count === "function" ? collection.count() : collection?.count,
  );
  if (count !== null && count >= 0 && count <= 10_000) {
    const items = [];
    for (let index = 0; index < count; index += 1) {
      let item = null;
      for (const methodName of ["objectAtIndex$", "objectAtIndex", "at"]) {
        try {
          const method = collection?.[methodName];
          if (typeof method !== "function") continue;
          item = method.call(collection, index);
          break;
        } catch {
          /* Try the next toll-free collection method. */
        }
      }
      if (item === null || item === undefined) {
        try {
          item = collection[index];
        } catch {
          item = null;
        }
      }
      items.push(item);
    }
    return items;
  }

  try {
    return [...collection];
  } catch {
    return [];
  }
}

function nativeStringObject(NSString, value) {
  try {
    return NSString?.stringWithUTF8String$?.(value) ?? value;
  } catch {
    return value;
  }
}

async function createNativeSystemSettingsLocator(options) {
  const loader = options?.loadObjcModule ?? loadObjcModule;
  const objc = await loader(options?.appPath);
  if (typeof objc?.NobjcLibrary !== "function" || typeof objc?.callFunction !== "function") {
    return () => null;
  }

  let appKit;
  let foundation;
  let coreFoundation;
  let coreGraphics;
  try {
    appKit = new objc.NobjcLibrary(APP_KIT_PATH);
    foundation = new objc.NobjcLibrary(FOUNDATION_PATH);
    coreFoundation = new objc.NobjcLibrary(CORE_FOUNDATION_PATH);
    // Loading CoreGraphics makes CGWindowListCopyWindowInfo available to callFunction.
    coreGraphics = new objc.NobjcLibrary(CORE_GRAPHICS_PATH);
  } catch {
    return () => null;
  }

  const NSString = foundation?.NSString;
  const NSRunningApplication = appKit?.NSRunningApplication;
  const keys = {
    bounds: nativeStringObject(NSString, "kCGWindowBounds"),
    layer: nativeStringObject(NSString, "kCGWindowLayer"),
    ownerPid: nativeStringObject(NSString, "kCGWindowOwnerPID"),
    x: nativeStringObject(NSString, "X"),
    y: nativeStringObject(NSString, "Y"),
    width: nativeStringObject(NSString, "Width"),
    height: nativeStringObject(NSString, "Height"),
  };
  const bundleId = nativeStringObject(NSString, SETTINGS_BUNDLE_ID);

  function settingsPids() {
    if (typeof NSRunningApplication?.runningApplicationsWithBundleIdentifier$ !== "function") {
      return [];
    }
    let applications;
    try {
      applications = NSRunningApplication.runningApplicationsWithBundleIdentifier$(bundleId);
    } catch {
      return [];
    }
    const pids = new Set();
    for (const application of nativeCollectionItems(applications)) {
      try {
        const rawPid =
          typeof application?.processIdentifier === "function"
            ? application.processIdentifier()
            : application?.processIdentifier;
        const pid = nativeInteger(rawPid);
        if (pid !== null && pid > 0) pids.add(pid);
      } catch {
        /* An exited application is not a usable target. */
      }
    }
    return pids;
  }

  function locate() {
    if (!coreFoundation || !coreGraphics) return null;
    const pids = settingsPids();
    if (pids.size === 0) return null;

    let windowList = null;
    try {
      windowList = objc.callFunction(
        "CGWindowListCopyWindowInfo",
        { returns: "@", args: ["I", "I"] },
        CG_WINDOW_LIST_OPTIONS,
        0,
      );
      if (!windowList) return null;

      let best = null;
      let bestArea = 0;
      for (const window of nativeCollectionItems(windowList)) {
        const ownerPid = nativeInteger(nativeDictionaryValue(window, keys.ownerPid));
        const layer = nativeInteger(nativeDictionaryValue(window, keys.layer));
        if (ownerPid === null || !pids.has(ownerPid) || layer !== 0) continue;

        const windowBounds = nativeDictionaryValue(window, keys.bounds);
        const x = nativeDouble(nativeDictionaryValue(windowBounds, keys.x));
        const y = nativeDouble(nativeDictionaryValue(windowBounds, keys.y));
        const width = nativeDouble(nativeDictionaryValue(windowBounds, keys.width));
        const height = nativeDouble(nativeDictionaryValue(windowBounds, keys.height));
        if (
          x === null ||
          y === null ||
          width === null ||
          height === null ||
          width <= 0 ||
          height <= 0
        ) {
          continue;
        }

        const area = width * height;
        if (!Number.isFinite(area) || area <= bestArea) continue;
        bestArea = area;
        best = { x, y, width, height };
      }
      return best;
    } catch {
      return null;
    } finally {
      if (windowList) {
        try {
          objc.callFunction("CFRelease", { returns: "v", args: ["@"] }, windowList);
        } catch {
          /* Metadata lookup is best-effort; never turn a guide into a crash. */
        }
      }
    }
  }

  return locate;
}

async function createNativeStatusMenuBridge(options) {
  const { appPath, onError } = options;
  const { NobjcClass, NobjcLibrary, callFunction, typedBlock } =
    await loadObjcModule(appPath);
  const appKit = new NobjcLibrary(APP_KIT_PATH);
  const foundation = new NobjcLibrary(FOUNDATION_PATH);
  const NSString = foundation.NSString;
  const NSMenuItem = appKit.NSMenuItem;
  const notificationCenter = foundation.NSNotificationCenter.defaultCenter();
  const handlers = new Map();
  const className = `IncodexStatusMenuTarget_${process.pid}`;
  const Target = NobjcClass.define({
    name: className,
    superclass: "NSObject",
    methods: {
      "performIncodexStatusAction:": {
        types: "v@:@",
        implementation: (_self, sender) => {
          const identifier = nativeString(sender?.identifier?.());
          try {
            handlers.get(identifier)?.();
          } catch (error) {
            onError(error);
          }
        },
      },
    },
  });
  const target = Target.alloc().init();
  const selectorName = NSString.stringWithUTF8String$("performIncodexStatusAction:");
  const selector = callFunction(
    "NSSelectorFromString",
    { returns: ":", args: ["@"] },
    selectorName,
  );

  function identifier(value) {
    return NSString.stringWithUTF8String$(value);
  }

  function findItem(menu, id) {
    return (
      menuItems(menu).find((item) => nativeString(item?.identifier?.()) === id) ?? null
    );
  }

  function insertItem(menu, index, item) {
    let nativeItem;
    if (item.type === "separator") {
      nativeItem = NSMenuItem.separatorItem();
    } else {
      const title = NSString.stringWithUTF8String$(item.label);
      const empty = NSString.stringWithUTF8String$("");
      nativeItem = NSMenuItem.alloc().initWithTitle$action$keyEquivalent$(
        title,
        selector,
        empty,
      );
      nativeItem.setTarget$(target);
      nativeItem.setEnabled$(item.enabled !== false);
      if (item.onSelect) handlers.set(item.id, item.onSelect);
    }
    nativeItem.setIdentifier$(identifier(item.id));
    menu.insertItem$atIndex$(nativeItem, index);
  }

  function updateItem(item, update) {
    item.setTitle$(NSString.stringWithUTF8String$(update.label));
    item.setEnabled$(update.enabled !== false);
    if (update.onSelect) handlers.set(update.id, update.onSelect);
    else handlers.delete(update.id);
  }

  function observeMenuOpen(handler) {
    const notificationName = NSString.stringWithUTF8String$(
      "NSMenuDidBeginTrackingNotification",
    );
    const callback = typedBlock({ returns: "v", args: ["@"] }, (notification) => {
      handler(notification.object());
    });
    const observer = notificationCenter.addObserverForName$object$queue$usingBlock$(
      notificationName,
      null,
      null,
      callback,
    );
    return () => notificationCenter.removeObserver$(observer);
  }

  function observeMenuMutation(handler) {
    let pendingMenu = null;
    let scheduled = null;
    const callbacks = [];
    const observers = [];

    function schedule(menu) {
      if (!isCodexStatusMenu(menu)) return;
      pendingMenu = menu;
      if (scheduled) return;
      scheduled = setImmediate(() => {
        scheduled = null;
        const currentMenu = pendingMenu;
        pendingMenu = null;
        if (currentMenu) handler(currentMenu);
      });
    }

    for (const name of ["NSMenuDidAddItemNotification", "NSMenuDidRemoveItemNotification"]) {
      const notificationName = NSString.stringWithUTF8String$(name);
      const callback = typedBlock({ returns: "v", args: ["@"] }, (notification) => {
        schedule(notification.object());
      });
      callbacks.push(callback);
      observers.push(
        notificationCenter.addObserverForName$object$queue$usingBlock$(
          notificationName,
          null,
          null,
          callback,
        ),
      );
    }

    return () => {
      if (scheduled) clearImmediate(scheduled);
      scheduled = null;
      pendingMenu = null;
      for (const observer of observers) notificationCenter.removeObserver$(observer);
      callbacks.length = 0;
    };
  }

  return {
    findItem,
    insertItem,
    isCodexStatusMenu,
    itemCount: (menu) => Number(menu?.numberOfItems?.() ?? 0),
    observeMenuOpen,
    observeMenuMutation,
    updateItem,
  };
}

export {
  loadObjcModule,
  createDockMenuController,
  createNativeStatusMenuBridge,
  createNativeSystemSettingsLocator,
  createStatusMenuController,
  isCodexStatusMenu,
  normalizeDockMenuLabel,
  normalizeStatusMenuLabel,
};
