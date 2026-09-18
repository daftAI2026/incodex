// @ts-nocheck
"use strict";

// This is the operation-neutral, one-shot host for the native Accessibility
// guide.  The CLI owns TCC probing, reset, Settings, and the final decision;
// this process owns only AppKit/SwiftUI presentation and the nonce pipe.
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { pathToFileURL } = require("node:url");
// The shipped bundle has the sibling CJS Runtime asset. Source-level Bun
// tests load the .cts sibling directly; this fallback never changes the
// official objc-js path or any production module lookup.
const safeHome = require(
  fs.existsSync(path.join(__dirname, "incodex-safe-home.cjs"))
    ? "./incodex-safe-home.cjs"
    : "./incodex-safe-home.cts",
);

const APP_PATH = "/Applications/ChatGPT.app";
const APP_BUNDLE_ID = "com.openai.codex";
const APP_EXECUTABLE_PATH = path.join(APP_PATH, "Contents", "MacOS", "ChatGPT");
const APP_KIT_PATH = "/System/Library/Frameworks/AppKit.framework/AppKit";
const FOUNDATION_PATH = "/System/Library/Frameworks/Foundation.framework/Foundation";
const CORE_FOUNDATION_PATH = "/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation";
const NONCE_LENGTH = 32;
const HOST_MAX_LINE_BYTES = 64 * 1024;
const PRE_READY_MAX_MESSAGES = 16;
const PRE_READY_MAX_BYTES = HOST_MAX_LINE_BYTES;
const HOST_TIMEOUT_MS = 15 * 60 * 1000;
const PRESENTATION_TIMEOUT_MS = 5 * 60 * 1000;
const GUIDE_RETRY_DELAY_MS = 10;
const ACCESSIBILITY_COPY = "__INCODEX_ACCESSIBILITY_COPY__";
const ACCESSIBILITY_LOCALE = "__INCODEX_ACCESSIBILITY_LOCALE__";
const accessibilityWindow = "__INCODEX_ACCESSIBILITY_WINDOW__";

const INPUT_STATES = new Set(["repairing", "awaiting-user", "granted", "error"]);
const OUTPUT_TYPES = new Set(["ready", "allow", "retry", "later", "error"]);

function asErrorMessage(error) {
  const message = error instanceof Error ? error.message : String(error ?? "unknown error");
  return message.replace(/[\r\n\u0000-\u001f]/g, " ").slice(0, 512);
}

function parseHostArgs(argv = process.argv) {
  const args = Array.from(argv).slice(2);
  if (args.length !== 2 || args[0] !== "--nonce") {
    throw new Error("permission host requires --nonce HEX");
  }
  const nonce = String(args[1] ?? "");
  if (nonce.length !== NONCE_LENGTH || !/^[0-9a-f]{32}$/i.test(nonce)) {
    throw new Error("permission host nonce must be exactly 32 hexadecimal characters");
  }
  return { nonce };
}

/**
 * @param {string} nonce
 * @param {string} type
 * @param {string | null} [message]
 */
function encodeHostMessage(nonce, type, message = null) {
  if (typeof nonce !== "string" || !/^[0-9a-f]{32}$/i.test(nonce)) {
    throw new Error("permission host message has invalid 32-character nonce");
  }
  if (!OUTPUT_TYPES.has(type)) throw new Error("permission host message has invalid type");
  const value = { nonce, type };
  if (message !== undefined && message !== null) value.message = asErrorMessage(message);
  return `${JSON.stringify(value)}\n`;
}

function parseHostInput(line, nonce) {
  if (Buffer.byteLength(String(line), "utf8") > HOST_MAX_LINE_BYTES) {
    throw new Error("permission host input line is too large");
  }
  let value;
  try {
    value = JSON.parse(String(line));
  } catch {
    throw new Error("permission host input is not JSON");
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("permission host input must be an object");
  }
  if (value.nonce !== nonce) throw new Error("permission host nonce mismatch");
  if (value.type === "close") {
    if (Object.keys(value).some(key => !["nonce", "type"].includes(key))) {
      throw new Error("permission host close message has extra fields");
    }
    return { nonce, type: "close" };
  }
  if (value.type !== "state" || !INPUT_STATES.has(value.state)) {
    throw new Error("permission host state is invalid");
  }
  if (value.message !== undefined && typeof value.message !== "string") {
    throw new Error("permission host state message is invalid");
  }
  if (typeof value.message === "string" && value.message.length > 512) {
    throw new Error("permission host state message is too large");
  }
  return {
    nonce,
    type: "state",
    state: value.state,
    ...(value.message ? { message: value.message } : {}),
  };
}

function resolvedLocale(raw) {
  const value = String(raw ?? "").trim().replaceAll("_", "-").split(".")[0];
  return value || "en";
}

function readLocaleOverride() {
  const defaultHome = path.join(os.homedir(), ".codex");
  const configuredHome = safeHome.resolveSourceHome(process.env.CODEX_HOME, defaultHome);
  const sourceHome = safeHome.resolveSourceHome(process.env.INCODEX_SOURCE_HOME, configuredHome);
  try {
    const file = path.join(sourceHome, "config.toml");
    const content = fs.readFileSync(file, "utf8");
    const match = content.match(/^\s*localeOverride\s*=\s*"([^"]+)"/m);
    return (match?.[1] ?? "").trim();
  } catch {
    return "";
  }
}

function hostLocale(options = {}) {
  return resolvedLocale(
    options.locale || readLocaleOverride() || options.systemLocale ||
    Intl.DateTimeFormat().resolvedOptions().locale || "en",
  );
}

function hostCopy(options = {}) {
  if (options.copy) return options.copy;
  if (!ACCESSIBILITY_COPY || typeof ACCESSIBILITY_COPY !== "object") {
    throw new Error("permission host copy catalog is not embedded");
  }
  const locale = ACCESSIBILITY_LOCALE?.resolveLocaleFromCatalog
    ? ACCESSIBILITY_LOCALE.resolveLocaleFromCatalog(hostLocale(options), ACCESSIBILITY_COPY)
    : hostLocale(options);
  return ACCESSIBILITY_COPY[locale] || ACCESSIBILITY_COPY.en;
}

function hostLayoutDirection(options = {}) {
  if (options.layoutDirection) return options.layoutDirection;
  if (ACCESSIBILITY_LOCALE?.resolveLocaleDirection && ACCESSIBILITY_COPY && typeof ACCESSIBILITY_COPY === "object") {
    return ACCESSIBILITY_LOCALE.resolveLocaleDirection(hostLocale(options), ACCESSIBILITY_COPY);
  }
  return "leftToRight";
}

async function loadOfficialObjcModule() {
  const file = path.join(APP_PATH, "Contents", "Resources", "app.asar.unpacked", "node_modules", "objc-js", "dist", "index.js");
  try {
    const stat = fs.lstatSync(file);
    if (!stat.isFile() || stat.isSymbolicLink()) throw new Error("unsafe bridge file");
  } catch {
    throw new Error("official objc-js bridge is unavailable");
  }
  return import(pathToFileURL(file).href);
}

function optionalCall(object, names, ...args) {
  for (const name of names) {
    try {
      if (typeof object?.[name] === "function") return object[name](...args);
    } catch {
      // Try the next selector spelling. objc-js uses the `$` spelling for
      // selectors with arguments, while small test doubles often do not.
    }
  }
  return undefined;
}

function nativeString(value) {
  try { return String(value?.toString?.() ?? value ?? ""); } catch { return ""; }
}

function nativeNumber(value, methods = ["intValue", "longLongValue", "doubleValue"]) {
  for (const method of methods) {
    try {
      const converted = typeof value?.[method] === "function" ? value[method]() : value;
      const number = Number(converted);
      if (Number.isFinite(number)) return number;
    } catch {}
  }
  return null;
}

function nativeCollectionItems(collection) {
  if (!collection) return [];
  if (Array.isArray(collection)) return collection;
  const count = nativeNumber(optionalCall(collection, ["count"]));
  if (!Number.isFinite(count) || count < 0 || count > 10000) return [];
  const items = [];
  for (let index = 0; index < count; index += 1) {
    const item = optionalCall(collection, ["objectAtIndex$", "objectAtIndex"], index);
    if (item !== undefined && item !== null) items.push(item);
  }
  return items;
}

function nativeDictionaryValue(dictionary, key) {
  for (const selector of ["objectForKey$", "objectForKey", "valueForKey$"]) {
    try {
      const value = dictionary?.[selector]?.(key);
      if (value !== undefined && value !== null) return value;
    } catch {}
  }
  return null;
}

function startAppKitEventPump({ application, foundation, runLoop, setIntervalFn = setInterval, clearIntervalFn = clearInterval, onError = () => {} }) {
  const nextEvent = application?.nextEventMatchingMask$untilDate$inMode$dequeue$;
  const sendEvent = application?.sendEvent$;
  if (typeof nextEvent !== "function" || typeof sendEvent !== "function") {
    throw new Error("permission host requires NSApplication nextEvent/sendEvent");
  }
  const NSDate = foundation?.NSDate;
  const NSString = foundation?.NSString;
  if (typeof NSDate?.dateWithTimeIntervalSinceNow$ !== "function" ||
      typeof NSString?.stringWithUTF8String$ !== "function") {
    throw new Error("permission host requires Foundation event helpers");
  }
  const mode = NSString.stringWithUTF8String$("kCFRunLoopDefaultMode");
  const drain = () => {
    for (let index = 0; index < 64; index += 1) {
      const deadline = NSDate.dateWithTimeIntervalSinceNow$(0);
      // NSEventMask is an unsigned long long. A negative Number is rejected
      // by objc-js before Objective-C sees it; BigInt preserves all bits.
      const event = nextEvent.call(application, 0xffffffffffffffffn, deadline, mode, true);
      if (!event) break;
      sendEvent.call(application, event);
    }
  };
  const usesPump = typeof runLoop?.pump === "function";
  let runLoopStop = null;
  if (!usesPump && typeof runLoop?.run === "function") runLoopStop = runLoop.run(10);
  else if (!usesPump) throw new Error("permission host requires objc-js RunLoop pump");
  let stopped = false;
  let timer = null;
  const stop = () => {
    if (stopped) return;
    stopped = true;
    if (timer) clearIntervalFn(timer);
    try { runLoopStop?.(); } catch {}
    try { runLoop?.stop?.(); } catch {}
  };
  const fail = (error) => {
    if (stopped) return;
    stop();
    try { onError(error); } catch {}
  };
  timer = setIntervalFn(() => {
    if (stopped) return;
    try {
      if (usesPump) runLoop.pump(0);
      drain();
    } catch (error) {
      fail(error);
    }
  }, 10);
  timer?.unref?.();
  return {
    pumpOnce() {
      if (stopped) return;
      try { drain(); } catch (error) { fail(error); throw error; }
    },
    stop,
  };
}

function makeAppKitRuntime(objc, options = {}) {
  if (!objc || typeof objc.NobjcLibrary !== "function") {
    throw new Error("permission host ObjC bridge is unavailable");
  }
  const appKit = new objc.NobjcLibrary(APP_KIT_PATH);
  // Loading Foundation before RunLoop.pump is required by objc-js. Do not
  // start the host's event pump until the bridge and NSApplication exist.
  const foundation = new objc.NobjcLibrary(FOUNDATION_PATH);
  // Keep CoreFoundation loaded before any CFRelease call below. objc-js can
  // resolve the symbol without it on some hosts, but that is not a safe
  // assumption for the standalone Node process.
  new objc.NobjcLibrary(CORE_FOUNDATION_PATH);
  new objc.NobjcLibrary("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics");
  const application = appKit.NSApplication.sharedApplication();
  optionalCall(application, ["setActivationPolicy$", "setActivationPolicy"], 1);
  optionalCall(application, ["finishLaunching"]);

  const eventPump = startAppKitEventPump({
    application,
    foundation,
    runLoop: objc.RunLoop,
    onError: options.onError,
  });

  const activate = (options = {}) => {
    optionalCall(application, ["activateIgnoringOtherApps$", "activateIgnoringOtherApps"], options.steal !== false);
  };
  const canPresent = () => {
    try {
      const workspace = appKit.NSWorkspace.sharedWorkspace();
      const front = workspace.frontmostApplication();
      const bundle = String(optionalCall(front, ["bundleIdentifier"]) ?? "");
      if (bundle !== APP_BUNDLE_ID) return false;
      const executableURL = optionalCall(front, ["executableURL"]);
      const executablePath = nativeString(optionalCall(executableURL, ["path"]));
      if (executablePath !== APP_EXECUTABLE_PATH) return false;
      const active = optionalCall(front, ["isActive"]);
      if (active !== undefined && !Boolean(active)) return false;
      const pid = nativeNumber(optionalCall(front, ["processIdentifier"]), ["intValue", "longLongValue"]);
      if (!Number.isFinite(pid) || typeof objc.callFunction !== "function") return false;
      // A same-bundle clone can be frontmost while the official process is
      // still running. Require one and only one process with the fixed
      // executable path, and bind that process to the frontmost PID before
      // consulting its visible window list.
      const matchingProcesses = nativeCollectionItems(
        optionalCall(workspace, ["runningApplications"]),
      ).filter(candidate => {
        const candidateBundle = String(optionalCall(candidate, ["bundleIdentifier"]) ?? "");
        const candidateURL = optionalCall(candidate, ["executableURL"]);
        const candidatePath = nativeString(optionalCall(candidateURL, ["path"]));
        return candidateBundle === APP_BUNDLE_ID && candidatePath === APP_EXECUTABLE_PATH;
      });
      if (matchingProcesses.length !== 1) return false;
      const matchingPid = nativeNumber(
        optionalCall(matchingProcesses[0], ["processIdentifier"]),
        ["intValue", "longLongValue"],
      );
      if (matchingPid !== pid) return false;
      const NSString = foundation.NSString;
      const ownerPidKey = NSString.stringWithUTF8String$("kCGWindowOwnerPID");
      const layerKey = NSString.stringWithUTF8String$("kCGWindowLayer");
      const boundsKey = NSString.stringWithUTF8String$("kCGWindowBounds");
      const xKey = NSString.stringWithUTF8String$("X");
      const yKey = NSString.stringWithUTF8String$("Y");
      const widthKey = NSString.stringWithUTF8String$("Width");
      const heightKey = NSString.stringWithUTF8String$("Height");
      const list = objc.callFunction(
        "CGWindowListCopyWindowInfo",
        { returns: "@", args: ["I", "I"] },
        17,
        0,
      );
      if (!list) return false;
      try {
        return nativeCollectionItems(list).some(window => {
          if (nativeNumber(nativeDictionaryValue(window, ownerPidKey), ["intValue", "longLongValue"]) !== pid) return false;
          if (nativeNumber(nativeDictionaryValue(window, layerKey)) !== 0) return false;
          const bounds = nativeDictionaryValue(window, boundsKey);
          const width = nativeNumber(nativeDictionaryValue(bounds, widthKey), ["doubleValue"]);
          const height = nativeNumber(nativeDictionaryValue(bounds, heightKey), ["doubleValue"]);
          const x = nativeNumber(nativeDictionaryValue(bounds, xKey), ["doubleValue"]);
          const y = nativeNumber(nativeDictionaryValue(bounds, yKey), ["doubleValue"]);
          return [x, y, width, height].every(Number.isFinite) && width > 0 && height > 0;
        });
      } finally {
        try { objc.callFunction("CFRelease", { returns: "v", args: ["@"] }, list); } catch {}
      }
    } catch {
      return false;
    }
  };
  let systemLocale = "en";
  try {
    const languages = foundation.NSLocale.preferredLanguages();
    const first = nativeCollectionItems(languages)[0];
    if (first) systemLocale = nativeString(first);
  } catch {}
  return { objc, foundation, application, activate, canPresent, systemLocale, stopPump: eventPump.stop };
}

function nativeGuideApi(options, runtime) {
  const embedded = options.native || accessibilityWindow;
  const create = options.createGuide || embedded?.createNativeAccessibilitySetupWindow;
  const handoff = embedded?.runNativePermissionHandoff;
  if (typeof create !== "function") throw new Error("native permission guide factory is unavailable");
  return {
    create,
    handoff,
    async makeGuide() {
      let settingsLocatorPromise;
      const locateSettings = options.locateSettings || (async () => {
        settingsLocatorPromise ||= (async () => {
          const dockMenu = require("./incodex-dock-menu.cjs");
          return dockMenu.createNativeSystemSettingsLocator({
            appPath: APP_PATH,
            loadObjcModule: options.loadObjcModule || loadOfficialObjcModule,
          });
        })();
        const locator = await settingsLocatorPromise;
        return typeof locator === "function" ? locator() : null;
      });
      const createOptions = {
        appPath: APP_PATH,
        copy: hostCopy({ ...options, systemLocale: runtime.systemLocale }),
        layoutDirection: hostLayoutDirection({ ...options, systemLocale: runtime.systemLocale }),
        loadObjcModule: options.loadObjcModule || loadOfficialObjcModule,
        locateSettings,
        activate: runtime.activate,
        canPresent: options.canPresent || runtime.canPresent,
        onHandoff: options.onHandoff || (this.handoff ? (payload) => this.handoff({
          ...payload,
          onError: options.onError || (() => {}),
        }) : undefined),
        onBack: options.onBack || (this.handoff ? (payload) => this.handoff({
          ...payload,
          onError: options.onError || (() => {}),
        }) : undefined),
      };
      return create(createOptions);
    },
  };
}

function createPermissionHost(options = {}) {
  const argv = options.argv || process.argv;
  const { nonce } = parseHostArgs(argv);
  const appPath = options.appPath || APP_PATH;
  if (appPath !== APP_PATH) throw new Error("permission host only accepts the official ChatGPT app");
  const stdin = options.stdin || process.stdin;
  const stdout = options.stdout || process.stdout;
  const native = options.native || accessibilityWindow;
  const preReadyMessages = [];
  let preReadyBytes = 0;
  const write = (type, message) => {
    if (finished) return;
    stdout.write(encodeHostMessage(nonce, type, message));
  };

  let guide = null;
  let finished = false;
  let repairChosen = false;
  let ready = false;
  let cleanupRequested = false;
  let stopPump = null;
  let removeInputHandlers = () => {};
  let timeout = null;
  let removeProcessHandlers = () => {};
  let removeGuideRetry = () => {};
  let removeGuideClose = () => {};
  let resolveRun;
  let rejectRun;
  const done = new Promise((resolve, reject) => { resolveRun = resolve; rejectRun = reject; });

  const cleanup = (error = null) => {
    if (cleanupRequested) return;
    cleanupRequested = true;
    finished = true;
    if (timeout) clearTimeout(timeout);
    timeout = null;
    removeProcessHandlers();
    removeInputHandlers();
    removeGuideRetry();
    removeGuideClose();
    removeGuideRetry = () => {};
    removeGuideClose = () => {};
    removeInputHandlers = () => {};
    try { stdin.pause?.(); } catch {}
    try { guide?.close?.(); } catch {}
    guide = null;
    try { stopPump?.(); } catch {}
    stopPump = null;
    if (error) rejectRun(error);
    else resolveRun();
  };

  const protocolError = (error) => {
    if (!finished) {
      try { write("error", asErrorMessage(error)); } catch {}
    }
    finished = true;
    cleanup();
  };

  const handleGuideChoice = (choice) => {
    if (finished || !guide) return;
    if (choice === "repair") {
      if (repairChosen) return;
      repairChosen = true;
      write("allow");
      return;
    }
    if (choice === "later") {
      write("later");
      finished = true;
      cleanup();
      return;
    }
    protocolError(new Error("native permission guide returned an invalid choice"));
  };

  const handleRetry = () => {
    if (finished || !repairChosen) return;
    write("retry");
  };

  const handleLine = (line) => {
    if (finished) return;
    try {
      const message = parseHostInput(line, nonce);
      if (message.type === "close") {
        finished = true;
        cleanup();
        return;
      }
      if (!guide) {
        // close/EOF/granted before a native guide exists must not create one
        // after the parent has already gone away. Other valid states can be
        // retained in a small bounded queue for a parent that writes as soon
        // as it starts the host; they are replayed only after `ready`.
        if (message.state === "granted") {
          finished = true;
          cleanup();
          return;
        }
        const encodedBytes = Buffer.byteLength(JSON.stringify(message), "utf8");
        if (preReadyMessages.length >= PRE_READY_MAX_MESSAGES || preReadyBytes + encodedBytes > PRE_READY_MAX_BYTES) {
          throw new Error("permission host pre-ready state queue is full");
        }
        preReadyMessages.push(message);
        preReadyBytes += encodedBytes;
        return;
      }
      if (!guide || typeof guide.setState !== "function") {
        throw new Error("native permission guide state bridge is unavailable");
      }
      guide.setState(message.state, message.message);
      if (message.state === "granted") {
        finished = true;
        cleanup();
      }
    } catch (error) {
      protocolError(error);
    }
  };

  const installInput = () => {
    let pending = Buffer.alloc(0);
    const onData = (chunk) => {
      if (finished) return;
      const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      let offset = 0;
      while (!finished && offset < bytes.length) {
        const newline = bytes.indexOf(0x0a, offset);
        if (newline < 0) {
          const segment = bytes.subarray(offset);
          if (pending.length + segment.length > HOST_MAX_LINE_BYTES) {
            protocolError(new Error("permission host input line is too large"));
            return;
          }
          pending = pending.length === 0 ? Buffer.from(segment) : Buffer.concat([pending, segment]);
          break;
        }
        const segment = bytes.subarray(offset, newline);
        if (pending.length + segment.length > HOST_MAX_LINE_BYTES) {
          protocolError(new Error("permission host input line is too large"));
          return;
        }
        const complete = pending.length === 0 ? Buffer.from(segment) : Buffer.concat([pending, segment]);
        let line = complete.toString("utf8");
        if (line.endsWith("\r")) line = line.slice(0, -1);
        pending = Buffer.alloc(0);
        handleLine(line);
        offset = newline + 1;
      }
    };
    const onEnd = () => {
      if (!finished && pending.length > 0) handleLine(pending.toString("utf8"));
      if (!finished) {
        finished = true;
        cleanup();
      }
    };
    const onError = (error) => protocolError(error);
    stdin.on("data", onData);
    stdin.once("end", onEnd);
    stdin.once("error", onError);
    stdin.resume?.();
    removeInputHandlers = () => {
      stdin.removeListener?.("data", onData);
      stdin.removeListener?.("end", onEnd);
      stdin.removeListener?.("error", onError);
    };
  };

  const waitForPresentation = async (canPresent, deadline) => {
    while (!finished) {
      let present = false;
      try { present = Boolean(canPresent()); } catch {}
      if (present) return true;
      if (Date.now() >= deadline) throw new Error("official ChatGPT window is not presentable");
      await new Promise(resolve => setTimeout(resolve, Math.min(50, Math.max(1, deadline - Date.now()))));
    }
    return false;
  };

  const makeGuideWithinPresentationDeadline = async (api, canPresent, timeoutMs) => {
    const deadline = Date.now() + timeoutMs;
    while (!finished) {
      const present = await waitForPresentation(canPresent, deadline);
      if (!present || finished) return null;
      const createdGuide = await api.makeGuide();
      if (finished) {
        try { createdGuide?.close?.(); } catch {}
        return null;
      }
      // The native factory deliberately returns null when focus/authentication
      // changed between the gate and AppKit construction. Retry the gate and
      // factory under the original deadline; undefined or another malformed
      // value remains a hard startup error below.
      if (createdGuide !== null) return createdGuide;
      if (Date.now() >= deadline) throw new Error("native permission guide did not become available");
      await new Promise(resolve => setTimeout(resolve, Math.min(GUIDE_RETRY_DELAY_MS, Math.max(1, deadline - Date.now()))));
    }
    return null;
  };

  const installLifecycle = () => {
    const onSignal = () => {
      if (!finished) {
        finished = true;
        cleanup();
      }
    };
    const handlers = [];
    for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
      try { process.once(signal, onSignal); handlers.push([signal, onSignal]); } catch {}
    }
    if (typeof process.on === "function" && typeof process.disconnect === "function") {
      try { process.once("disconnect", onSignal); handlers.push(["disconnect", onSignal]); } catch {}
    }
    removeProcessHandlers = () => {
      for (const [event, handler] of handlers) {
        try { process.removeListener(event, handler); } catch {}
      }
    };
  };

  async function start() {
    if (process.platform !== "darwin") throw new Error("permission host is macOS only");
    installInput();
    installLifecycle();
    timeout = setTimeout(() => protocolError(new Error("permission host timed out")), options.timeoutMs ?? HOST_TIMEOUT_MS);
    timeout.unref?.();
    let runtime = options.runtime;
    if (!runtime) {
      // Unit/integration callers may inject the native guide factory. Keep
      // those tests headless; the shipped host takes the real AppKit path.
      if (options.createGuide || options.native?.createNativeAccessibilitySetupWindow) {
        runtime = {
          activate: options.activate || (() => {}),
          canPresent: options.canPresent || (() => true),
          stopPump: null,
        };
      } else {
        const objc = await (options.loadObjcModule || loadOfficialObjcModule)();
        runtime = makeAppKitRuntime(objc, { onError: protocolError });
      }
    }
    stopPump = runtime.stopPump || null;
    if (finished) {
      try { stopPump?.(); } catch {}
      stopPump = null;
      return done;
    }
    const api = nativeGuideApi(options, runtime);
    const createdGuide = await makeGuideWithinPresentationDeadline(
      api,
      options.canPresent || runtime.canPresent || (() => true),
      options.presentationTimeoutMs ?? PRESENTATION_TIMEOUT_MS,
    );
    if (finished || createdGuide === null) return done;
    guide = createdGuide;
    if (!guide || typeof guide.choice?.then !== "function") {
      throw new Error("native permission guide did not return a choice promise");
    }
    if (typeof guide.onRetry === "function") {
      const unsubscribe = guide.onRetry(handleRetry);
      removeGuideRetry = typeof unsubscribe === "function" ? unsubscribe : () => {};
    }
    if (typeof guide.onClose === "function") {
      const unsubscribe = guide.onClose(() => {
        // Native close normally resolves choice to later. Treat an out-of-band
        // AppKit close as terminal too, so a stale host cannot remain alive.
        if (!finished) {
          write("later");
          finished = true;
          cleanup();
        }
      });
      removeGuideClose = typeof unsubscribe === "function" ? unsubscribe : () => {};
    }
    Promise.resolve(guide.choice).then(handleGuideChoice, protocolError);
    write("ready");
    ready = true;
    const queued = preReadyMessages.splice(0, preReadyMessages.length);
    preReadyBytes = 0;
    for (const message of queued) {
      if (finished) break;
      guide.setState(message.state, message.message);
      if (message.state === "granted") {
        finished = true;
        cleanup();
      }
    }

    return done;
  }

  return {
    run: async () => {
      try {
        const pending = await start();
        await pending;
      } catch (error) {
        if (finished) return;
        if (!finished) {
          try { write("error", asErrorMessage(error)); } catch {}
          finished = true;
          cleanup();
        }
        throw error;
      }
    },
    close: () => {
      finished = true;
      cleanup();
    },
    get ready() { return ready; },
  };
}

async function runPermissionHost(options = {}) {
  const host = createPermissionHost(options);
  await host.run();
}

if (require.main === module) {
  runPermissionHost().catch(error => {
    // stdout is protocol-only. Keep diagnostics on stderr and use a nonzero
    // exit code so the CLI can distinguish host startup failure from Later.
    try { process.stderr.write(`${asErrorMessage(error)}\n`); } catch {}
    process.exitCode = 1;
  });
}

export {
  APP_PATH,
  HOST_MAX_LINE_BYTES,
  createPermissionHost,
  encodeHostMessage,
  makeAppKitRuntime,
  parseHostArgs,
  parseHostInput,
  startAppKitEventPump,
  runPermissionHost,
};
