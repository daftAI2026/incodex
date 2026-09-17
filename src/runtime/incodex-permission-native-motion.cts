// @ts-nocheck
// Adapted from Cavalry-i18n e76175fe (MIT), macos_permission_handoff.m.
// Copyright (c) 2026 daftAI. See LICENSE.
const { runPermissionFlight, alignPermissionFrame } = require("./incodex-permission-motion.cts");
const { createPermissionGraphics } = require("./incodex-permission-graphics.cts");
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
      const panel = kit.NSPanel.alloc().initWithContentRect$styleMask$backing$defer$(frame, 128, 2, false);
      panel.setReleasedWhenClosed$(false);
      // Keep the panel owned even if construction of its children fails.
      const item = { panel, frame, scale }; next.push(item);
      panel.setOpaque$(false); panel.setBackgroundColor$(kit.NSColor.clearColor());
      panel.setHasShadow$(false); panel.setIgnoresMouseEvents$(true); panel.setLevel$(25);
      panel.setHidesOnDeactivate$(false);
      const root = kit.NSView.alloc().initWithFrame$(rect(0, 0, frame.size.width, frame.size.height));
      const surface = kit.NSView.alloc().initWithFrame$(rect(0, 0, 1, 1));
      surface.setWantsLayer$(true);
      surface.layer().setMasksToBounds$(false);
      surface.layer().setContentsScale$(scale);
      root.setWantsLayer$(true); root.layer().setMasksToBounds$(false);
      panel.setContentView$(root);
      // CUA ReplicantWindow: destination, key and ambient shadows each have
      // an even-odd cutout. The animated container extends 30pt past the card.
      const masks = [];
      const shadows = [[.06, 2, -3], [.09, 15, -5], [.2, 3, 0]].map(([opacity, radius, y]) => {
        const layer = quartz.CALayer.layer();
        graphics.setBlackColor(layer, "shadowColor", 1);
        layer.setShadowOpacity$(opacity); layer.setShadowRadius$(radius);
        layer.setShadowOffset$({ width: 0, height: y }); layer.setMasksToBounds$(false);
        const mask = quartz.CAShapeLayer.layer();
        mask.setFillRule$(string("even-odd")); graphics.setColor(mask, "fillColor", [1, 1, 1, 1]);
        layer.setMask$(mask); masks.push(mask);
        root.layer().addSublayer$(layer); return layer;
      });
      root.addSubview$(surface);
      const strokeView = kit.NSView.alloc().initWithFrame$(rect(0, 0, 1, 1));
      strokeView.setWantsLayer$(true); strokeView.layer().setMasksToBounds$(false);
      const stroke = quartz.CAShapeLayer.layer();
      stroke.setLineWidth$(.5); graphics.setBlackColor(stroke, "strokeColor", 1);
      graphics.setBlackColor(stroke, "fillColor", 0); stroke.setOpacity$(0);
      strokeView.layer().addSublayer$(stroke); root.addSubview$(strokeView);
      const images = [source.image, targetImage].map(image => {
        const view = kit.NSImageView.alloc().initWithFrame$(rect(0, 0, 1, 1));
        view.setImage$(image); view.setImageScaling$(2); view.setWantsLayer$(true);
        view.layer().setMasksToBounds$(false); view.layer().setContentsScale$(scale);
        surface.addSubview$(view); return view;
      });
      const imageSizes = [source.image, targetImage].map(image => image.size());
      Object.assign(item, { root, surface, shadows, masks, strokeView, stroke, images, imageSizes });
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
      for (const { frame, scale, root, surface, shadows, masks, strokeView, stroke, images, imageSizes } of entries) {
        // The reference applies CGRectIntegral in screen points, not nearest
        // backing pixels, before translating into each screen's container.
        const b = sample.bounds;
        const x = Math.floor(b.x), y = Math.floor(b.y);
        const width = Math.ceil(b.x + b.width) - x, height = Math.ceil(b.y + b.height) - y;
        const localX = x - frame.origin.x, localY = y - frame.origin.y;
        const aligned = { x: Math.floor(localX), y: Math.floor(localY),
          width: Math.ceil(localX + width) - Math.floor(localX),
          height: Math.ceil(localY + height) - Math.floor(localY) };
        const bounds = rect(0, 0, aligned.width, aligned.height);
        const outer = rect(0, 0, aligned.width + 60, aligned.height + 60);
        const inner = rect(30, 30, aligned.width, aligned.height);
        root.setFrame$(rect(aligned.x - 30, aligned.y - 30, outer.size.width, outer.size.height));
        surface.setFrame$(inner); strokeView.setFrame$(inner);
        surface.layer().setCornerRadius$(sample.cornerRadius);
        strokeView.layer().setCornerRadius$(sample.cornerRadius);
        const radius = Math.max(0, sample.cornerRadius - .25);
        stroke.setFrame$(bounds); stroke.setOpacity$(.15 * Math.max(0, Math.min(1, sample.progress)));
        graphics.setRoundedPath(stroke, rect(.25, .25, Math.max(0, aligned.width - .5), Math.max(0, aligned.height - .5)), radius);
        shadows.forEach((layer, index) => {
          layer.setFrame$(outer); masks[index].setFrame$(outer);
          graphics.setRoundedShadowPath(layer, inner, radius);
          graphics.setOuterShadowMaskPath(masks[index], outer, inner, radius);
        });
        shadows[0].setOpacity$(Math.max(0, Math.min(1, sample.progress)));
        // CUA 0x100EB75F4 uses NSImage.size for a centered fixed frame,
        // inside an expanding centered frame. The bitmap itself never stretches.
        images.forEach((view, index) => {
          const { width, height } = imageSizes[index];
          view.setFrame$(rect((aligned.width - width) / 2, (aligned.height - height) / 2, width, height));
        });
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
