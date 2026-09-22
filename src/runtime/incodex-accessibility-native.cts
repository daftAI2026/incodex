// @ts-nocheck
// Native presentation adapted from Cavalry-i18n e76175fe (MIT).
// Copyright (c) 2026 daftAI. See LICENSE. Permission decisions stay in the host controller.
const { loadPermissionNativeLibrary } = require("./incodex-permission-native.cts");
let generation = 0;
const APP_PATH = "/Applications/ChatGPT.app";
const rect = (x, y, width, height) => ({ origin: { x, y }, size: { width, height } });

async function createNativeAccessibilitySetupWindow({ appPath, copy, layoutDirection = "leftToRight", loadObjcModule, locateSettings, onHandoff, onBack, electron = null, activate = null, nativeLibrary = null, canPresent = () => true }) {
  if (appPath !== APP_PATH) throw new Error("Native permission guide requires the default ChatGPT app");
  if (!canPresent()) return null;
  const objc = await loadObjcModule();
  // Loading the bridge yields. The host may lose focus to authentication or
  // close in the meantime; never create a foreground guide on stale readiness.
  if (!canPresent()) return null;
  const kit = new objc.NobjcLibrary("/System/Library/Frameworks/AppKit.framework/AppKit");
  const foundation = new objc.NobjcLibrary("/System/Library/Frameworks/Foundation.framework/Foundation");
  const swift = nativeLibrary ?? loadPermissionNativeLibrary(objc);
  const InitialView = swift?.IncodexPermissionInitialView;
  const HelperView = swift?.IncodexPermissionHelperView;
  const ArrowView = swift?.IncodexPermissionArrowView;
  if (!InitialView || !HelperView || !ArrowView) throw new Error("Native permission SwiftUI guide classes are unavailable");
  const activateApp = typeof activate === "function"
    ? activate
    : (options) => electron?.app?.focus?.(options);
  const text = key => typeof copy === "function" ? copy(key) : copy[key] ?? "";
  const nativeLayoutDirection = layoutDirection === "rightToLeft" ? "rightToLeft" : "leftToRight";
  const str = value => foundation.NSString.stringWithUTF8String$(String(value));
  const array = value => foundation.NSArray.arrayWithObject$(value);
  const unique = `IncodexPermission_${process.pid}_${++generation}`;
  let closed = false, state = "pending", settled = false, source = null, helper = null, arrowPanel = null, arrow = null, arrowTracker = null;
  let tracking = null, arrowTimer = null, returnTimer = null, backFlightTimer = null, flight = null, locating = false, attempts = 0, presented = false, dragging = false, dragSession = null, returning = false, retryReady = false, returnSequence = 0;
  let terminalDragOwner = null;
  const closeHandlers = new Set();
  const retryHandlers = new Set();
  let resolveChoice;
  const choice = new Promise(resolve => { resolveChoice = resolve; });
  const resolveOnce = value => { if (!settled) { settled = true; resolveChoice(value); } };
  const panels = [];
  const copyKeys = [
    "title", "body", "permissionTitle", "permissionDescription", "repair", "later", "back",
    "addedTitle", "addedBody", "dragInstruction", "dragInstructionRuns", "completeInSettings", "checking", "repairing",
    "openSettings", "errorTitle", "errorBody",
  ];
  function nativeCopy() {
    const dictionary = foundation.NSMutableDictionary.dictionary();
    for (const key of copyKeys) dictionary.setObject$forKey$(str(text(key)), str(key));
    dictionary.setObject$forKey$(str(nativeLayoutDirection), str("layoutDirection"));
    return dictionary;
  }
  function define(name, superclass, methods, protocols) { return objc.NobjcClass.define({ name: `${unique}_${name}`, superclass, methods, ...(protocols ? { protocols } : {}) }); }
  const flipped = { isFlipped: { types: "B@:", implementation: () => true } };
  function snapshot(view) {
    view.displayIfNeeded(); const bounds = view.bounds();
    const width = Number(bounds?.size?.width); const height = Number(bounds?.size?.height);
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
      throw new Error("Permission view snapshot has empty bounds");
    }
    const rep = view.bitmapImageRepForCachingDisplayInRect$(bounds);
    if (!rep) throw new Error("Permission view could not be captured");
    view.cacheDisplayInRect$toBitmapImageRep$(bounds, rep);
    const image = kit.NSImage.alloc().initWithSize$(bounds.size); image.addRepresentation$(rep); return image;
  }
  const icon = kit.NSImage.alloc().initWithContentsOfFile$(str(`${APP_PATH}/Contents/Resources/icon-chatgpt.png`));
  if (!icon) throw new Error("ChatGPT icon is unavailable");
  let permissionIcon = kit.NSImage.alloc().initWithContentsOfFile$(str("/System/Library/ExtensionKit/Extensions/AccessibilitySettingsExtension.appex/Contents/Resources/UniversalAccessPref.icns"));
  if (!permissionIcon) permissionIcon = kit.NSImage.alloc().initWithContentsOfFile$(str("/System/Library/PreferencePanes/UniversalAccessPref.prefPane/Contents/Resources/UniversalAccessPref.icns"));
  if (!permissionIcon) permissionIcon = kit.NSImage.imageWithSystemSymbolName$accessibilityDescription$(str("accessibility"), str(text("permissionTitle")));

  function stopArrow() { clearTimeout(arrowTimer); clearTimeout(returnTimer); arrowTimer = returnTimer = null; }
  function resetArrow() { try { arrow?.resetToIdentity?.(); } catch {} }
  function stopBackFlightTimer() { clearTimeout(backFlightTimer); backFlightTimer = null; }
  function close() {
    if (closed) return;
    // AppKit retains the source window until its drag ends and may order it
    // again after close. Retire its pixels now and finish disposal at drag end.
    if ((dragging || dragSession) && helper) {
      terminalDragOwner = { panel: helper.panel, session: dragSession };
      try { terminalDragOwner.panel.setAlphaValue$(0); } catch {}
    }
    closed = true; returning = false; retryReady = false; dragging = false; dragSession = null; returnSequence++;
    clearInterval(tracking); tracking = null; stopArrow(); stopBackFlightTimer();
    const activeFlight = flight; flight = null;
    try { activeFlight?.dispose(); } catch {}
    resetArrow();
    resolveOnce("later");
    for (const panel of [...panels]) {
      if (panel === terminalDragOwner?.panel) {
        // Keep the source view tree and session alive until AppKit calls ended.
        const index = panels.indexOf(panel);
        if (index >= 0) panels.splice(index, 1);
        try { panel.setDelegate$(null); } catch {}
        try { panel.orderOut$(null); } catch {}
      } else discardPanel(panel);
    }
    helper = null; arrowPanel = null; arrow = null; arrowTracker = null; appRowView = null;
    const callbacks = [...closeHandlers]; closeHandlers.clear(); retryHandlers.clear();
    for (const callback of callbacks) { try { callback(); } catch {} }
  }
  const Delegate = define("Delegate", "NSObject", {
    "windowWillClose:": { types: "v@:@", implementation: () => close() },
    "allow:": { types: "v@:@", implementation: (_self, sender) => {
      if (closed) return;
      if (state === "pending" && !settled) {
        try { source = captureSource(); }
        catch { source = null; }
        resolveOnce("repair");
        return;
      }
      if (state !== "pending" || !settled || !retryReady) return;
      retryReady = false;
      try { source = captureSource(); }
      catch { restoreInitialPage(true); return; }
      setState("repairing");
      if (retryHandlers.size === 0) { restoreInitialPage(true); return; }
      for (const callback of retryHandlers) {
        try {
          const result = callback();
          if (result && typeof result.then === "function") {
            void Promise.resolve(result).catch(() => {
              if (!closed && state === "repairing" && !retryReady) restoreInitialPage(true);
            });
          }
        } catch {
          if (!closed && state === "repairing" && !retryReady) restoreInitialPage(true);
        }
      }
    } },
    "resumeSettings:": { types: "v@:@", implementation: () => {
      if (closed || returning || dragging || state !== "awaiting-user") return;
      // The existing controller retry reopens Settings without resetting TCC.
      for (const callback of retryHandlers) {
        try { Promise.resolve(callback()).catch(() => { if (!closed && state === "repairing") restoreInitialPage(true); }); }
        catch { if (!closed && state === "repairing") restoreInitialPage(true); }
      }
    } },
    "later:": { types: "v@:@", implementation: () => helper && state === "awaiting-user" ? handleBack() : close() },
    "skip:": { types: "v@:@", implementation: () => { if (!closed && !helper) close(); } },
  });
  const delegate = Delegate.alloc().init();
  function configurePanel(panel, floating) {
    panels.push(panel); panel.setReleasedWhenClosed$(false); panel.setHidesOnDeactivate$(false);
    panel.setDelegate$(delegate); panel.setLevel$(floating ? 3 : 0);
  }
  function discardPanel(panel) {
    const index = panels.indexOf(panel);
    if (index >= 0) panels.splice(index, 1);
    try { panel.setDelegate$(null); } catch {}
    try { panel.orderOut$(null); } catch {}
    try { panel.close(); } catch {}
  }
  // SwiftUI owns the material, text, controls and permission card. AppKit
  // only owns the panel lifetime and the transparent drag/flight shells.
  const INITIAL_WIDTH = 600;
  const INITIAL_MIN_HEIGHT = 312;
  const initial = kit.NSWindow.alloc().initWithContentRect$styleMask$backing$defer$(rect(0, 0, INITIAL_WIDTH, INITIAL_MIN_HEIGHT), 1 | 2 | 32768, 2, false);
  configurePanel(initial, true); initial.setTitle$(str("")); initial.setTitlebarAppearsTransparent$(true); initial.setTitleVisibility$(1);
  let initialView;
  let card;
  try {
    initialView = InitialView.alloc().initWithFrame$(rect(0, 0, INITIAL_WIDTH, INITIAL_MIN_HEIGHT));
    card = initialView.permissionCardView();
    if (!card) throw new Error("Native permission card host is unavailable");
    initialView.configureWithCopy$appIcon$permissionIcon$actionTarget$(nativeCopy(), icon, permissionIcon, delegate);
    initial.setContentView$(initialView);
  } catch (error) {
    discardPanel(initial);
    throw error;
  }
  let initialTitle = text("title");
  let initialBody = text("body");
  let initialAllowEnabled = true;
  let initialPlaceholder = false;

  function fitInitialBody() {
    const preferred = initialView.preferredContentSize();
    const width = Number(preferred?.width); const height = Number(preferred?.height);
    if (!Number.isFinite(width) || width <= 0 || !Number.isFinite(height) || height <= 0) {
      throw new Error("Native permission initial view has invalid preferred size");
    }
    initialView.setFrame$(rect(0, 0, width, height));
    initial.setContentSize$({ width, height });
  }
  function setInitialContent({ title = initialTitle, body = initialBody, allowEnabled = initialAllowEnabled, settingsPlaceholder = initialPlaceholder } = {}) {
    initialTitle = String(title); initialBody = String(body);
    initialAllowEnabled = Boolean(allowEnabled); initialPlaceholder = Boolean(settingsPlaceholder);
    initialView.setContentWithTitle$body$allowEnabled$settingsPlaceholder$(
      str(initialTitle), str(initialBody), initialAllowEnabled, initialPlaceholder,
    );
  }
  function showSettingsPlaceholder(show) {
    setInitialContent({ settingsPlaceholder: show });
    card.setHidden$(show);
  }
  function captureSource() {
    const image = initialView.snapshotPermissionCardWithScale$(Number(initial.backingScaleFactor()));
    if (!image) throw new Error("Permission card foreground snapshot unavailable");
    return { frame: initial.convertRectToScreen$(card.convertRect$toView$(card.bounds(), null)),
      image, radius: 24 };
  }

  // CUA foreground and snapshot both carry a fixed 531x108 SwiftUI frame;
  // the observed AppKit shell is 531x110. Long Incodex translations retain
  // that width and add only their measured extra line height.
  const HELPER_WIDTH = 531;
  const HELPER_HEIGHT = 110;
  const HELPER_ROW_X = 62;
  const HELPER_ROW_Y = 48;
  const HELPER_ROW_WIDTH = 459;
  const HELPER_ROW_HEIGHT = 42;
  const HELPER_ARROW_WINDOW_X = 30;
  const HELPER_ARROW_WINDOW_Y = 60;
  const HELPER_ARROW_WINDOW_SIZE = 100;
  const HELPER_ARROW_GRAPHIC_SIZE = 28;
  // Reference AX text starts at x102 and has 408pt available. NSTextField's
  // cell adds 2pt on each side, unlike SwiftUI Text's glyph bounds.
  const HELPER_INSTRUCTION_X = 100;
  const HELPER_INSTRUCTION_WIDTH = 412;
  function screens() { const values = kit.NSScreen.screens(); return Array.from({ length: Number(values.count()) }, (_, i) => values.objectAtIndex$(i)); }
  function helperFrame(target, size = { width: HELPER_WIDTH, height: HELPER_HEIGHT }) {
    const first = screens()[0];
    if (!first) throw new Error("No display is available for the permission guide");
    const primary = first.frame();
    // CUA Accessibility accessory: trailing/bottom inset 10pt inside Settings.
    // Convert Quartz window bounds to AppKit once, then match CGRectIntegral.
    // ScreenRecording has a separate heading-aware branch; we do not request it.
    const width = Number(size.width);
    const height = Number(size.height);
    const x = target.x + target.width - width - 10;
    const y = primary.origin.y + primary.size.height - target.y - target.height + 10;
    const left = Math.floor(x), bottom = Math.floor(y);
    return rect(left, bottom, Math.ceil(x + width) - left, Math.ceil(y + height) - bottom);
  }
  function animateArrow(x, y) {
    if (!arrow || closed) return;
    arrow.animateToScaleX$scaleY$(x, y);
  }
  const reducedMotion = () => Boolean(kit.NSWorkspace.sharedWorkspace().accessibilityDisplayShouldReduceMotion());
  function stretchArrow() {
    stopArrow();
    if (!presented || closed) return;
    if (reducedMotion()) { resetArrow(); return; }
    if (dragging) { scheduleArrow(4000); return; }
    animateArrow(1.15, 1.6);
    returnTimer = setTimeout(() => {
      if (closed || dragging) return;
      animateArrow(1, 1); scheduleArrow(4000);
    }, 250);
  }
  function scheduleArrow(delay = 500) {
    stopArrow(); if (closed) return;
    if (reducedMotion()) { resetArrow(); return; }
    arrowTimer = setTimeout(stretchArrow, delay);
  }
  const ArrowTracker = define("ArrowTracker", "NSView", { ...flipped,
    "mouseEntered:": { types: "v@:@", implementation: stretchArrow },
  });
  let appRowView;
  const Drag = define("Drag", "NSView", { ...flipped,
    "acceptsFirstMouse:": { types: "B@:@", implementation: () => true },
    "mouseDown:": { types: "v@:@", implementation: (self, event) => {
      if (closed || state !== "awaiting-user") return;
      const item = kit.NSPasteboardItem.alloc().init(); item.setDataProvider$forTypes$(self, array(str("public.file-url")));
      const dragging = kit.NSDraggingItem.alloc().initWithPasteboardWriter$(item);
      const dragFrame = appRowView.convertRect$toView$(appRowView.bounds(), self);
      dragging.setDraggingFrame$contents$(dragFrame, snapshot(appRowView));
      dragSession = self.beginDraggingSessionWithItems$event$source$(array(dragging), event, self);
      dragSession.setAnimatesToStartingPositionsOnCancelOrFail$(true);
    } },
    "pasteboard:item:provideDataForType:": { types: "v@:@@@", implementation: (_self, _pasteboard, item, type) => {
      if (String(type) === "public.file-url" || type.isEqualToString$(str("public.file-url"))) item.setString$forType$(foundation.NSURL.fileURLWithPath$(str(APP_PATH)).absoluteString(), str("public.file-url"));
    } },
    "draggingSession:sourceOperationMaskForDraggingContext:": { types: "Q@:@q", implementation: () => 1 },
    "ignoreModifierKeysForDraggingSession:": { types: "B@:@", implementation: () => true },
    "draggingSession:willBeginAtPoint:": { types: "v@:@{CGPoint=dd}", implementation: () => {
      if (closed) return;
      dragging = true; stopArrow(); if (reducedMotion()) resetArrow(); else animateArrow(1, 1); appRowView.setHidden$(true);
    } },
    "draggingSession:endedAtPoint:operation:": { types: "v@:@{CGPoint=dd}Q", implementation: () => {
      dragging = false; dragSession = null;
      if (closed) {
        const owner = terminalDragOwner; terminalDragOwner = null;
        if (owner) discardPanel(owner.panel);
        return;
      }
      if (!closed) {
        appRowView.setHidden$(false); scheduleArrow(4000);
      }
    } },
  }, ["NSDraggingSource", "NSPasteboardItemDataProvider"]);

  function positionArrow(frame) {
    // The original child window is positioned from the helper's bottom-left
    // origin, independently of the fitted helper height.
    const arrowY = frame.origin.y + HELPER_ARROW_WINDOW_Y;
    const arrowX = nativeLayoutDirection === "rightToLeft"
      ? frame.size.width - HELPER_ARROW_WINDOW_X - HELPER_ARROW_WINDOW_SIZE
      : HELPER_ARROW_WINDOW_X;
    arrowPanel.setFrame$display$(rect(
      frame.origin.x + arrowX,
      arrowY,
      HELPER_ARROW_WINDOW_SIZE,
      HELPER_ARROW_WINDOW_SIZE,
    ), false);
  }
  const HelperPanel = define("HelperPanel", "NSPanel", {
    canBecomeKeyWindow: { types: "B@:", implementation: () => false },
    canBecomeMainWindow: { types: "B@:", implementation: () => false },
  });
  const ArrowPanel = define("ArrowPanel", "NSPanel", {
    canBecomeKeyWindow: { types: "B@:", implementation: () => false },
    canBecomeMainWindow: { types: "B@:", implementation: () => false },
  });
  function createHelper(frame) {
    const panel = HelperPanel.alloc().initWithContentRect$styleMask$backing$defer$(frame,0x8091,2,false); configurePanel(panel,true);
    try {
      panel.setOpaque$(false); panel.setBackgroundColor$(kit.NSColor.whiteColor().colorWithAlphaComponent$(.001)); panel.setHasShadow$(true); panel.setIgnoresMouseEvents$(false);
      panel.setMovableByWindowBackground$(false); panel.setMovable$(false);
      panel.setTitleVisibility$(1); panel.setTitlebarAppearsTransparent$(true); panel.setToolbarStyle$(3);
      panel.setCollectionBehavior$(0x24a);
      const view = HelperView.alloc().initWithFrame$(rect(0, 0, HELPER_WIDTH, HELPER_HEIGHT));
      view.configureWithCopy$appIcon$actionTarget$(nativeCopy(), icon, delegate);
      const preferred = view.preferredContentSize();
      const fittedSize = { width: Number(preferred?.width), height: Number(preferred?.height) };
      if (!Number.isFinite(fittedSize.width) || fittedSize.width <= 0 || !Number.isFinite(fittedSize.height) || fittedSize.height <= 0) {
        throw new Error("Native permission helper has invalid preferred size");
      }
      view.setFrame$(rect(0, 0, fittedSize.width, fittedSize.height));
      const controller = kit.NSViewController.alloc().init();
      controller.setView$(view);
      panel.setContentViewController$(controller);
      const rowFrame = view.appRowFrame();
      appRowView = view.appRowView();
      if (!appRowView || !rowFrame) throw new Error("Native permission helper row host is unavailable");
      // Keep drag tracking in AppKit, while the visible row remains the native
      // SwiftUI host. The overlay frame is supplied by the native layout ABI.
      const row = Drag.alloc().initWithFrame$(rowFrame);
      view.addSubview$(row);
      arrowPanel=ArrowPanel.alloc().initWithContentRect$styleMask$backing$defer$(rect(0,0,HELPER_ARROW_WINDOW_SIZE,HELPER_ARROW_WINDOW_SIZE),128,2,false); configurePanel(arrowPanel,true); arrowPanel.setOpaque$(false); arrowPanel.setBackgroundColor$(kit.NSColor.clearColor()); arrowPanel.setHasShadow$(false);
      const canvas=kit.NSView.alloc().initWithFrame$(rect(0,0,HELPER_ARROW_WINDOW_SIZE,HELPER_ARROW_WINDOW_SIZE)); canvas.setWantsLayer$(true); canvas.layer().setMasksToBounds$(false);
      arrow=ArrowView.alloc().initWithFrame$(rect(36,10,HELPER_ARROW_GRAPHIC_SIZE,HELPER_ARROW_GRAPHIC_SIZE)); arrow.setFrame$(rect(36,10,HELPER_ARROW_GRAPHIC_SIZE,HELPER_ARROW_GRAPHIC_SIZE));
      arrowTracker=ArrowTracker.alloc().initWithFrame$(rect(36,10,HELPER_ARROW_GRAPHIC_SIZE,HELPER_ARROW_GRAPHIC_SIZE)); arrowTracker.setFrame$(rect(36,10,HELPER_ARROW_GRAPHIC_SIZE,HELPER_ARROW_GRAPHIC_SIZE));
      canvas.addSubview$(arrow); canvas.addSubview$(arrowTracker); arrowPanel.setContentView$(canvas); panel.addChildWindow$ordered$(arrowPanel,1); positionArrow(frame);
      const area=kit.NSTrackingArea.alloc().initWithRect$options$owner$userInfo$(arrowTracker.bounds(),1|128|512,arrowTracker,null); arrowTracker.addTrackingArea$(area);
      return {panel,view,frame,size:fittedSize,radius:12,row};
    } catch (error) {
      const child = arrowPanel;
      arrowPanel = null; arrow = null; arrowTracker = null; appRowView = null;
      if (child) discardPanel(child);
      discardPanel(panel);
      throw error;
    }
  }
  function disposeHelper() {
    const child = arrowPanel;
    const owner = helper?.panel;
    resetArrow();
    const windows = [child, owner].filter(Boolean);
    owner?.removeChildWindow$?.(child);
    for (const panel of windows) {
      const index = panels.indexOf(panel);
      if (index >= 0) panels.splice(index, 1);
    }
    helper = null; arrowPanel = null; arrow = null; arrowTracker = null; appRowView = null; presented = false; dragging = false; dragSession = null;
    for (const panel of new Set(windows)) {
      panel.setDelegate$(null); panel.orderOut$(null); panel.close();
    }
  }
  function restoreInitialPage(enableRetry) {
    if (closed) return;
    clearInterval(tracking); tracking = null; stopArrow(); stopBackFlightTimer();
    disposeHelper(); showSettingsPlaceholder(false); returning = false; state = "pending"; retryReady = Boolean(enableRetry);
    setInitialContent({ title: text("title"), body: text("body"), allowEnabled: Boolean(enableRetry), settingsPlaceholder: false });
    fitInitialBody();
    initial.setLevel$(3); activateApp({ steal: true }); initial.makeKeyAndOrderFront$(null);
  }
  function fallbackToInitial() {
    returnSequence++;
    stopBackFlightTimer();
    const active = flight; flight = null;
    try { active?.dispose?.(); } catch {}
    restoreInitialPage(true);
  }
  function revealHelper() {
    if (closed || returning || state !== "awaiting-user") return;
    presented=true; helper.panel.orderFrontRegardless(); arrowPanel.orderFront$(null);
    // Present the nonactivating accessory, then hand focus to Settings once.
    // Never activate the guide host here or repeat this during drag callbacks.
    try {
      const applications = kit.NSRunningApplication.runningApplicationsWithBundleIdentifier$(str("com.apple.systempreferences"));
      if (Number(applications.count()) === 1) applications.objectAtIndex$(0)?.activateWithOptions$(1);
    } catch {}
    scheduleArrow();
  }
  function handleBack() {
    if (closed || returning || dragging || state !== "awaiting-user" || !helper) return;
    // Restore the floating level only when the reverse flight completes (or
    // its fallback restores the initial page), not while its replica is moving.
    returning = true; retryReady = false; const token = ++returnSequence;
    clearInterval(tracking); tracking = null; stopArrow();
    try {
      const application = kit.NSApplication.sharedApplication();
      if (typeof application.activate === "function") application.activate();
      else application.activateIgnoringOtherApps$(false);
    } catch {
      fallbackToInitial();
      return;
    }
    setInitialContent({ title: text("title"), body: text("body"), allowEnabled: true, settingsPlaceholder: false });
    fitInitialBody();
    if (reducedMotion() || !onBack || !helper.flightTarget) { fallbackToInitial(); return; }
    let returnSource;
    // Capture the restored card synchronously, then keep the placeholder on
    // screen until completion. Revealing the real card early duplicates it
    // beneath the flying replica (unlike the reference Back transition).
    try { card.setHidden$(false); returnSource = captureSource(); card.setHidden$(true); }
    catch { fallbackToInitial(); return; }
    initial.orderFront$(null);
    let active;
    try {
      // Keep the accessory window lifetime through completion while its
      // foreground replica takes over drawing the return transition.
      helper.panel.setAlphaValue$(0); helper.view.setAlphaValue$(0); arrowPanel?.setAlphaValue$(0);
      active = onBack({ objc, source: returnSource, target: helper.flightTarget, reverse: true, isClosed: () => closed });
    } catch { fallbackToInitial(); return; }
    if (!active?.finished || typeof active.finished.then !== "function") {
      try { active?.dispose?.(); } catch {}
      fallbackToInitial();
      return;
    }
    flight = active;
    const finish = () => {
      if (closed || token !== returnSequence || flight !== active) return;
      try { active.dispose?.(); } catch {}
      flight = null; stopBackFlightTimer(); restoreInitialPage(true);
    };
    backFlightTimer = setTimeout(() => {
      if (token === returnSequence && flight === active) fallbackToInitial();
    }, 5000);
    void Promise.resolve(active.finished).then(finish, finish);
  }
  async function place() {
    if (closed || returning || state !== "awaiting-user" || locating) return;
    locating=true;
    try {
      const target=await locateSettings?.(); if (closed || returning || state !== "awaiting-user") return;
      if (!target) {
        attempts++;
        if (helper) { if (attempts >= 10 && !dragging) close(); }
        else if (attempts >= 50) setState("error");
        return;
      }
      attempts = 0;
      const frame=helperFrame(target, helper?.size);
      if (!helper) {
        const created = createHelper(frame);
        const fittedFrame = helperFrame(target, created.size);
        created.frame = fittedFrame;
        created.panel.setFrame$display$(fittedFrame, false);
        positionArrow(fittedFrame);
        helper=created;
        if (target && source) {
          // Original TransitionCapture.cornerRadius is 14; this is separate
          // from the ordinary helper window's material/clip styling.
          helper.flightTarget = { panel: helper.panel, view: helper.view, radius: 14,
            captureImage: () => created.view.snapshotImageWithScale$(Number(created.panel.backingScaleFactor())),
            frame: helper.panel.convertRectToScreen$(helper.view.bounds()) };
          if (onHandoff) {
            const activeFlight=onHandoff({objc,source,target:helper.flightTarget,isClosed:()=>closed});
            flight=activeFlight;
            void Promise.resolve(activeFlight?.finished).then(() => {
              if (flight !== activeFlight || closed || returning) return;
              flight=null;
              revealHelper();
            }).catch(() => {
              if (flight !== activeFlight || closed || returning) return;
              flight=null; setState("error");
            });
          } else revealHelper();
        } else revealHelper();
      } else if (!dragging) {
        helper.frame=frame; helper.panel.setFrame$display$(frame,false); positionArrow(frame);
        if (helper.flightTarget) helper.flightTarget.frame = helper.panel.convertRectToScreen$(helper.view.bounds());
      }
    } catch (error) {
      if (!closed) setState("error");
    } finally { locating=false; }
  }
  function setState(next) {
    if (closed) return;
    state=next;
    if (next === "repairing" || next === "awaiting-user") initial.setLevel$(0);
    if (next==="granted") { close(); return; }
    if (next==="repairing") {
      setInitialContent({ body: text("repairing"), allowEnabled: false, settingsPlaceholder: false });
      fitInitialBody();
    }
    if (next==="awaiting-user") {
      setInitialContent({ body: text("body"), allowEnabled: false, settingsPlaceholder: true });
      showSettingsPlaceholder(true);
      fitInitialBody();
      if (!tracking) tracking=setInterval(()=>void place(),100); void place();
    }
    if (next==="error" || next==="unknown") {
      clearInterval(tracking);tracking=null;stopArrow();flight?.dispose();flight=null;
      disposeHelper(); initial.setLevel$(3);
      setInitialContent({ title: text("errorTitle"), body: text("errorBody"), allowEnabled: false, settingsPlaceholder: false });
      card.setHidden$(false); fitInitialBody(); initial.orderFront$(null);
    }
  }
  try {
    setInitialContent();
    fitInitialBody();
  } catch (error) {
    discardPanel(initial);
    throw error;
  }
  initial.center(); initial.setLevel$(3); activateApp({ steal: true }); initial.makeKeyAndOrderFront$(null);
  return {choice,setState,close,isDestroyed:()=>closed,
    onClose:callback=>{closeHandlers.add(callback);return()=>closeHandlers.delete(callback);},
    onRetry:callback=>{if(typeof callback!=="function") return ()=>{}; retryHandlers.add(callback); return()=>retryHandlers.delete(callback);}};
}
export {createNativeAccessibilitySetupWindow};
