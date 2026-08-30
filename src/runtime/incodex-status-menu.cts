// @ts-nocheck
"use strict";

const path = require("node:path");
const { pathToFileURL } = require("node:url");

const OPEN_ITEM_ID = "incodex-open-incognito";
const IDENTITY_ITEM_ID = "incodex-incognito-identity";
const SEPARATOR_ITEM_ID = "incodex-status-menu-separator";
const MAX_LABEL_LENGTH = 80;
const STATUS_ITEM_CLASS_HINT = "StatusItem";
const APP_KIT_PATH = "/System/Library/Frameworks/AppKit.framework/AppKit";
const FOUNDATION_PATH = "/System/Library/Frameworks/Foundation.framework/Foundation";

function normalizeStatusMenuLabel(value) {
  if (typeof value !== "string") return null;
  if (/[\u0000-\u001f\u007f-\u009f]/u.test(value)) return null;
  const label = value.trim();
  if (!label || [...label].length > MAX_LABEL_LENGTH) return null;
  return label;
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
        !bridge.findItem(menu, SEPARATOR_ITEM_ID)
      ) {
        bridge.insertItem(menu, 0, {
          id: SEPARATOR_ITEM_ID,
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
            for (const release of releaseObservers) release();
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
  return candidates.some((candidate) => nativeClassName(candidate).includes(STATUS_ITEM_CLASS_HINT));
}

async function loadObjcModule(appPath) {
  const modulePath = path.join(appPath, "node_modules", "objc-js", "dist", "index.js");
  return import(pathToFileURL(modulePath).href);
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
  createNativeStatusMenuBridge,
  createStatusMenuController,
  isCodexStatusMenu,
  normalizeStatusMenuLabel,
};
