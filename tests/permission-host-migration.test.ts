import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const presenterPath = join(import.meta.dir, "..", "native", "macos", "permission-host-presenter.swift");
const source = readFileSync(presenterPath, "utf8");

function functionBody(signature: string, nextSignature: string): string {
  const start = source.indexOf(signature);
  expect(start, `missing Swift function: ${signature}`).toBeGreaterThanOrEqual(0);
  const end = source.indexOf(nextSignature, start + signature.length);
  expect(end, `missing Swift function boundary: ${nextSignature}`).toBeGreaterThan(start);
  return source.slice(start, end);
}

test("helper fitting finishes before the forward flight can start", () => {
  const createHelper = functionBody("private func createHelper(frame: NSRect)", "private func positionArrow");
  const placeHelper = functionBody("private func placeHelper()", "private func createHelper");
  expect(createHelper).not.toContain("startForwardFlight()");
  const fitted = placeHelper.indexOf("helperPanel.setFrame(fittedFrame");
  const flight = placeHelper.indexOf("startForwardFlight()", fitted);
  expect(fitted, "placeHelper must apply the fitted frame").toBeGreaterThanOrEqual(0);
  expect(flight, "placeHelper must start flight after fitting").toBeGreaterThan(fitted);
});

test("losing Settings after helper presentation reports later before closing", () => {
  const placeHelper = functionBody("private func placeHelper()", "private func createHelper");
  const missingTarget = placeHelper.indexOf("if helperPanel != nil");
  const branch = placeHelper.slice(missingTarget, placeHelper.indexOf("return", missingTarget));
  const later = branch.indexOf('onEvent("later")');
  const close = branch.indexOf("close()");
  expect(later, "helper-loss branch must notify the CLI").toBeGreaterThanOrEqual(0);
  expect(close, "helper-loss branch must close the presenter").toBeGreaterThan(later);
});

test("all migrated guide panels use immediate AppKit window creation", () => {
  const present = functionBody("public func present()", "public func setState");
  const createHelper = functionBody("private func createHelper(frame: NSRect)", "private func positionArrow");
  const arrow = functionBody("let arrowPanel = PermissionHostArrowPanel(", "self.helperPanel = panel");
  expect(present).toContain("defer: false");
  expect(createHelper).toContain("defer: false");
  expect(arrow).toContain("defer: false");
});

test("drag start animates the arrow back to identity in normal motion", () => {
  const drag = functionBody("fileprivate func handleDragBegan()", "fileprivate func handleDragEnded");
  expect(drag).toContain("resetToIdentity()");
  expect(drag).toContain("animate(toScaleX: 1, scaleY: 1)");
});

test("Back honors Reduce Motion before attempting endpoint capture", () => {
  const back = functionBody("private func startBackFlight()", "private func finishBackFlight");
  const reduceMotion = back.indexOf("accessibilityDisplayShouldReduceMotion");
  const capture = back.indexOf("captureInitialEndpoint()");
  expect(reduceMotion, "Back must inspect Reduce Motion locally").toBeGreaterThanOrEqual(0);
  expect(capture, "Back must capture the card endpoint").toBeGreaterThan(reduceMotion);
});

test("invalid initial fitting size makes present fail closed", () => {
  const fit = functionBody("private func fitInitialPage()", "private func setInitialContent");
  const present = functionBody("public func present()", "public func setState");
  expect(fit).toMatch(/private func fitInitialPage\(\)\s*->\s*Bool/);
  expect(present).toMatch(/guard\s+fitInitialPage\(\)\s+else\s*\{[\s\S]*?return false/);
});

test("host errors preserve the existing localized error-page body", () => {
  const state = functionBody("public func setState", "public func close");
  const errorPage = state.slice(state.indexOf('case "error":'), state.indexOf("default:"));
  expect(errorPage).toContain('body: permissionHostString(copy, "errorBody")');
  expect(errorPage).not.toContain('body: message ??');
});
