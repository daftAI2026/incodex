import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const presenter = readFileSync(join(root, "native/macos/permission-host-presenter.swift"), "utf8");
const flight = readFileSync(join(root, "native/macos/permission-host-flight.swift"), "utf8");
const settings = readFileSync(join(root, "native/macos/permission-host-settings.swift"), "utf8");
const sharedGuide = readFileSync(join(root, "src/runtime/incodex-accessibility-native.cts"), "utf8");

test("unshipped Swift presenter retains the verified initial window and foreground capture contracts", () => {
  expect(presenter).toContain("private var initialPanel: NSWindow?");
  expect(presenter).toContain("let panel = NSWindow(");
  expect(presenter.includes("panel.isMovableByWindowBackground = true")).toBe(true);
  expect(presenter.includes("panel.backgroundColor = NSColor.white.withAlphaComponent(0.001)")).toBe(true);
  expect(presenter).toContain("initialView.snapshotPermissionCard(scale: scale)");
  expect(presenter).not.toContain("image: permissionHostCachedImage(cardView)");
});

test("unshipped Swift flight keeps one screen host and a moving inner replica", () => {
  expect(flight).toContain("let contentView: NSView");
  expect(flight).toContain("contentView.addSubview(root)");
  expect(flight).toContain("panel.animationBehavior = .none");
  expect(flight).toContain("panel.collectionBehavior = NSWindow.CollectionBehavior(rawValue: 0x1149)");
  expect(flight).toContain("entry.root.layer?.cornerRadius = sample.cornerRadius");
  expect(flight).toContain("entry.surface.layer?.cornerRadius = sample.cornerRadius");
  expect(flight).toContain("entry.strokeView.layer?.cornerRadius = sample.cornerRadius");
});

test("unshipped Swift accessory and drag input preserve the proven window policy", () => {
  for (const contract of [
    "final class PermissionHostHelperPanel: NSPanel",
    "final class PermissionHostArrowPanel: NSPanel",
    "override var canBecomeKey: Bool { false }",
    "override var canBecomeMain: Bool { false }",
    "override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }",
    "let panel = PermissionHostHelperPanel(",
    "let arrowPanel = PermissionHostArrowPanel(",
    "styleMask: [],",
    "arrowPanel.collectionBehavior = NSWindow.CollectionBehavior(rawValue: 4)",
  ]) expect(presenter.includes(contract)).toBe(true);
});

test("Swift helper protects the activation boundary without changing the reference drag method", () => {
  const drag = presenter.split("private final class PermissionHostDragView:")[1]?.split("public final class PermissionHostPresenter:")[0] ?? "";
  expect(drag).not.toContain("shouldDelayWindowOrdering");
  expect(drag).not.toContain("preventWindowOrdering");
  const mouseDown = drag.split("override func mouseDown(with event: NSEvent) {")[1]?.split("func pasteboard(")[0] ?? "";
  expect(mouseDown).toContain("beginDraggingSession(with:");

  const reveal = presenter.split("private func revealHelper() {")[1]?.split("private func startForwardFlight")[0] ?? "";
  const back = presenter.split("private func startBackFlight() {")[1]?.split("private func finishBackFlight")[0] ?? "";
  const error = presenter.split('case "error":')[1]?.split("default:")[0] ?? "";
  const restore = presenter.split("private func restoreInitialPage(")[1]?.split("private func disposeActiveFlight")[0] ?? "";
  const close = presenter.split("public func close() {")[1]?.split("fileprivate func handleAllow")[0] ?? "";
  expect(reveal.indexOf("application.activate(options: .activateAllWindows)")).toBeLessThan(reveal.indexOf("setGuideActivationPolicy(.prohibited)"));
  expect(back.indexOf("setGuideActivationPolicy(.accessory)")).toBeLessThan(back.indexOf("NSApplication.shared.activate(ignoringOtherApps: false)"));
  expect(error).toContain("setGuideActivationPolicy(.accessory)");
  expect(restore).toContain("setGuideActivationPolicy(.accessory)");
  expect(close).toContain("setActivationPolicy(.accessory)");
});

test("unshipped Swift Back keeps the placeholder and ordered helper through the reverse flight", () => {
  const back = presenter.split("private func startBackFlight() {")[1]?.split("private func finishBackFlight")[0] ?? "";
  const restore = presenter.split("private func restoreInitialPage(")[1]?.split("private func disposeActiveFlight")[0] ?? "";
  expect(back.includes("settingsPlaceholder: true")).toBe(true);
  expect(back.includes("initialPanel?.level = .floating")).toBe(false);
  expect(back.includes("helperPanel?.alphaValue = 0")).toBe(true);
  expect(back.includes("helperView?.alphaValue = 0")).toBe(true);
  expect(back.includes("arrowPanel?.orderOut(nil)")).toBe(true);
  expect(back.includes("helperPanel?.orderOut(nil)")).toBe(false);
  expect(presenter.includes("restoreInitialPage(raiseBeforeHelperDisposal: true)")).toBe(true);
  expect(restore.indexOf("if raiseBeforeHelperDisposal { initialPanel?.level = .floating }")).toBeLessThan(restore.indexOf("disposeHelper()"));
});

test("unshipped Swift presenter passes the same copy keys and uses the same SwiftUI components", () => {
  const swiftKeys = presenter.match(/let keys = \[([\s\S]*?)\]/)?.[1]?.match(/"[^"]+"/g) ?? [];
  const tsKeys = sharedGuide.match(/const copyKeys = \[([\s\S]*?)\]/)?.[1]?.match(/"[^"]+"/g) ?? [];
  expect(swiftKeys).toEqual(tsKeys);
  for (const view of ["IncodexPermissionInitialView", "IncodexPermissionHelperView", "IncodexPermissionArrowView"]) {
    expect(presenter.includes(view)).toBe(true);
    expect(sharedGuide.includes(view)).toBe(true);
  }
  for (const value of [
    "permissionHostInitialWidth: CGFloat = 600", "permissionHostHelperWidth: CGFloat = 531",
    "permissionHostHelperHeight: CGFloat = 110", "permissionHostRowWidth: CGFloat = 459",
    "permissionHostRowHeight: CGFloat = 42", "permissionHostArrowWindowSize: CGFloat = 100",
    "permissionHostArrowWindowX: CGFloat = 30", "permissionHostArrowWindowY: CGFloat = 60",
    "permissionHostArrowGraphicSize: CGFloat = 28",
  ]) expect(presenter.includes(value)).toBe(true);
});

test("unshipped Swift candidate prepares Settings before polling and uses the shared visible-window filter", () => {
  const awaiting = presenter.split('case "awaiting-user":')[1]?.split('case "granted":')[0] ?? "";
  expect(awaiting.includes("prepareHandoff()")).toBe(true);
  expect(awaiting.indexOf("prepareHandoff()")).toBeLessThan(awaiting.indexOf("startSettingsTracking()"));
  expect(settings.includes("public func prepareHandoff()")).toBe(true);
  expect(settings.includes("bounds.width > 600")).toBe(true);
  expect(settings.includes("bounds.height >= 470")).toBe(true);
});

test("unshipped Swift retry enters repairing before IPC and does not retry a failed foreground capture", () => {
  const allow = presenter.split("fileprivate func handleAllow() {")[1]?.split("fileprivate func handleSkip")[0] ?? "";
  const retry = allow.slice(allow.indexOf("guard state =="));
  expect(retry.includes("guard let endpoint = captureInitialEndpoint() else")).toBe(true);
  expect(retry.includes("restoreInitialPage()")).toBe(true);
  expect(retry.indexOf('setState("repairing", message: nil)')).toBeGreaterThanOrEqual(0);
  expect(retry.indexOf('setState("repairing", message: nil)')).toBeLessThan(retry.indexOf('onEvent("retry")'));
});

test("unshipped Swift Back completion or internal failure restores a usable card without a stale timeout", () => {
  const back = presenter.split("private func startBackFlight() {")[1]?.split("private func finishBackFlight")[0] ?? "";
  const complete = presenter.split("private func finishBackFlight(sequence: Int) {")[1]?.split("private func fallbackToInitial")[0] ?? "";
  expect(complete.includes("restoreInitialPage(raiseBeforeHelperDisposal: true)")).toBe(true);
  const started = back.indexOf("active.start()");
  const activeGuard = back.indexOf("guard flight === active, returning, !closed else { return }", started);
  const timer = back.indexOf("backFlightTimer = Timer", started);
  expect(activeGuard).toBeGreaterThan(started);
  expect(timer).toBeGreaterThan(activeGuard);
});

test("unshipped Swift close retains a transparent drag source until AppKit sends ended", () => {
  const close = presenter.split("public func close() {")[1]?.split("fileprivate func handleAllow")[0] ?? "";
  const ended = presenter.split("fileprivate func handleDragEnded() {")[1]?.split("private func report")[0] ?? "";
  expect(presenter.includes("private var terminalDragPanel: NSPanel?")).toBe(true);
  expect(presenter.includes("private var dragSession: NSDraggingSession?")).toBe(true);
  expect(close.includes("terminalDragPanel?.alphaValue = 0")).toBe(true);
  expect(close.includes("helperPanel !== terminalDragPanel")).toBe(true);
  expect(ended.includes("if closed { finishTerminalDrag(); return }")).toBe(true);
  expect(presenter.includes("private func finishTerminalDrag()")).toBe(true);
});

test("unshipped Swift drag does not change helper input policy or raise its window", () => {
  const began = presenter.split("fileprivate func handleDragBegan() {")[1]?.split("fileprivate func handleDragEnded")[0] ?? "";
  const ended = presenter.split("fileprivate func handleDragEnded() {")[1]?.split("private func report")[0] ?? "";
  expect(began.includes("helperPanel?.ignoresMouseEvents = true")).toBe(false);
  expect(ended.includes("helperPanel?.ignoresMouseEvents = false")).toBe(false);
  expect(ended.includes("helperPanel?.orderFront(nil)")).toBe(false);
});

test("unshipped Swift forward completion cannot reveal a stale or returning helper", () => {
  const forward = presenter.split("private func startForwardFlight() {")[1]?.split("private func startBackFlight")[0] ?? "";
  expect(forward.includes("forwardSequence += 1")).toBe(true);
  expect(forward.includes("finishForwardFlight(sequence: sequence)")).toBe(true);
  expect(forward.includes("guard !closed, !returning, state == \"awaiting-user\", forwardSequence == sequence, flight != nil else { return }")).toBe(true);
});

test("unshipped Swift helper reveal preserves accessory order and returns focus to Settings", () => {
  const reveal = presenter.split("private func revealHelper() {")[1]?.split("private func startForwardFlight")[0] ?? "";
  expect(reveal.includes("helperPanel?.orderFrontRegardless()")).toBe(true);
  expect(reveal.includes("application.activate(options: .activateAllWindows)")).toBe(true);
  expect(reveal.indexOf("helperPanel?.orderFrontRegardless()")).toBeLessThan(reveal.indexOf("application.activate(options: .activateAllWindows)"));
});

test("unshipped Swift arrow rechecks Reduce Motion before scheduling and before return spring", () => {
  const pulse = presenter.split("private func stretchArrow() {")[1]?.split("private func stopArrow()")[0] ?? "";
  const schedule = presenter.split("private func scheduleArrow(after delay: TimeInterval) {")[1]?.split("private func stretchArrow()")[0] ?? "";
  expect(schedule.includes("accessibilityDisplayShouldReduceMotion")).toBe(true);
  expect(pulse.indexOf("accessibilityDisplayShouldReduceMotion", pulse.indexOf("arrowReturnTimer ="))).toBeGreaterThan(0);
});

test("unshipped Swift internal flight errors dispose before presenting the fallback state", () => {
  const forward = presenter.split("private func startForwardFlight() {")[1]?.split("private func finishForwardFlight")[0] ?? "";
  const back = presenter.split("private func startBackFlight() {")[1]?.split("private func finishBackFlight")[0] ?? "";
  expect(forward.includes("onError: { [weak self] _ in self?.noteFlightFallback() }")).toBe(true);
  expect(back.includes("onError: { [weak self] _ in self?.noteFlightFallback() }")).toBe(true);
  expect(flight.includes('onError("Native permission foreground snapshot is unavailable"); dispose()')).toBe(true);
  expect(presenter.includes("private func handleFlightError(")).toBe(false);
});

test("unshipped Swift activates the host before Back captures or starts the reverse flight", () => {
  const back = presenter.split("private func startBackFlight() {")[1]?.split("private func finishBackFlight")[0] ?? "";
  const activation = back.indexOf("NSApplication.shared.activate(ignoringOtherApps: false)");
  expect(activation).toBeGreaterThanOrEqual(0);
  expect(activation).toBeLessThan(back.indexOf("setInitialContent("));
  expect(activation).toBeLessThan(back.indexOf("active.start()"));
});

test("unshipped Swift applies error and duplicate-awaiting state in the proven order", () => {
  const awaiting = presenter.split('case "awaiting-user":')[1]?.split('case "granted":')[0] ?? "";
  const error = presenter.split('case "error":')[1]?.split("default:")[0] ?? "";
  expect(awaiting.includes('let enteringAwaitingUser = state != "awaiting-user"')).toBe(true);
  expect(awaiting.includes('if enteringAwaitingUser { SettingsLocator().prepareHandoff() }')).toBe(true);
  expect(error.indexOf("disposeActiveFlight()")).toBeLessThan(error.indexOf("disposeHelper()"));
});

test("unshipped Swift helper construction failure stays a recoverable guide error", () => {
  const helper = presenter.split("private func placeHelper() {")[1]?.split("private func positionArrow")[0] ?? "";
  expect(helper.includes('setState("error", message: "no display is available for the permission guide")')).toBe(true);
  expect(helper.includes('setState("error", message: "native permission helper has invalid preferred size")')).toBe(true);
  expect(helper.includes('setState("error", message: "ChatGPT icon is unavailable")')).toBe(true);
});
