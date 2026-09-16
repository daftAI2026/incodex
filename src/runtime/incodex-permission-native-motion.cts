// @ts-nocheck
// Adapted from Cavalry-i18n e76175fe (MIT), macos_permission_handoff.m.
// Copyright (c) 2026 daftAI. See LICENSE.
const { runPermissionFlight } = require("./incodex-permission-motion.cts");
let sequence = 0;
const rect = (x, y, width, height) => ({ origin: { x, y }, size: { width, height } });

function snapshot(kit, view) {
  view.displayIfNeeded?.();
  const bounds = view.bounds();
  const rep = view.bitmapImageRepForCachingDisplayInRect$(bounds);
  if (!rep) throw new Error("Native permission snapshot is unavailable");
  view.cacheDisplayInRect$toBitmapImageRep$(bounds, rep);
  const image = kit.NSImage.alloc().initWithSize$(bounds.size);
  image.addRepresentation$(rep);
  return image;
}

function createNativeReplicants({ objc, source, target }) {
  const kit = new objc.NobjcLibrary("/System/Library/Frameworks/AppKit.framework/AppKit");
  const foundation = new objc.NobjcLibrary("/System/Library/Frameworks/Foundation.framework/Foundation");
  const quartz = new objc.NobjcLibrary("/System/Library/Frameworks/QuartzCore.framework/QuartzCore");
  const coreImage = new objc.NobjcLibrary("/System/Library/Frameworks/CoreImage.framework/CoreImage");
  // Register CoreGraphics symbols for the existing bridge's public C function dispatcher.
  const graphics = new objc.NobjcLibrary("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics");
  const string = value => foundation.NSString.stringWithUTF8String$(value);
  function setCGColor(layer, selector, alpha) {
    // objc-js cannot return NSColor.CGColor's typed pointer through method dispatch.
    const color = objc.callFunction("CGColorCreateGenericRGB", { returns: "^v", args: ["d", "d", "d", "d"] }, 0, 0, 0, alpha);
    try { layer[selector](color); }
    finally { if (color) objc.callFunction("CGColorRelease", { returns: "v", args: ["^v"] }, color); }
  }
  const targetImage = snapshot(kit, target.view);
  const Panel = objc.NobjcClass.define({ name: `IncodexPermissionFlight_${process.pid}_${++sequence}`,
    superclass: "NSPanel", methods: {
      canBecomeKeyWindow: { types: "B@:", implementation: () => false },
      canBecomeMainWindow: { types: "B@:", implementation: () => false },
    } });
  const screens = kit.NSScreen.screens();
  const entries = [];
  const allowsBlur = !kit.NSWorkspace.sharedWorkspace().accessibilityDisplayShouldReduceTransparency();
  function dispose() {
    for (const item of entries.splice(0)) { item.panel.orderOut$(null); item.panel.close(); }
  }
  try {
    for (let i = 0; i < Number(screens.count()); i++) {
      const screen = screens.objectAtIndex$(i);
      const frame = screen.frame();
      const scale = Number(screen.backingScaleFactor());
      const panel = Panel.alloc().initWithContentRect$styleMask$backing$defer$(frame, 128, 2, false);
      panel.setReleasedWhenClosed$(false);
      // Keep the panel owned even if construction of its children fails.
      const item = { panel, frame, scale }; entries.push(item);
      panel.setOpaque$(false); panel.setBackgroundColor$(kit.NSColor.clearColor());
      panel.setHasShadow$(false); panel.setIgnoresMouseEvents$(true); panel.setLevel$(3);
      panel.setHidesOnDeactivate$(false);
      const root = kit.NSView.alloc().initWithFrame$(rect(0, 0, frame.size.width, frame.size.height));
      const surface = kit.NSView.alloc().initWithFrame$(rect(0, 0, 1, 1));
      surface.setWantsLayer$(true);
      surface.layer().setMasksToBounds$(false);
      surface.layer().setContentsScale$(scale);
      root.addSubview$(surface); panel.setContentView$(root);
      const shadows = [[.2, 3, 0, -3], [.06, 2, -3, -1], [.09, 15, -5, -2]].map(([opacity, radius, y, z]) => {
        const layer = quartz.CALayer.layer();
        setCGColor(layer, "setShadowColor$", 1);
        layer.setShadowOpacity$(opacity); layer.setShadowRadius$(radius);
        layer.setShadowOffset$({ width: 0, height: y }); layer.setZPosition$(z);
        surface.layer().addSublayer$(layer); return layer;
      });
      const stroke = quartz.CALayer.layer();
      stroke.setBorderWidth$(.5); setCGColor(stroke, "setBorderColor$", .15);
      stroke.setZPosition$(3); surface.layer().addSublayer$(stroke);
      const images = [source.image, targetImage].map(image => {
        const view = kit.NSImageView.alloc().initWithFrame$(rect(0, 0, 1, 1));
        view.setImage$(image); view.setImageScaling$(1); view.setWantsLayer$(true);
        view.layer().setMasksToBounds$(false); view.layer().setContentsScale$(scale);
        surface.addSubview$(view); return view;
      });
      Object.assign(item, { surface, shadows, stroke, images });
      panel.orderFront$(null);
    }
  } catch (error) { dispose(); throw error; }
  function blur(view, radius, scale) {
    const layer = view.layer(); layer.setFilters$(null); layer.setShouldRasterize$(false);
    if (!allowsBlur || radius <= 0) return;
    const filter = coreImage.CIFilter.filterWithName$(string("CIGaussianBlur"));
    if (!filter) return;
    filter.setValue$forKey$(foundation.NSNumber.numberWithDouble$(radius), string("inputRadius"));
    layer.setFilters$(foundation.NSArray.arrayWithObject$(filter)); layer.setShouldRasterize$(true); layer.setRasterizationScale$(scale);
  }
  return { dispose, render(sample) {
    quartz.CATransaction.begin(); quartz.CATransaction.setDisableActions$(true);
    try {
      for (const { frame, scale, surface, shadows, stroke, images } of entries) {
        const b = sample.bounds;
        const integral = value => Math.round(value * scale) / scale;
        const bounds = rect(0, 0, integral(b.width), integral(b.height));
        surface.setFrame$(rect(integral(b.x - frame.origin.x), integral(b.y - frame.origin.y), bounds.size.width, bounds.size.height));
        stroke.setFrame$(bounds); stroke.setCornerRadius$(sample.cornerRadius); stroke.setOpacity$(sample.progress);
        for (const layer of shadows) {
          layer.setFrame$(bounds);
          const path = objc.callFunction("CGPathCreateWithRoundedRect", { returns: "^v", args: ["{CGRect={CGPoint=dd}{CGSize=dd}}", "d", "d", "^v"] }, bounds, 12, 12, null);
          try { layer.setShadowPath$(path); } finally { if (path) objc.callFunction("CGPathRelease", { returns: "v", args: ["^v"] }, path); }
        }
        shadows[1].setShadowOpacity$(.06 * sample.progress);
        images.forEach(view => view.setFrame$(bounds));
        images[0].setAlphaValue$(sample.sourceOpacity); images[1].setAlphaValue$(sample.targetOpacity);
        blur(images[0], sample.sourceBlur, scale); blur(images[1], sample.targetBlur, scale);
      }
    } finally { quartz.CATransaction.commit(); }
  } };
}

function runNativePermissionHandoff(options) {
  let { objc, source, target, isClosed = () => false,
  reducedMotion, now = () => performance.now(), schedule = fn => setTimeout(fn, 16), cancel = clearTimeout,
  createReplicants = createNativeReplicants, onError = () => {} } = options;
  let resolve;
  const finished = new Promise(done => { resolve = done; });
  let done = false, stop = null, replicas = null;
  const dispose = () => {
    if (done) return;
    done = true; stop?.();
    try { replicas?.dispose(); } finally { resolve(); }
  };
  try {
    if (reducedMotion === undefined) {
      const kit = new objc.NobjcLibrary("/System/Library/Frameworks/AppKit.framework/AppKit");
      reducedMotion = Boolean(kit.NSWorkspace.sharedWorkspace().accessibilityDisplayShouldReduceMotion());
    }
    if (reducedMotion || isClosed()) { dispose(); return { finished, dispose }; }
    replicas = createReplicants({ objc, source, target });
    const flip = item => ({ x: item.frame.origin.x, y: -item.frame.origin.y - item.frame.size.height,
      width: item.frame.size.width, height: item.frame.size.height, radius: item.radius ?? 12 });
    stop = runPermissionFlight({ source: flip(source), target: flip(target), reducedMotion: false, now, schedule, cancel,
      render(sample) {
        if (isClosed()) { dispose(); return; }
        const bounds = { ...sample.bounds, y: -sample.bounds.y - sample.bounds.height };
        try { replicas.render({ ...sample, bounds }); } catch (error) { onError(error); dispose(); }
      }, onComplete: dispose });
    if (done) stop();
  } catch (error) { onError(error); dispose(); }
  return { finished, dispose };
}

export { runNativePermissionHandoff };
