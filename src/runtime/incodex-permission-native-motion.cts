// @ts-nocheck
// Adapted from Cavalry-i18n e76175fe (MIT), macos_permission_handoff.m.
// Copyright (c) 2026 daftAI. See LICENSE.
const { runPermissionFlight, alignPermissionFrame } = require("./incodex-permission-motion.cts");
const { createPermissionGraphics } = require("./incodex-permission-graphics.cts");
const { loadPermissionNativeLibrary } = require("./incodex-permission-native.cts");
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

function createNativeReplicants({ objc, nativeLibrary, source, target, reverse = false }) {
  const kit = new objc.NobjcLibrary("/System/Library/Frameworks/AppKit.framework/AppKit");
  const foundation = new objc.NobjcLibrary("/System/Library/Frameworks/Foundation.framework/Foundation");
  const quartz = new objc.NobjcLibrary("/System/Library/Frameworks/QuartzCore.framework/QuartzCore");
  const swiftLibrary = nativeLibrary ?? loadPermissionNativeLibrary(objc);
  const FlightView = swiftLibrary?.IncodexPermissionFlightView;
  if (!FlightView) throw new Error("Native permission SwiftUI flight class is unavailable");
  const graphics = createPermissionGraphics(objc);
  const string = value => foundation.NSString.stringWithUTF8String$(value);
  // CUA renders an appearance-bound SwiftUI foreground for its helper,
  // excluding the live window's Material. Honor that native capture provider
  // on both legs; a failed provider must not reintroduce the full background.
  const targetImage = typeof target.captureImage === "function" ? target.captureImage() : snapshot(kit, target.view);
  if (!targetImage) throw new Error("Native permission foreground snapshot is unavailable");
  // Each leg composites its outgoing snapshot below its incoming snapshot.
  const flightImages = reverse ? [targetImage, source.image] : [source.image, targetImage];
  const entries = [];
  const reduceTransparency = Boolean(kit.NSWorkspace.sharedWorkspace().accessibilityDisplayShouldReduceTransparency());
  let topologyKey = null;
  function closeEntries(items) {
    let firstError;
    const remember = error => { if (!firstError) firstError = error; };
    for (const item of items) {
      try { item.surface?.setSourceImage$targetImage$(null, null); } catch (error) { remember(error); }
      try { item.panel.orderOut$(null); } catch (error) { remember(error); }
      try { item.panel.close(); } catch (error) { remember(error); }
    }
    if (firstError) throw firstError;
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
      // SwiftUI owns the live material and the clipped image ZStack. Keep the
      // AppKit root and its shadow/stroke layers around that native surface.
      const surface = FlightView.alloc().initWithFrame$(rect(0, 0, 1, 1));
      if (!surface) throw new Error("Native permission SwiftUI flight view construction failed");
      item.surface = surface;
      surface.setWantsLayer$(true);
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
      surface.setSourceImage$targetImage$(flightImages[0], flightImages[1]);
      root.addSubview$(surface);
      const strokeView = kit.NSView.alloc().initWithFrame$(rect(0, 0, 1, 1));
      strokeView.setWantsLayer$(true); strokeView.layer().setMasksToBounds$(false);
      const stroke = quartz.CAShapeLayer.layer();
      stroke.setLineWidth$(.5); graphics.setBlackColor(stroke, "strokeColor", 1);
      graphics.setBlackColor(stroke, "fillColor", 0); stroke.setOpacity$(0);
      strokeView.layer().addSublayer$(stroke); root.addSubview$(strokeView);
      Object.assign(item, { root, surface, shadows, masks, strokeView, stroke });
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
  try { rebuild(); } catch (error) { dispose(); throw error; }
  return { dispose, render(sample) {
    const current = screenSnapshot();
    if (current.key !== topologyKey) rebuild();
    quartz.CATransaction.begin(); quartz.CATransaction.setDisableActions$(true);
    try {
      for (const { frame, root, surface, shadows, masks, strokeView, stroke } of entries) {
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
        surface.setFrame$(inner);
        surface.updateProgress$cornerRadius$reduceTransparency$(sample.progress, sample.cornerRadius, reduceTransparency);
        strokeView.setFrame$(inner);
        const strokeRadius = Math.max(0, sample.cornerRadius - .25);
        stroke.setFrame$(bounds); stroke.setOpacity$(.15 * Math.max(0, Math.min(1, sample.progress)));
        graphics.setRoundedPath(stroke, rect(.25, .25, Math.max(0, aligned.width - .5), Math.max(0, aligned.height - .5)), strokeRadius);
        shadows.forEach((layer, index) => {
          layer.setFrame$(outer); masks[index].setFrame$(outer);
          // Shadow silhouette and cutout follow the full clipping shape;
          // only the centered half-point stroke needs a quarter-point inset.
          graphics.setRoundedShadowPath(layer, inner, sample.cornerRadius);
          graphics.setOuterShadowMaskPath(masks[index], outer, inner, sample.cornerRadius);
        });
        shadows[0].setOpacity$(Math.max(0, Math.min(1, sample.progress)));
      }
    } finally { quartz.CATransaction.commit(); }
    for (const item of entries) { if (!item.shown) { item.panel.orderFront$(null); item.shown = true; } }
  } };
}

function runNativePermissionHandoff(options) {
  let { objc, source, target, isClosed = () => false,
  nativeLibrary, reducedMotion, reverse = false, now = () => performance.now(), schedule = fn => setTimeout(fn, 1000 / 60), cancel = clearTimeout,
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
    replicas = createReplicants({ objc, nativeLibrary, source, target: initialTarget, reverse });
    const flip = item => ({ x: item.frame.origin.x, y: -item.frame.origin.y - item.frame.size.height,
      width: item.frame.size.width, height: item.frame.size.height, radius: item.radius ?? 12 });
    // CUA reverse completion's caller supplies helper -> original captures,
    // then initializes a fresh 0 -> 1 spring, including shadows and stroke.
    stop = runPermissionFlight({ source: flip(reverse ? initialTarget : source),
      target: () => flip(reverse ? source : resolveTarget()), reducedMotion: false, now, schedule, cancel,
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
