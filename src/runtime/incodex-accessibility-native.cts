// @ts-nocheck
// Native presentation adapted from Cavalry-i18n e76175fe (MIT).
// Copyright (c) 2026 daftAI. See LICENSE. Permission decisions stay in the host controller.
const { createPermissionGraphics } = require("./incodex-permission-graphics.cts");
const { createPermissionCardBackground } = require("./incodex-permission-card.cts");
let generation = 0;
const APP_PATH = "/Applications/ChatGPT.app";
const rect = (x, y, width, height) => ({ origin: { x, y }, size: { width, height } });

async function createNativeAccessibilitySetupWindow({ appPath, copy, loadObjcModule, locateSettings, onHandoff, onBack, electron = null }) {
  if (appPath !== APP_PATH) throw new Error("Native permission guide requires the default ChatGPT app");
  const objc = await loadObjcModule();
  const kit = new objc.NobjcLibrary("/System/Library/Frameworks/AppKit.framework/AppKit");
  const foundation = new objc.NobjcLibrary("/System/Library/Frameworks/Foundation.framework/Foundation");
  const quartz = new objc.NobjcLibrary("/System/Library/Frameworks/QuartzCore.framework/QuartzCore");
  const graphics = createPermissionGraphics(objc);
  const text = key => typeof copy === "function" ? copy(key) : copy[key] ?? "";
  const str = value => foundation.NSString.stringWithUTF8String$(String(value));
  const array = value => foundation.NSArray.arrayWithObject$(value);
  const selector = name => objc.callFunction("NSSelectorFromString", { returns: ":", args: ["@"] }, str(name));
  const unique = `IncodexPermission_${process.pid}_${++generation}`;
  let closed = false, state = "pending", settled = false, source = null, helper = null, arrowPanel = null, arrow = null;
  let tracking = null, arrowTimer = null, returnTimer = null, backFlightTimer = null, flight = null, locating = false, attempts = 0, presented = false, dragging = false, dragSession = null, returning = false, retryReady = false, returnSequence = 0;
  const closeHandlers = new Set();
  const retryHandlers = new Set();
  let resolveChoice;
  const choice = new Promise(resolve => { resolveChoice = resolve; });
  const resolveOnce = value => { if (!settled) { settled = true; resolveChoice(value); } };
  const panels = [];
  function define(name, superclass, methods, protocols) { return objc.NobjcClass.define({ name: `${unique}_${name}`, superclass, methods, ...(protocols ? { protocols } : {}) }); }
  const flipped = { isFlipped: { types: "B@:", implementation: () => true } };
  const View = define("View", "NSView", flipped);
  const Material = define("Material", "NSVisualEffectView", flipped);
  const NonactivatingPanel = define("NonactivatingPanel", "NSPanel", {
    canBecomeKeyWindow: { types: "B@:", implementation: () => false },
    canBecomeMainWindow: { types: "B@:", implementation: () => false },
  });
  function label(value, frame, size = 13, bold = false, centered = false, secondary = false) {
    const field = kit.NSTextField.labelWithString$(str(value));
    field.setFrame$(frame); field.setFont$(bold ? kit.NSFont.boldSystemFontOfSize$(size) : kit.NSFont.systemFontOfSize$(size));
    field.setAlignment$(centered ? 1 : 0); field.setTextColor$(secondary ? kit.NSColor.secondaryLabelColor() : kit.NSColor.labelColor());
    field.setMaximumNumberOfLines$(0); field.setLineBreakMode$(0);
    return field;
  }
  function imageView(image, frame) {
    const view = kit.NSImageView.alloc().initWithFrame$(frame);
    view.setImage$(image); view.setImageScaling$(3); return view;
  }
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
  function stopBackFlightTimer() { clearTimeout(backFlightTimer); backFlightTimer = null; }
  function close() {
    if (closed) return;
    closed = true; returning = false; retryReady = false; dragging = false; dragSession = null; returnSequence++;
    clearInterval(tracking); tracking = null; stopArrow(); stopBackFlightTimer(); flight?.dispose(); flight = null;
    resolveOnce("later");
    for (const panel of panels) { panel.orderOut$(null); panel.close(); }
    helper = null; arrowPanel = null; arrow = null; appRowView = null;
    for (const callback of closeHandlers) callback(); closeHandlers.clear();
    retryHandlers.clear();
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
    "later:": { types: "v@:@", implementation: () => helper && state === "awaiting-user" ? handleBack() : close() },
  });
  const delegate = Delegate.alloc().init();
  function configurePanel(panel, floating) {
    panels.push(panel); panel.setReleasedWhenClosed$(false); panel.setHidesOnDeactivate$(false);
    panel.setDelegate$(delegate); if (floating) panel.setLevel$(3);
  }
  function button(title, frame, action) {
    const control = kit.NSButton.buttonWithTitle$target$action$(str(title), delegate, selector(action));
    control.setFrame$(frame); control.setBezelStyle$(1); control.setFont$(kit.NSFont.systemFontOfSize$(12)); return control;
  }
  // CUA PermissionView: 600pt width, 28/32pt vertical padding, 64pt icon,
  // 20pt icon-to-title gap, 26pt bold title, 41pt horizontal row inset.
  // A single 80pt Accessibility row replaces the reference's permission list.
  const initial = kit.NSPanel.alloc().initWithContentRect$styleMask$backing$defer$(rect(0, 0, 600, 312), 1 | 2 | 32768, 2, false);
  configurePanel(initial, true); initial.setTitle$(str("")); initial.setTitlebarAppearsTransparent$(true); initial.setTitleVisibility$(1);
  const initialView = View.alloc().initWithFrame$(rect(0, 0, 600, 312));
  const background = Material.alloc().initWithFrame$(rect(0, 0, 600, 312));
  background.setMaterial$(6); background.setBlendingMode$(0); background.setState$(1);
  initialView.addSubview$(background); initial.setContentView$(initialView);
  const dark = String(initial.effectiveAppearance().name()).includes("Dark");
  function surface(frame, radius, fill) {
    const box = kit.NSBox.alloc().initWithFrame$(frame); box.setBoxType$(4); box.setBorderType$(0);
    box.setCornerRadius$(radius); box.setFillColor$(fill); return box;
  }
  initialView.addSubview$(imageView(icon, rect(268, 28, 64, 64)));
  const title = label(text("title"), rect(20, 112, 560, 32), 26, true, true);
  const body = label(text("body"), rect(41, 147, 518, 32), 13, false, true, true);
  initialView.addSubview$(title); initialView.addSubview$(body);
  const card = View.alloc().initWithFrame$(rect(41, 200, 518, 80));
  card.setClipsToBounds$(false); card.setWantsLayer$(true); card.layer().setMasksToBounds$(false);
  card.addSubview$(createPermissionCardBackground({ View, Material, kit, graphics, str, size: { width: 518, height: 80 }, dark }));
  initialView.addSubview$(card);
  card.addSubview$(imageView(permissionIcon, rect(8, 8, 64, 64)));
  const permissionTitle = label(text("permissionTitle"), rect(84, 19, 330, 20), 16);
  permissionTitle.setFont$(kit.NSFont.systemFontOfSize$weight$(16, .3));
  card.addSubview$(permissionTitle);
  card.addSubview$(label(text("permissionDescription"), rect(84, 42, 330, 18), 13, false, false, true));
  const allowSurface = View.alloc().initWithFrame$(rect(436, 26, 62, 28));
  const allow = button(text("repair"), rect(0, 0, 62, 28), "allow:"); allow.setKeyEquivalent$(str("\r"));
  allow.setBordered$(true); allow.setFont$(kit.NSFont.systemFontOfSize$(13));
  allowSurface.addSubview$(allow); card.addSubview$(allowSurface);
  function captureSource() {
    return { frame: initial.convertRectToScreen$(allowSurface.convertRect$toView$(allowSurface.bounds(), null)),
      image: snapshot(allowSurface), radius: 14 };
  }

  function screens() { const values = kit.NSScreen.screens(); return Array.from({ length: Number(values.count()) }, (_, i) => values.objectAtIndex$(i)); }
  function helperFrame(target) {
    const displays = screens(); const first = displays[0];
    if (!first) throw new Error("No display is available for the permission guide");
    const primary = first.frame();
    const appRect = target ? rect(target.x, primary.origin.y + primary.size.height - target.y - target.height, target.width, target.height) : first.visibleFrame();
    const midpoint = { x: appRect.origin.x + appRect.size.width / 2, y: appRect.origin.y + appRect.size.height / 2 };
    const screen = displays.find(value => { const f = value.frame(); return midpoint.x >= f.origin.x && midpoint.x < f.origin.x + f.size.width && midpoint.y >= f.origin.y && midpoint.y < f.origin.y + f.size.height; }) ?? first;
    const area = screen.visibleFrame();
    return rect(Math.round(Math.max(area.origin.x + 20, Math.min(midpoint.x - 266, area.origin.x + area.size.width - 552))), Math.round(area.origin.y + 20), 532, 112);
  }
  function animateArrow(x, y) {
    if (!arrow || closed) return;
    const layer = arrow.layer();
    const transform = objc.callFunction("CATransform3DMakeScale", { returns: "{CATransform3D=dddddddddddddddd}", args: ["d", "d", "d"] }, x, y, 1);
    const spring = quartz.CASpringAnimation.animationWithKeyPath$(str("transform"));
    spring.setMass$(1); spring.setStiffness$(200); spring.setDamping$(11); spring.setInitialVelocity$(0);
    spring.setFromValue$(foundation.NSValue.valueWithCATransform3D$((layer.presentationLayer() ?? layer).transform()));
    spring.setToValue$(foundation.NSValue.valueWithCATransform3D$(transform)); spring.setDuration$(spring.settlingDuration());
    layer.setTransform$(transform); layer.addAnimation$forKey$(spring, str("incodex-permission-arrow"));
  }
  const reducedMotion = () => Boolean(kit.NSWorkspace.sharedWorkspace().accessibilityDisplayShouldReduceMotion());
  function stretchArrow() {
    stopArrow();
    if (!presented || reducedMotion() || closed) return;
    if (dragging) { scheduleArrow(4000); return; }
    animateArrow(1.15, 1.6);
    returnTimer = setTimeout(() => {
      if (closed || dragging) return;
      animateArrow(1, 1); scheduleArrow(4000);
    }, 250);
  }
  function scheduleArrow(delay = 500) {
    stopArrow(); if (closed || reducedMotion()) return;
    arrowTimer = setTimeout(stretchArrow, delay);
  }
  const Arrow = define("Arrow", "NSView", { ...flipped,
    "mouseEntered:": { types: "v@:@", implementation: stretchArrow },
    "drawRect:": { types: "v@:{CGRect={CGPoint=dd}{CGSize=dd}}", implementation: () => {
      const path = kit.NSBezierPath.bezierPath(); const point = (x, y) => ({ x: 2 + x * 24 / 256, y: 2 + y * 24 / 256 });
      const line = (x, y) => path.lineToPoint$(point(x, y));
      const curve = (x,y,a,b,c,d) => path.curveToPoint$controlPoint1$controlPoint2$(point(x,y),point(a,b),point(c,d));
      path.moveToPoint$(point(128,20)); line(232,116); curve(232,132,238.25,122.25,238.25,125.75); line(200,164); curve(184,164,193.75,170.25,190.25,170.25);
      line(160,140); line(160,224); curve(152,232,160,228.42,156.42,232); line(104,232); curve(96,224,99.58,232,96,228.42); line(96,140); line(72,164); curve(56,164,65.75,170.25,62.25,170.25); line(24,132); curve(24,116,17.75,125.75,17.75,122.25); path.closePath();
      path.setLineWidth$(2); path.setLineJoinStyle$(1); kit.NSColor.colorWithSRGBRed$green$blue$alpha$(0,107/255,1,1).setFill(); kit.NSColor.whiteColor().setStroke(); path.fill(); path.stroke();
    } },
  });
  let appRowView;
  const Drag = define("Drag", "NSView", { ...flipped,
    "mouseDown:": { types: "v@:@", implementation: (self, event) => {
      if (closed || state !== "awaiting-user") return;
      const item = kit.NSPasteboardItem.alloc().init(); item.setDataProvider$forTypes$(self, array(str("public.file-url")));
      const dragging = kit.NSDraggingItem.alloc().initWithPasteboardWriter$(item);
      dragging.setDraggingFrame$contents$(appRowView.frame(), snapshot(appRowView));
      dragSession = self.beginDraggingSessionWithItems$event$source$(array(dragging), event, self);
      dragSession.setAnimatesToStartingPositionsOnCancelOrFail$(true);
    } },
    "pasteboard:item:provideDataForType:": { types: "v@:@@@", implementation: (_self, _pasteboard, item, type) => {
      if (String(type) === "public.file-url" || type.isEqualToString$(str("public.file-url"))) item.setString$forType$(foundation.NSURL.fileURLWithPath$(str(APP_PATH)).absoluteString(), str("public.file-url"));
    } },
    "draggingSession:sourceOperationMaskForDraggingContext:": { types: "Q@:@q", implementation: () => 1 },
    "ignoreModifierKeysForDraggingSession:": { types: "B@:@", implementation: () => true },
    "draggingSession:willBeginAtPoint:": { types: "v@:@{CGPoint=dd}", implementation: () => {
      dragging = true; stopArrow(); animateArrow(1, 1); appRowView.setHidden$(true);
      helper?.panel.setIgnoresMouseEvents$(true);
    } },
    "draggingSession:endedAtPoint:operation:": { types: "v@:@{CGPoint=dd}Q", implementation: () => {
      dragging = false; dragSession = null;
      if (!closed) {
        helper?.panel.setIgnoresMouseEvents$(false); helper?.panel.orderFront$(null);
        appRowView.setHidden$(false); scheduleArrow(4000);
      }
    } },
  }, ["NSDraggingSource", "NSPasteboardItemDataProvider"]);

  let instructionX = 0;
  function positionArrow(frame) { arrowPanel.setFrame$display$(rect(frame.origin.x + instructionX - 7, frame.origin.y + 112 - 12 - 28 - 11, 42, 62.8), false); }
  function createHelper(frame) {
    const panel = NonactivatingPanel.alloc().initWithContentRect$styleMask$backing$defer$(frame,128,2,false); configurePanel(panel,true);
    panel.setOpaque$(false); panel.setBackgroundColor$(kit.NSColor.clearColor()); panel.setHasShadow$(false); panel.setIgnoresMouseEvents$(false);
    const view = Material.alloc().initWithFrame$(rect(0,0,532,112)); view.setMaterial$(6); view.setBlendingMode$(0); view.setState$(1); view.setWantsLayer$(true); view.layer().setCornerRadius$(12); view.layer().setMasksToBounds$(true);
    const edge = surface(rect(0,0,532,112),12,kit.NSColor.clearColor());
    edge.setBorderType$(1); edge.setBorderWidth$(.5); edge.setBorderColor$(kit.NSColor.separatorColor()); view.addSubview$(edge);
    panel.setContentView$(view);
    const instruction = label(text("dragInstruction") || text("addedBody"),rect(0,0,480,18),13,false); instruction.setFont$(kit.NSFont.systemFontOfSize$weight$(13, .23)); instruction.sizeToFit();
    const width = Math.min(instruction.frame().size.width,464); instructionX=(532-28-8-width)/2;
    instruction.setFrame$(rect(instructionX+36,17,width,18)); view.addSubview$(instruction);
    const back = button("",rect(16,58,32,32),"later:");
    const backLabel = str(text("back"));
    back.setImage$(kit.NSImage.imageWithSystemSymbolName$accessibilityDescription$(str("chevron.left"),backLabel));
    back.setAccessibilityLabel$(backLabel); back.setBezelStyle$(7); back.setToolTip$(backLabel); view.addSubview$(back);
    const row=Drag.alloc().initWithFrame$(rect(64,52,452,44)); view.addSubview$(row);
    const box=kit.NSBox.alloc().initWithFrame$(rect(0,0,452,44)); box.setBoxType$(4); box.setBorderType$(1); box.setCornerRadius$(8); box.setBorderWidth$(.5); box.setBorderColor$(kit.NSColor.separatorColor()); box.setFillColor$(kit.NSColor.controlBackgroundColor()); row.addSubview$(box);
    appRowView=View.alloc().initWithFrame$(rect(0,0,452,44)); row.addSubview$(appRowView);
    appRowView.addSubview$(imageView(icon,rect(8,8,28,28))); appRowView.addSubview$(label("ChatGPT",rect(44,13,396,18),13));
    arrowPanel=NonactivatingPanel.alloc().initWithContentRect$styleMask$backing$defer$(rect(0,0,42,62.8),128,2,false); configurePanel(arrowPanel,true); arrowPanel.setOpaque$(false); arrowPanel.setBackgroundColor$(kit.NSColor.clearColor()); arrowPanel.setHasShadow$(false);
    const canvas=kit.NSView.alloc().initWithFrame$(rect(0,0,42,62.8)); canvas.setWantsLayer$(true); canvas.layer().setMasksToBounds$(false);
    arrow=Arrow.alloc().initWithFrame$(rect(7,11,28,28)); arrow.setWantsLayer$(true); arrow.layer().setGeometryFlipped$(true); arrow.layer().setAnchorPoint$({x:.5,y:1}); arrow.setFrame$(rect(7,11,28,28)); arrow.layer().setMasksToBounds$(false);
    graphics.setBlackColor(arrow.layer(),"shadowColor",1); arrow.layer().setShadowOpacity$(.23); arrow.layer().setShadowRadius$(7); arrow.layer().setShadowOffset$({width:0,height:4});
    canvas.addSubview$(arrow); arrowPanel.setContentView$(canvas); panel.addChildWindow$ordered$(arrowPanel,1); positionArrow(frame);
    const area=kit.NSTrackingArea.alloc().initWithRect$options$owner$userInfo$(arrow.bounds(),1|128|512,arrow,null); arrow.addTrackingArea$(area);
    return {panel,view,frame,radius:12,row};
  }
  function disposeHelper() {
    const child = arrowPanel;
    const owner = helper?.panel;
    const windows = [child, owner].filter(Boolean);
    owner?.removeChildWindow$?.(child);
    for (const panel of windows) {
      const index = panels.indexOf(panel);
      if (index >= 0) panels.splice(index, 1);
    }
    helper = null; arrowPanel = null; arrow = null; appRowView = null; presented = false; dragging = false; dragSession = null;
    for (const panel of new Set(windows)) {
      panel.setDelegate$(null); panel.orderOut$(null); panel.close();
    }
  }
  function restoreInitialPage(enableRetry) {
    if (closed) return;
    clearInterval(tracking); tracking = null; stopArrow(); stopBackFlightTimer();
    disposeHelper(); returning = false; state = "pending"; retryReady = Boolean(enableRetry);
    title.setStringValue$(str(text("title"))); body.setStringValue$(str(text("body")));
    allow.setEnabled$(Boolean(enableRetry)); initial.orderFront$(null);
  }
  function fallbackToInitial() {
    returnSequence++;
    stopBackFlightTimer();
    const active = flight; flight = null; active?.dispose?.();
    restoreInitialPage(true);
  }
  function revealHelper() {
    if (closed || returning || state !== "awaiting-user") return;
    presented=true; helper.panel.orderFront$(null); arrowPanel.orderFront$(null); scheduleArrow();
  }
  function handleBack() {
    if (closed || returning || dragging || state !== "awaiting-user" || !helper) return;
    returning = true; retryReady = false; const token = ++returnSequence;
    clearInterval(tracking); tracking = null; stopArrow();
    initial.orderFront$(null); title.setStringValue$(str(text("title"))); body.setStringValue$(str(text("body"))); allow.setEnabled$(true);
    if (reducedMotion() || !onBack || !helper.targetRow) { fallbackToInitial(); return; }
    let returnSource;
    try { returnSource = captureSource(); }
    catch { fallbackToInitial(); return; }
    let active;
    try {
      active = onBack({ objc, source: returnSource, target: helper.targetRow, reverse: true, isClosed: () => closed });
    } catch { fallbackToInitial(); return; }
    helper.panel.orderOut$(null); arrowPanel?.orderOut$(null);
    if (!active?.finished || typeof active.finished.then !== "function") { active?.dispose?.(); fallbackToInitial(); return; }
    flight = active;
    const finish = () => {
      if (closed || token !== returnSequence || flight !== active) return;
      active.dispose?.();
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
      const frame=helperFrame(target);
      if (!helper) {
        helper=createHelper(frame); initial.orderOut$(null);
        if (target && source) {
          helper.targetRow = { panel: helper.panel, view: helper.row, radius: 8,
            frame: helper.panel.convertRectToScreen$(helper.row.convertRect$toView$(helper.row.bounds(), null)) };
          if (onHandoff) {
            const activeFlight=onHandoff({objc,source,target:helper.targetRow,isClosed:()=>closed});
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
        if (helper.targetRow) helper.targetRow.frame = helper.panel.convertRectToScreen$(helper.row.convertRect$toView$(helper.row.bounds(), null));
      }
    } catch (error) {
      if (!closed) setState("error");
    } finally { locating=false; }
  }
  function setState(next) {
    if (closed) return;
    state=next;
    if (next==="granted") { close(); return; }
    if (next==="repairing") { allow.setEnabled$(false); body.setStringValue$(str(text("repairing"))); }
    if (next==="awaiting-user") { allow.setEnabled$(false); body.setStringValue$(str(text("checking"))); if (!tracking) tracking=setInterval(()=>void place(),100); void place(); }
    if (next==="error" || next==="unknown") {
      clearInterval(tracking);tracking=null;stopArrow();flight?.dispose();flight=null;
      disposeHelper(); title.setStringValue$(str(text("errorTitle")));body.setStringValue$(str(text("errorBody")));initial.orderFront$(null);
    }
  }
  initial.center(); electron?.app?.focus?.({steal:true}); initial.makeKeyAndOrderFront$(null);
  return {choice,setState,close,isDestroyed:()=>closed,
    onClose:callback=>{closeHandlers.add(callback);return()=>closeHandlers.delete(callback);},
    onRetry:callback=>{if(typeof callback!=="function") return ()=>{}; retryHandlers.add(callback); return()=>retryHandlers.delete(callback);}};
}
export {createNativeAccessibilitySetupWindow};
