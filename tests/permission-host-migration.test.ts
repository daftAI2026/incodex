/**
 * [INPUT]: 依赖原生权限源码、构建摘要与隔离测试夹具。
 * [OUTPUT]: 验证共享权限卡的源码和分发边界。
 * [POS]: 双行接入的既有合同回归；不申请真实权限。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { readPermissionPresenterSource, swiftDeclarationBody } from "./permission-swift-source.ts";
import { join } from "node:path";

const source = readPermissionPresenterSource(join(import.meta.dir, ".."));

function functionBody(signature: string, _nextSignature: string): string {
  return swiftDeclarationBody(source, signature);
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
  expect(fit).toMatch(/func fitInitialPage\(\)\s*->\s*Bool/);
  expect(present).toMatch(/guard\s+fitInitialPage\(\)\s+else\s*\{[\s\S]*?return false/);
});

test("host errors preserve the existing localized error-page body", () => {
  const state = functionBody("public func setState", "public func close");
  const errorPage = state.slice(state.indexOf('case "error":'), state.indexOf("default:"));
  expect(errorPage).toContain('body: permissionHostString(copy, "errorBody")');
  expect(errorPage).not.toContain('body: message ??');
});
