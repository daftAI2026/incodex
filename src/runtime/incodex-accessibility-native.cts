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
  const VibrantLabel = define("VibrantLabel", "NSTextField", {
    allowsVibrancy: { types: "B@:", implementation: () => true },
  });
  function label(value, frame, size = 13, bold = false, centered = false, secondary = false, vibrant = false) {
    const field = (vibrant ? VibrantLabel : kit.NSTextField).labelWithString$(str(value));
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
    "skip:": { types: "v@:@", implementation: () => { if (!closed && !helper) close(); } },
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
  // The reference keeps a 600x540 guide allocation even when fewer rows are
  // shown; localized body text may grow beyond that minimum.
  const INITIAL_WIDTH = 600;
  const INITIAL_MIN_HEIGHT = 540;
  const INITIAL_SKIP_TRAILING = 57;
  const INITIAL_SKIP_BOTTOM = 12.5;
  const initial = kit.NSPanel.alloc().initWithContentRect$styleMask$backing$defer$(rect(0, 0, INITIAL_WIDTH, INITIAL_MIN_HEIGHT), 1 | 2 | 32768, 2, false);
  configurePanel(initial, true); initial.setTitle$(str("")); initial.setTitlebarAppearsTransparent$(true); initial.setTitleVisibility$(1);
  const initialView = View.alloc().initWithFrame$(rect(0, 0, INITIAL_WIDTH, INITIAL_MIN_HEIGHT));
  const background = Material.alloc().initWithFrame$(rect(0, 0, INITIAL_WIDTH, INITIAL_MIN_HEIGHT));
  background.setMaterial$(6); background.setBlendingMode$(1); background.setState$(1);
  initialView.addSubview$(background); initial.setContentView$(initialView);
  // Reference VStack.offset(y: -9) preserves the padded background and size.
  const contentGroup = View.alloc().initWithFrame$(rect(0, -9, INITIAL_WIDTH, INITIAL_MIN_HEIGHT));
  // Labels participate in the material's native vibrancy composition.
  background.addSubview$(contentGroup);
  const dark = String(initial.effectiveAppearance().name()).includes("Dark");
  function surface(frame, radius, fill) {
    const box = kit.NSBox.alloc().initWithFrame$(frame); box.setBoxType$(4); box.setBorderType$(0);
    box.setCornerRadius$(radius); box.setFillColor$(fill); return box;
  }
  contentGroup.addSubview$(imageView(icon, rect(268, 28, 64, 64)));
  // Reference Text.offset(y: -11) shifts drawing without moving the body/card.
  const title = label(text("title"), rect(20, 112 - 11, 560, 32), 26, true, true, false, true);
  const body = label(text("body"), rect(41, 147, 518, 32), 13, false, true, true, true);
  contentGroup.addSubview$(title); contentGroup.addSubview$(body);
  const card = View.alloc().initWithFrame$(rect(41, 200, 518, 80));
  card.setClipsToBounds$(false); card.setWantsLayer$(true); card.layer().setMasksToBounds$(false);
  card.addSubview$(createPermissionCardBackground({ View, Material, kit, graphics, str, size: { width: 518, height: 80 }, dark }));
  contentGroup.addSubview$(card);
  card.addSubview$(imageView(permissionIcon, rect(8, 8, 64, 64)));
  const permissionTitle = label(text("permissionTitle"), rect(82.5, 20.5, 330, 20), 16, false, false, false, true);
  permissionTitle.setFont$(kit.NSFont.systemFontOfSize$weight$(16, .3));
  card.addSubview$(permissionTitle);
  card.addSubview$(label(text("permissionDescription"), rect(82.5, 42.5, 330, 18), 13, false, false, true, true));
  // Reference: DefaultButtonStyle -> continuous Capsule -> minWidth 62 -> x +4.
  const allowSurface = View.alloc().initWithFrame$(rect(440, 28, 62, 24));
  const allow = button(text("repair"), rect(0, 0, 62, 24), "allow:"); allow.setKeyEquivalent$(str("\r"));
  allow.setBordered$(true); allow.setFont$(kit.NSFont.systemFontOfSize$(13)); allow.sizeToFit();
  const allowSize = allow.frame().size;
  const allowWidth = Math.max(62, Number(allowSize.width));
  allowSurface.setFrame$(rect(498 - allowWidth + 4, (80 - allowSize.height) / 2, allowWidth, allowSize.height));
  allow.setFrame$(rect(Math.round((allowWidth - allowSize.width) / 2), 0, allowSize.width, allowSize.height));
  allow.setWantsLayer$(true); allow.layer().setCornerRadius$(allowSize.height / 2);
  allow.layer().setCornerCurve$(str("continuous")); allow.layer().setMasksToBounds$(true);
  allowSurface.addSubview$(allow); card.addSubview$(allowSurface);
  const skip = button(text("later"), rect(0, 0, 0, 0), "skip:");
  skip.setBezelStyle$(0); skip.setBordered$(false); skip.setFont$(kit.NSFont.systemFontOfSize$(13));
  skip.setAccessibilityLabel$(str(text("later"))); skip.setToolTip$(str(text("later"))); skip.sizeToFit();
  background.addSubview$(skip);
  function fitInitialBody() {
    // CUA description: centered Text.lineSpacing(2), measured after styling.
    const paragraph = kit.NSMutableParagraphStyle.alloc().init();
    paragraph.setAlignment$(1); paragraph.setLineSpacing$(2);
    const attributed = body.attributedStringValue().mutableCopy();
    attributed.addAttribute$value$range$(str("NSParagraphStyle"), paragraph,
      { location: 0, length: Number(attributed.length()) });
    body.setAttributedStringValue$(attributed);
    const measured = Number(body.cell().cellSizeForBounds$(rect(0, 0, 518, 1000)).height);
    if (!Number.isFinite(measured) || measured <= 0) throw new Error("Permission text has invalid native bounds");
    // NSHostingView includes the titled window's safe area. A plain NSView
    // does not lay out inside it automatically; keep the system-provided inset.
    const safeTop = Number(initialView.safeAreaInsets().top);
    const titleHeight = Number(title.fittingSize().height);
    if (!Number.isFinite(safeTop) || safeTop < 0 || !Number.isFinite(titleHeight) || titleHeight <= 0) {
      throw new Error("Permission header has invalid native bounds");
    }
    const bodyHeight = Math.ceil(measured);
    const bodyY = 112 + titleHeight + 3;
    const cardY = bodyY + bodyHeight + 21;
    const contentHeight = Math.max(INITIAL_MIN_HEIGHT, safeTop + cardY + 80 + 32);
    title.setFrame$(rect(20, 112 - 11, 560, titleHeight));
    body.setFrame$(rect(41, bodyY, 518, bodyHeight));
    card.setFrame$(rect(41, cardY, 518, 80));
    initialView.setFrame$(rect(0, 0, INITIAL_WIDTH, contentHeight));
    background.setFrame$(rect(0, 0, INITIAL_WIDTH, contentHeight));
    contentGroup.setFrame$(rect(0, safeTop - 9, INITIAL_WIDTH, contentHeight - safeTop));
    const skipSize = skip.frame().size;
    skip.setFrame$(rect(
      INITIAL_WIDTH - INITIAL_SKIP_TRAILING - Number(skipSize.width),
      contentHeight - INITIAL_SKIP_BOTTOM - Number(skipSize.height),
      Number(skipSize.width),
      Number(skipSize.height),
    ));
    initial.setContentSize$({ width: INITIAL_WIDTH, height: contentHeight });
  }
  function captureSource() {
    return { frame: initial.convertRectToScreen$(allowSurface.convertRect$toView$(allowSurface.bounds(), null)),
      image: snapshot(allowSurface), radius: Number(allow.frame().size.height) / 2 };
  }

  // The original accessory is hosted by an NSHostingView and its window
  // frame comes from fittingSize(). 531x110 is the observed English
  // geometry/fallback, not a universal localized width.
  const HELPER_WIDTH = 531;
  const HELPER_HEIGHT = 110;
  const HELPER_ROW_X = 62;
  const HELPER_ROW_Y = 48;
  const HELPER_ROW_WIDTH = 459;
  const HELPER_ROW_HEIGHT = 42;
  const HELPER_ARROW_WINDOW_X = 31;
  const HELPER_ARROW_WINDOW_Y = 60;
  const HELPER_ARROW_WINDOW_SIZE = 100;
  const HELPER_ARROW_GRAPHIC_SIZE = 28;
  // Settled AX geometry: the instruction cluster starts 5pt inside the row,
  // leaves 7pt after the 28pt arrow, and leaves 11pt at the trailing edge.
  // The native row itself keeps the proven 10pt outer trailing inset.
  const HELPER_INSTRUCTION_LEADING = 5;
  const HELPER_ARROW_TEXT_GAP = 7;
  const HELPER_INSTRUCTION_TRAILING = 11;
  const HELPER_CONTENT_TRAILING = 10;
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

  function positionArrow(frame) {
    // The original child window is positioned from the helper's bottom-left
    // origin, independently of the fitted helper height.
    const arrowY = frame.origin.y + HELPER_ARROW_WINDOW_Y;
    arrowPanel.setFrame$display$(rect(
      frame.origin.x + HELPER_ARROW_WINDOW_X,
      arrowY,
      HELPER_ARROW_WINDOW_SIZE,
      HELPER_ARROW_WINDOW_SIZE,
    ), false);
  }
  function createHelper(frame) {
    const panel = kit.NSPanel.alloc().initWithContentRect$styleMask$backing$defer$(frame,128,2,false); configurePanel(panel,true);
    panel.setOpaque$(false); panel.setBackgroundColor$(kit.NSColor.clearColor()); panel.setHasShadow$(false); panel.setIgnoresMouseEvents$(false);
    const instruction = label(text("dragInstruction") || text("addedBody"),rect(0,0,480,18),13,false); instruction.setFont$(kit.NSFont.systemFontOfSize$weight$(13, .23)); instruction.sizeToFit();
    const referenceInstructionWidth = HELPER_ROW_WIDTH - HELPER_INSTRUCTION_LEADING - HELPER_ARROW_GRAPHIC_SIZE - HELPER_ARROW_TEXT_GAP - HELPER_INSTRUCTION_TRAILING;
    let instructionWidth = Number(instruction.frame().size.width);
    try {
      const measured = instruction.fittingSize();
      const measuredWidth = Number(measured?.width);
      if (Number.isFinite(measuredWidth) && measuredWidth > 0) instructionWidth = measuredWidth;
    } catch {}
    if (!Number.isFinite(instructionWidth) || instructionWidth <= 0) instructionWidth = referenceInstructionWidth;
    const contentRowWidth = Math.max(
      HELPER_ROW_WIDTH,
      HELPER_INSTRUCTION_LEADING + HELPER_ARROW_GRAPHIC_SIZE + HELPER_ARROW_TEXT_GAP + instructionWidth + HELPER_INSTRUCTION_TRAILING,
    );
    const layoutSize = {
      width: HELPER_ROW_X + contentRowWidth + HELPER_CONTENT_TRAILING,
      height: HELPER_HEIGHT,
    };
    const view = Material.alloc().initWithFrame$(rect(0,0,layoutSize.width,layoutSize.height)); view.setMaterial$(6); view.setBlendingMode$(0); view.setState$(1); view.setWantsLayer$(true); view.layer().setCornerRadius$(12); view.layer().setMasksToBounds$(true);
    const edge = surface(rect(0,0,layoutSize.width,layoutSize.height),12,kit.NSColor.clearColor());
    edge.setBorderType$(1); edge.setBorderWidth$(.5); edge.setBorderColor$(kit.NSColor.separatorColor()); view.addSubview$(edge);
    panel.setContentView$(view);
    const instructionX = HELPER_ROW_X + HELPER_INSTRUCTION_LEADING + HELPER_ARROW_GRAPHIC_SIZE + HELPER_ARROW_TEXT_GAP;
    instruction.setFrame$(rect(instructionX,17,instructionWidth,18)); view.addSubview$(instruction);
    const back = button("",rect(18,55,28,28),"later:");
    const backLabel = str(text("back"));
    back.setImage$(kit.NSImage.imageWithSystemSymbolName$accessibilityDescription$(str("chevron.left"),backLabel));
    back.setAccessibilityLabel$(backLabel); back.setBezelStyle$(7); back.setToolTip$(backLabel); view.addSubview$(back);
    const row=Drag.alloc().initWithFrame$(rect(HELPER_ROW_X,HELPER_ROW_Y,contentRowWidth,HELPER_ROW_HEIGHT)); view.addSubview$(row);
    const box=kit.NSBox.alloc().initWithFrame$(rect(0,0,contentRowWidth,HELPER_ROW_HEIGHT)); box.setBoxType$(4); box.setBorderType$(1); box.setCornerRadius$(8); box.setBorderWidth$(.5); box.setBorderColor$(kit.NSColor.separatorColor()); box.setFillColor$(kit.NSColor.controlBackgroundColor()); row.addSubview$(box);
    appRowView=View.alloc().initWithFrame$(rect(0,0,contentRowWidth,HELPER_ROW_HEIGHT)); row.addSubview$(appRowView);
    appRowView.addSubview$(imageView(icon,rect(5,5,32,32))); appRowView.addSubview$(label("ChatGPT",rect(41,13,145.5,16),13));
    const applyLayout = (size) => {
      const width = Number(size?.width);
      const height = Number(size?.height);
      const fittedWidth = Number.isFinite(width) && width > 0 ? Math.max(layoutSize.width, width) : layoutSize.width;
      const fittedHeight = Number.isFinite(height) && height > 0 ? Math.max(layoutSize.height, height) : layoutSize.height;
      const fittedRowWidth = Math.max(HELPER_ROW_WIDTH, fittedWidth - HELPER_ROW_X - HELPER_CONTENT_TRAILING);
      view.setFrame$(rect(0, 0, fittedWidth, fittedHeight));
      edge.setFrame$(rect(0, 0, fittedWidth, fittedHeight));
      instruction.setFrame$(rect(instructionX, 17, instructionWidth, 18));
      row.setFrame$(rect(HELPER_ROW_X, HELPER_ROW_Y, fittedRowWidth, HELPER_ROW_HEIGHT));
      box.setFrame$(rect(0, 0, fittedRowWidth, HELPER_ROW_HEIGHT));
      appRowView.setFrame$(rect(0, 0, fittedRowWidth, HELPER_ROW_HEIGHT));
      return { width: fittedWidth, height: fittedHeight };
    };
    let fittedSize = applyLayout(layoutSize);
    try {
      const measured = view.fittingSize();
      const width = Number(measured?.width);
      const height = Number(measured?.height);
      // A manually hosted visual-effect view may report an empty intrinsic
      // size. The measured NSTextField layout above remains the real minimum;
      // a native hosting fit may enlarge it further.
      if (Number.isFinite(width) && Number.isFinite(height) && width > 0 && height > 0) fittedSize = applyLayout({ width, height });
    } catch {}
    arrowPanel=kit.NSPanel.alloc().initWithContentRect$styleMask$backing$defer$(rect(0,0,HELPER_ARROW_WINDOW_SIZE,HELPER_ARROW_WINDOW_SIZE),128,2,false); configurePanel(arrowPanel,true); arrowPanel.setOpaque$(false); arrowPanel.setBackgroundColor$(kit.NSColor.clearColor()); arrowPanel.setHasShadow$(false);
    const canvas=kit.NSView.alloc().initWithFrame$(rect(0,0,HELPER_ARROW_WINDOW_SIZE,HELPER_ARROW_WINDOW_SIZE)); canvas.setWantsLayer$(true); canvas.layer().setMasksToBounds$(false);
    arrow=Arrow.alloc().initWithFrame$(rect(36,10,HELPER_ARROW_GRAPHIC_SIZE,HELPER_ARROW_GRAPHIC_SIZE)); arrow.setWantsLayer$(true); arrow.layer().setGeometryFlipped$(true); arrow.layer().setAnchorPoint$({x:.5,y:1}); arrow.setFrame$(rect(36,10,HELPER_ARROW_GRAPHIC_SIZE,HELPER_ARROW_GRAPHIC_SIZE)); arrow.layer().setMasksToBounds$(false);
    graphics.setBlackColor(arrow.layer(),"shadowColor",1); arrow.layer().setShadowOpacity$(.23); arrow.layer().setShadowRadius$(7); arrow.layer().setShadowOffset$({width:0,height:4});
    canvas.addSubview$(arrow); arrowPanel.setContentView$(canvas); panel.addChildWindow$ordered$(arrowPanel,1); positionArrow(frame);
    const area=kit.NSTrackingArea.alloc().initWithRect$options$owner$userInfo$(arrow.bounds(),1|128|512,arrow,null); arrow.addTrackingArea$(area);
    return {panel,view,frame,size:fittedSize,radius:12,row};
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
    allow.setEnabled$(Boolean(enableRetry)); fitInitialBody();
    electron?.app?.focus?.({steal:true}); initial.makeKeyAndOrderFront$(null);
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
    title.setStringValue$(str(text("title"))); body.setStringValue$(str(text("body"))); allow.setEnabled$(true); fitInitialBody(); initial.orderFront$(null);
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
      const frame=helperFrame(target, helper?.size);
      if (!helper) {
        const created = createHelper(frame);
        const fittedFrame = helperFrame(target, created.size);
        created.frame = fittedFrame;
        created.panel.setFrame$display$(fittedFrame, false);
        positionArrow(fittedFrame);
        helper=created;
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
      disposeHelper(); title.setStringValue$(str(text("errorTitle")));body.setStringValue$(str(text("errorBody")));fitInitialBody();initial.orderFront$(null);
    }
  }
  fitInitialBody(); initial.center(); electron?.app?.focus?.({steal:true}); initial.makeKeyAndOrderFront$(null);
  return {choice,setState,close,isDestroyed:()=>closed,
    onClose:callback=>{closeHandlers.add(callback);return()=>closeHandlers.delete(callback);},
    onRetry:callback=>{if(typeof callback!=="function") return ()=>{}; retryHandlers.add(callback); return()=>retryHandlers.delete(callback);}};
}
export {createNativeAccessibilitySetupWindow};
