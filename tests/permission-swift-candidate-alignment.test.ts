/**
 * [INPUT]: 依赖原生权限源码、构建摘要与隔离测试夹具。
 * [OUTPUT]: 验证共享权限卡的源码和分发边界。
 * [POS]: 双行接入的既有合同回归；不申请真实权限。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { readPermissionPresenterSource, swiftDeclarationBody } from "./permission-swift-source.ts";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const presenter = readPermissionPresenterSource(root);
const flight = readFileSync(join(root, "native/macos/permission-host-flight.swift"), "utf8");
const settings = readFileSync(join(root, "native/macos/permission-host-settings.swift"), "utf8");
const sharedGuide = readFileSync(join(root, "src/runtime/incodex-accessibility-native.cts"), "utf8");

test("current Swift presenter retains the verified initial window and foreground capture contracts", () => {
  expect(presenter).toContain("var initialPanel: NSWindow?");
  expect(presenter).toContain("let panel = NSWindow(");
  expect(presenter.includes("panel.isMovableByWindowBackground = true")).toBe(true);
  expect(presenter.includes("panel.backgroundColor = NSColor.white.withAlphaComponent(0.001)")).toBe(true);
  expect(presenter).toContain("initialView.snapshotPermissionCard(scale: scale)");
  expect(presenter).not.toContain("image: permissionHostCachedImage(cardView)");
});

test("current Swift flight keeps one screen host and a moving inner replica", () => {
  expect(flight).toContain("let contentView: NSView");
  expect(flight).toContain("contentView.addSubview(root)");
  expect(flight).toContain("panel.animationBehavior = .none");
  expect(flight).toContain("panel.collectionBehavior = NSWindow.CollectionBehavior(rawValue: 0x1149)");
  expect(flight).toContain("entry.root.layer?.cornerRadius = sample.cornerRadius");
  expect(flight).toContain("entry.surface.layer?.cornerRadius = sample.cornerRadius");
  expect(flight).toContain("entry.strokeView.layer?.cornerRadius = sample.cornerRadius");
});

test("current Swift accessory and drag input preserve the proven window policy", () => {
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
  const drag = swiftDeclarationBody(presenter, "private final class PermissionHostDragView:");
  expect(drag).not.toContain("shouldDelayWindowOrdering");
  expect(drag).not.toContain("preventWindowOrdering");
  const mouseDown = drag.split("override func mouseDown(with event: NSEvent) {")[1]?.split("func pasteboard(")[0] ?? "";
  expect(mouseDown).toContain("beginDraggingSession(with:");

  const reveal = swiftDeclarationBody(presenter, "private func revealHelper() {");
  const back = swiftDeclarationBody(presenter, "private func startBackFlight() {");
  const error = swiftDeclarationBody(presenter, "func setState(").split('case "error":')[1]?.split("default:")[0] ?? "";
  const restore = swiftDeclarationBody(presenter, "private func restoreInitialPage(");
  const close = swiftDeclarationBody(presenter, "public func close() {");
  expect(reveal.indexOf("application.activate(options: .activateAllWindows)")).toBeLessThan(reveal.indexOf("setGuideActivationPolicy(.prohibited)"));
  expect(back.indexOf("setGuideActivationPolicy(.accessory)")).toBeLessThan(back.indexOf("NSApplication.shared.activate(ignoringOtherApps: false)"));
  expect(error).toContain("setGuideActivationPolicy(.accessory)");
  expect(restore).toContain("setGuideActivationPolicy(.accessory)");
  expect(close).toContain("setActivationPolicy(.accessory)");
});

test("current Swift Back keeps the placeholder and ordered helper through the reverse flight", () => {
  const back = swiftDeclarationBody(presenter, "private func startBackFlight() {");
  const restore = swiftDeclarationBody(presenter, "private func restoreInitialPage(");
  expect(back.includes("settingsPlaceholder: true")).toBe(true);
  expect(back.includes("initialPanel?.level = .floating")).toBe(false);
  expect(back.includes("helperPanel?.alphaValue = 0")).toBe(true);
  expect(back.includes("helperView?.alphaValue = 0")).toBe(true);
  expect(back.includes("arrowPanel?.orderOut(nil)")).toBe(true);
  expect(back.includes("helperPanel?.orderOut(nil)")).toBe(false);
  expect(presenter.includes("restoreInitialPage(raiseBeforeHelperDisposal: true)")).toBe(true);
  expect(restore.indexOf("if raiseBeforeHelperDisposal { initialPanel?.level = .floating }")).toBeLessThan(restore.indexOf("disposeHelper()"));
});

test("current Swift presenter adapts platform inputs before projecting the shared view copy", () => {
  const swiftKeys = presenter.match(/let keys = \[([\s\S]*?)\]/)?.[1]?.match(/"[^"]+"/g) ?? [];
  const tsKeys = sharedGuide.match(/const copyKeys = \[([\s\S]*?)\]/)?.[1]?.match(/"[^"]+"/g) ?? [];
  const platformKeys = ["permissionLocale", "permissionListInstruction", "permissionListInstructionRuns"].map(key => JSON.stringify(key));
  for (const key of platformKeys) expect(tsKeys).toContain(key);
  const viewKeys = tsKeys.filter(key => !platformKeys.includes(key));
  expect(swiftKeys.slice(0, viewKeys.length)).toEqual(viewKeys);
  expect(swiftKeys.slice(viewKeys.length)).toEqual(["screenTitle", "screenDescription", "screenAddedTitle", "screenAddedBody", "screenDragInstruction", "screenDragInstructionRuns", "screenChecking", "done", "screenUnknown"].map(key => JSON.stringify(key)));
  expect(presenter).toContain("IncodexPermissionPlatform.copyForCurrentSystem(copy)");
  expect(sharedGuide).toContain("platform.copyForCurrentSystem$(nativeCopy())");
  for (const view of ["IncodexPermissionInitialView", "IncodexPermissionHelperView", "IncodexPermissionArrowView"]) {
    expect(presenter.includes(view)).toBe(true);
    expect(sharedGuide.includes(view)).toBe(true);
  }
  for (const value of [
    "permissionHostInitialWidth: CGFloat = 600", "permissionHostHelperWidth: CGFloat = 531",
    "permissionHostHelperHeight: CGFloat = 110", "permissionHostRowWidth: CGFloat = 459",
    "permissionHostRowHeight: CGFloat = 42", "permissionHostArrowWindowSize: CGFloat = 100",
    "permissionHostArrowWindowX: CGFloat = 31", "permissionHostArrowWindowY: CGFloat = 60",
    "permissionHostArrowGraphicSize: CGFloat = 28",
  ]) expect(presenter.includes(value)).toBe(true);
});

test("current Swift host prepares Settings before polling and uses the shared visible-window filter", () => {
  const awaiting = swiftDeclarationBody(presenter, "func setState(").split('case "awaiting-user":')[1]?.split('case "granted":')[0] ?? "";
  expect(awaiting.includes("prepareHandoff()")).toBe(true);
  expect(awaiting.indexOf("prepareHandoff()")).toBeLessThan(awaiting.indexOf("startSettingsTracking()"));
  expect(settings.includes("public func prepareHandoff()")).toBe(true);
  expect(settings.includes("bounds.width > 600")).toBe(true);
  expect(settings.includes("bounds.height >= 470")).toBe(true);
});

test("current Swift retry enters repairing before IPC and does not retry a failed foreground capture", () => {
  const allow = swiftDeclarationBody(presenter, "fileprivate func handleAllow() {");
  const retry = allow.slice(allow.indexOf("guard state =="));
  expect(retry.includes("guard let endpoint = captureInitialEndpoint() else")).toBe(true);
  expect(retry.includes("restoreInitialPage()")).toBe(true);
  expect(retry.indexOf('setState("repairing", message: nil)')).toBeGreaterThanOrEqual(0);
  expect(retry.indexOf('setState("repairing", message: nil)')).toBeLessThan(retry.indexOf('onEvent("retry")'));
});

test("current Swift Back completion or internal failure restores a usable card without a stale timeout", () => {
  const back = swiftDeclarationBody(presenter, "private func startBackFlight() {");
  const complete = swiftDeclarationBody(presenter, "private func finishBackFlight(sequence: Int) {");
  expect(complete.includes("restoreInitialPage(raiseBeforeHelperDisposal: true)")).toBe(true);
  const started = back.indexOf("active.start()");
  const activeGuard = back.indexOf("guard flight === active, returning, !closed else { return }", started);
  const timer = back.indexOf("backFlightTimer = Timer", started);
  expect(activeGuard).toBeGreaterThan(started);
  expect(timer).toBeGreaterThan(activeGuard);
});

test("current Swift close retains a transparent drag source until AppKit sends ended", () => {
  const close = swiftDeclarationBody(presenter, "public func close() {");
  const ended = swiftDeclarationBody(presenter, "fileprivate func handleDragEnded() {");
  const retain = swiftDeclarationBody(presenter, "private func retainDragSourceUntilEnded() -> Bool {");
  const dispose = swiftDeclarationBody(presenter, "private func disposeHelper() {");
  expect(presenter.includes("var terminalDragPanel: NSPanel?")).toBe(true);
  expect(presenter.includes("var dragSession: NSDraggingSession?")).toBe(true);
  expect(close.includes("retainDragSourceUntilEnded()")).toBe(true);
  expect(dispose.includes("retainDragSourceUntilEnded()")).toBe(true);
  expect(retain.includes("terminalDragPanel = helperPanel")).toBe(true);
  expect(retain.includes("terminalDragSession = dragSession")).toBe(true);
  expect(retain.includes("helperPanel.alphaValue = 0")).toBe(true);
  expect(close.includes("helperPanel !== terminalDragPanel")).toBe(true);
  expect(ended.includes("if closed || terminalDragPanel != nil { finishTerminalDrag(); return }")).toBe(true);
  expect(presenter.includes("func finishTerminalDrag()")).toBe(true);
});

test("current Swift drag does not change helper input policy or raise its window", () => {
  const began = swiftDeclarationBody(presenter, "fileprivate func handleDragBegan() {");
  const ended = swiftDeclarationBody(presenter, "fileprivate func handleDragEnded() {");
  expect(began.includes("helperPanel?.ignoresMouseEvents = true")).toBe(false);
  expect(ended.includes("helperPanel?.ignoresMouseEvents = false")).toBe(false);
  expect(ended.includes("helperPanel?.orderFront(nil)")).toBe(false);
});

test("current Swift forward completion cannot reveal a stale or returning helper", () => {
  const forward = swiftDeclarationBody(presenter, "private func startForwardFlight() {");
  expect(forward.includes("forwardSequence += 1")).toBe(true);
  expect(forward.includes("finishForwardFlight(sequence: sequence)")).toBe(true);
  expect(swiftDeclarationBody(presenter, "func finishForwardFlight(").includes("guard !closed, !returning, state == \"awaiting-user\", forwardSequence == sequence, flight != nil else { return }")).toBe(true);
});

test("current Swift helper reveal preserves accessory order and returns focus to Settings", () => {
  const reveal = swiftDeclarationBody(presenter, "private func revealHelper() {");
  expect(reveal.includes("helperPanel?.orderFrontRegardless()")).toBe(true);
  expect(reveal.includes("application.activate(options: .activateAllWindows)")).toBe(true);
  expect(reveal.indexOf("helperPanel?.orderFrontRegardless()")).toBeLessThan(reveal.indexOf("application.activate(options: .activateAllWindows)"));
});

test("current Swift arrow rechecks Reduce Motion before scheduling and before return spring", () => {
  const pulse = swiftDeclarationBody(presenter, "private func stretchArrow() {");
  const schedule = swiftDeclarationBody(presenter, "private func scheduleArrow(after delay: TimeInterval) {");
  expect(schedule.includes("accessibilityDisplayShouldReduceMotion")).toBe(true);
  expect(pulse.indexOf("accessibilityDisplayShouldReduceMotion", pulse.indexOf("arrowReturnTimer ="))).toBeGreaterThan(0);
});

test("current Swift internal flight errors dispose before presenting the fallback state", () => {
  const forward = swiftDeclarationBody(presenter, "private func startForwardFlight() {");
  const back = swiftDeclarationBody(presenter, "private func startBackFlight() {");
  expect(forward.includes("onError: { [weak self] _ in self?.noteFlightFallback() }")).toBe(true);
  expect(back.includes("onError: { [weak self] _ in self?.noteFlightFallback() }")).toBe(true);
  expect(flight.includes('onError("Native permission foreground snapshot is unavailable"); dispose()')).toBe(true);
  expect(presenter.includes("private func handleFlightError(")).toBe(false);
});

test("current Swift activates the host before Back captures or starts the reverse flight", () => {
  const back = swiftDeclarationBody(presenter, "private func startBackFlight() {");
  const activation = back.indexOf("NSApplication.shared.activate(ignoringOtherApps: false)");
  expect(activation).toBeGreaterThanOrEqual(0);
  expect(activation).toBeLessThan(back.indexOf("setInitialContent("));
  expect(activation).toBeLessThan(back.indexOf("active.start()"));
});

test("current Swift applies error and duplicate-awaiting state in the proven order", () => {
  const awaiting = swiftDeclarationBody(presenter, "func setState(").split('case "awaiting-user":')[1]?.split('case "granted":')[0] ?? "";
  const error = swiftDeclarationBody(presenter, "func setState(").split('case "error":')[1]?.split("default:")[0] ?? "";
  expect(awaiting.includes('let enteringAwaitingUser = state != "awaiting-user"')).toBe(true);
  expect(awaiting.includes('if enteringAwaitingUser { SettingsLocator().prepareHandoff() }')).toBe(true);
  expect(error.indexOf("disposeActiveFlight()")).toBeLessThan(error.indexOf("disposeHelper()"));
});

test("current Swift helper construction failure stays a recoverable guide error", () => {
  const helper = swiftDeclarationBody(presenter, "private func placeHelper() {");
  expect(helper.includes('setState("error", message: "no display is available for the permission guide")')).toBe(true);
  expect(swiftDeclarationBody(presenter, "func createHelper(").includes('setState("error", message: "native permission helper has invalid preferred size")')).toBe(true);
  expect(swiftDeclarationBody(presenter, "func createHelper(").includes('setState("error", message: "ChatGPT icon is unavailable")')).toBe(true);
});
