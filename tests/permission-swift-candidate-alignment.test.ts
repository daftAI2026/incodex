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
