// @ts-nocheck
// Adapted from Cavalry-i18n e76175fe (MIT), macos_permission_handoff.m.
// Copyright (c) 2026 daftAI. See LICENSE.
const { runPermissionFlight, alignPermissionFrame } = require("./incodex-permission-motion.cts");
const { createPermissionGraphics } = require("./incodex-permission-graphics.cts");
let sequence = 0;
const rect = (x, y, width, height) => ({ origin: { x, y }, size: { width, height } });

function snapshot(kit, view) {
  if (!view) throw new Error("Native permission snapshot view is unavailable");
  view.displayIfNeeded?.();
  const bounds = view.bounds();
  const width = Number(bounds?.size?.width);
  const height = Number(bounds?.size?.height);
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    throw new Error("Native permission snapshot has empty bounds");
  }
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
  const graphics = createPermissionGraphics(objc);
  const string = value => foundation.NSString.stringWithUTF8String$(value);
  const targetImage = snapshot(kit, target.view);
  const Panel = objc.NobjcClass.define({ name: `IncodexPermissionFlight_${process.pid}_${++sequence}`,
    superclass: "NSPanel", methods: {
      canBecomeKeyWindow: { types: "B@:", implementation: () => false },
      canBecomeMainWindow: { types: "B@:", implementation: () => false },
    } });
  const entries = [];
  const allowsBlur = !kit.NSWorkspace.sharedWorkspace().accessibilityDisplayShouldReduceTransparency();
  let topologyKey = null;
  function closeEntries(items) {
    for (const item of items) { item.panel.orderOut$(null); item.panel.close(); }
  }
  function screenSnapshot() {
    const screens = kit.NSScreen.screens();
    const specs = [];
    const key = [];
    for (let i = 0; i < Number(screens.count()); i++) {
      const screen = screens.objectAtIndex$(i);
      const raw = screen.frame();
      const scale = Number(screen.backingScaleFactor());
      const aligned = alignPermissionFrame({ x: raw.origin.x, y: raw.origin.y,
        width: raw.size.width, height: raw.size.height }, scale);
      const frame = rect(aligned.x, aligned.y, aligned.width, aligned.height);
      specs.push({ screen, frame, scale });
      key.push(`${raw.origin.x},${raw.origin.y},${raw.size.width},${raw.size.height}@${scale}`);
    }
    return { specs, key: key.join(";") };
  }
  function dispose() {
    closeEntries(entries.splice(0));
  }
  function buildEntries(specs) {
    const next = [];
    try {
      for (const { frame, scale } of specs) {
      const panel = Panel.alloc().initWithContentRect$styleMask$backing$defer$(frame, 128, 2, false);
      panel.setReleasedWhenClosed$(false);
      // Keep the panel owned even if construction of its children fails.
      const item = { panel, frame, scale }; next.push(item);
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
        graphics.setBlackColor(layer, "shadowColor", 1);
        layer.setShadowOpacity$(opacity); layer.setShadowRadius$(radius);
        layer.setShadowOffset$({ width: 0, height: y }); layer.setZPosition$(z);
        surface.layer().addSublayer$(layer); return layer;
      });
      const stroke = quartz.CALayer.layer();
      stroke.setBorderWidth$(.5); graphics.setBlackColor(stroke, "borderColor", .15);
      stroke.setZPosition$(3); surface.layer().addSublayer$(stroke);
      const images = [source.image, targetImage].map(image => {
        const view = kit.NSImageView.alloc().initWithFrame$(rect(0, 0, 1, 1));
        view.setImage$(image); view.setImageScaling$(1); view.setWantsLayer$(true);
        view.layer().setMasksToBounds$(false); view.layer().setContentsScale$(scale);
        surface.addSubview$(view); return view;
      });
      Object.assign(item, { surface, shadows, stroke, images });
      }
      return next;
    } catch (error) {
      closeEntries(next);
      throw error;
    }
  }
  function rebuild() {
    const current = screenSnapshot();
    const old = entries.splice(0);
    closeEntries(old);
    const next = buildEntries(current.specs);
    entries.push(...next);
    topologyKey = current.key;
  }
  function blur(view, radius, scale) {
    const layer = view.layer(); layer.setFilters$(null); layer.setShouldRasterize$(false);
    if (!allowsBlur || radius <= 0) return;
    const filter = coreImage.CIFilter.filterWithName$(string("CIGaussianBlur"));
    if (!filter) return;
    filter.setValue$forKey$(foundation.NSNumber.numberWithDouble$(radius), string("inputRadius"));
    layer.setFilters$(foundation.NSArray.arrayWithObject$(filter)); layer.setShouldRasterize$(true); layer.setRasterizationScale$(scale);
  }
  try { rebuild(); } catch (error) { dispose(); throw error; }
  return { dispose, render(sample) {
    const current = screenSnapshot();
    if (current.key !== topologyKey) rebuild();
    quartz.CATransaction.begin(); quartz.CATransaction.setDisableActions$(true);
    try {
      for (const { frame, scale, surface, shadows, stroke, images } of entries) {
        const b = sample.bounds;
        const aligned = alignPermissionFrame({ x: b.x - frame.origin.x, y: b.y - frame.origin.y, width: b.width, height: b.height }, scale);
        const bounds = rect(0, 0, aligned.width, aligned.height);
        surface.setFrame$(rect(aligned.x, aligned.y, aligned.width, aligned.height));
        stroke.setFrame$(bounds); stroke.setCornerRadius$(sample.cornerRadius); stroke.setOpacity$(sample.progress);
        for (const layer of shadows) {
          layer.setFrame$(bounds);
          graphics.setRoundedShadowPath(layer, bounds, 12);
        }
        shadows[1].setShadowOpacity$(.06 * sample.progress);
        images.forEach(view => view.setFrame$(bounds));
        images[0].setAlphaValue$(sample.sourceOpacity); images[1].setAlphaValue$(sample.targetOpacity);
        blur(images[0], sample.sourceBlur, scale); blur(images[1], sample.targetBlur, scale);
      }
    } finally { quartz.CATransaction.commit(); }
    for (const item of entries) { if (!item.shown) { item.panel.orderFront$(null); item.shown = true; } }
  } };
}

function runNativePermissionHandoff(options) {
  let { objc, source, target, isClosed = () => false,
  reducedMotion, reverse = false, now = () => performance.now(), schedule = fn => setTimeout(fn, 1000 / 60), cancel = clearTimeout,
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
    const resolveTarget = () => typeof target === "function" ? target() : target;
    const initialTarget = resolveTarget();
    replicas = createReplicants({ objc, source, target: initialTarget });
    const flip = item => ({ x: item.frame.origin.x, y: -item.frame.origin.y - item.frame.size.height,
      width: item.frame.size.width, height: item.frame.size.height, radius: item.radius ?? 12 });
    stop = runPermissionFlight({ source: flip(source), target: () => flip(resolveTarget()), reducedMotion: false, reverse, now, schedule, cancel,
      render(sample) {
        if (isClosed()) { dispose(); return; }
        const bounds = { ...sample.bounds, y: -sample.bounds.y - sample.bounds.height };
        try { replicas.render({ ...sample, bounds }); } catch (error) { onError(error); dispose(); }
      }, onComplete: dispose });
    if (done) stop();
  } catch (error) { onError(error); dispose(); }
  return { finished, dispose };
}

export { createNativeReplicants, runNativePermissionHandoff };
