import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const layoutSmokeSource = join(import.meta.dir, "native", "permission-views-layout-smoke.m");

test("initial permission page measures naturally with a bottom-trailing Skip overlay", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const start = source.indexOf("private struct PermissionInitialRoot:");
  const root = source.slice(start, source.indexOf("@MainActor", start));
  // Frozen root frame has width 600 and nil height; Skip has a 41pt frame,
  // followed by trailing padding, inside a bottomTrailing overlay.
  expect(root).toContain(".overlay(alignment: .bottomTrailing)");
  expect(root).toMatch(/\.frame\(height:\s*41(?:,\s*alignment:\s*\.center)?\)/);
  expect(root).toContain(".padding(.trailing, 57)");
  expect(root).not.toContain("minHeight: 312");
  expect(root).not.toContain(".padding(.bottom, 12.5)");
  expect(root).not.toContain("Spacer()");
});

test("Allow uses one live SwiftUI control style for the card and foreground snapshots", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const card = source.slice(source.indexOf("private struct PermissionCardRoot:"), source.indexOf("@objc(IncodexPermissionInitialView)"));
  const snapshot = source.slice(source.indexOf("public func snapshotPermissionCard(scale:"), source.indexOf("@objc public var preferredContentSize:"));
  expect(card).toContain(".buttonStyle(PermissionAllowButtonStyle(state: state))");
  expect(card).toContain("Text(state.allow)");
  expect(card).not.toContain(".buttonStyle(DefaultButtonStyle())");
  expect(card).toContain(".clipShape(Capsule(style: .continuous))");
  expect(card).toContain(".frame(minWidth: 62)");
  expect(snapshot).toContain("PermissionCardRoot(state: state).foreground");
  expect(snapshot).toContain("useHostingView: true");
  expect(snapshot).not.toContain("permissionHostCachedImage");
});

test("Allow draws the SwiftUI capsule at its target height and keeps natural label width", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const card = source.slice(source.indexOf("private struct PermissionCardRoot:"), source.indexOf("@objc(IncodexPermissionInitialView)"));
  const styleStart = source.indexOf("private struct PermissionAllowButtonStyle:");
  const style = source.slice(styleStart, source.indexOf("private struct PermissionCardRoot:", styleStart));
  const start = card.indexOf("Button { state.send(\"allow:\") }");
  const end = card.indexOf(".disabled(!state.allowEnabled)", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  expect(styleStart).toBeGreaterThanOrEqual(0);
  expect(style).toContain("configuration.label");
  expect(style).toContain(".padding(.horizontal, 12)");
  expect(style).toContain(".frame(height: 24)");
  expect(style).toContain("Capsule(style: .continuous)");
  expect(style).toContain("configuration.isPressed");
  expect(style).toContain("controlActiveState");
  expect(style).toContain("controlActiveState == .key");
  expect(style).toContain("isEnabled");
  expect(style).toContain("Color(nsColor: .systemFill)");
  expect(style).not.toContain("Color(nsColor: .tertiarySystemFill)");
  expect(style).toContain("guard isEnabled else { return Color(nsColor: .controlColor) }");
  expect(style).not.toContain(".frame(width:");
  const allow = card.slice(start, end);
  expect(allow).toContain("Text(state.allow)");
  expect(allow).not.toContain(".font(.system(size: 13))");
  expect(allow).toContain(".buttonStyle(PermissionAllowButtonStyle(state: state))");
  expect(allow).toContain(".keyboardShortcut(.defaultAction)");
  expect(allow).toContain(".clipShape(Capsule(style: .continuous))");
  expect(allow).toContain(".frame(minWidth: 62)");
  expect(allow).not.toContain(".frame(width:");
});

test("helper foreground composes bottom-aligned stacks and semantic padding", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const start = source.indexOf("private struct PermissionHelperForeground");
  const foreground = source.slice(start, source.indexOf("private struct PermissionHelperRoot", start));
  expect(foreground).toContain("HStack(alignment: .bottom, spacing: 16)");
  expect(foreground).toContain("VStack(alignment: .leading, spacing: 7)");
  expect(foreground).toContain(".padding(.leading, 18)");
  expect(foreground).toContain(".padding(.bottom, 27)");
  expect(foreground).toContain(".padding(.leading, 4)");
  expect(foreground).toContain(".padding(.trailing, 10)");
  expect(foreground).toContain(".padding(.bottom, 20)");
  expect(foreground).not.toContain(".offset(");
  expect(foreground).not.toContain("ZStack");
  expect(foreground).not.toContain("state.positionX");
});

test("helper hint lays out arrow and body text in a natural horizontal stack", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const start = source.indexOf("private struct PermissionHelperDragHint:");
  expect(start).toBeGreaterThanOrEqual(0);
  const hint = source.slice(start, source.indexOf("private struct PermissionHelperForeground", start));
  expect(hint).toContain("HStack(alignment: .center, spacing: 8)");
  expect(hint).toContain("Text(state.styledInstruction)");
  expect(hint).toContain(".font(.body)");
  expect(hint).not.toContain(".fixedSize");
  expect(hint).not.toContain(".frame(width: 408");
  expect(hint).not.toContain(".offset(");
});

test("live helper arrow uses the shared SwiftUI shape and native spring surface", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const start = source.indexOf("@objc(IncodexPermissionArrowView)");
  const end = source.indexOf("private struct PermissionHelperDragHint", start);
  const rootStart = source.indexOf("private struct PermissionLiveArrowRoot");
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  expect(rootStart).toBeGreaterThanOrEqual(0);
  const arrow = source.slice(rootStart, end);
  expect(arrow).toContain("private struct PermissionLiveArrowRoot: View");
  expect(arrow).toContain("PermissionSnapshotArrow()");
  expect(arrow).toContain(".interpolatingSpring(mass: 1, stiffness: 200, damping: 11, initialVelocity: 0)");
  expect(arrow).toContain("@objc(animateToScaleX:scaleY:)");
  expect(arrow).toContain("@objc(resetToIdentity)");
  expect(arrow).toContain("host.clipsToBounds = false");
  expect(arrow.indexOf(".scaleEffect")).toBeLessThan(arrow.indexOf(".shadow("));
  expect(source).not.toContain("CASpringAnimation");
});

test("permission display clock exposes a main-thread display-link surface with an explicit old-OS fallback", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const start = source.indexOf("@objc(IncodexPermissionDisplayLink)");
  expect(start).toBeGreaterThanOrEqual(0);
  const clock = source.slice(start, source.indexOf("private func permissionCopyString", start));
  expect(clock).toContain("window.displayLink(target: self, selector:");
  expect(clock).toContain("if #available(macOS 14.0, *)");
  expect(clock).toContain("displayLinked = false");
  expect(clock).toContain("invalidate()");
  expect(clock).toContain("handler:");
});

test("helper delegates outer edge treatment to its native window shell", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const start = source.indexOf("private struct PermissionHelperRoot:");
  const end = source.indexOf("@MainActor", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  const root = source.slice(start, end);
  expect(root).toContain(".background(.regularMaterial)");
  expect(root).not.toContain(".clipShape");
  expect(root).not.toContain(".stroke");
});

test("snapshot row retains the original separate SwiftUI fill and stroke shell", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const start = source.indexOf("private struct PermissionHelperSnapshotRow");
  const row = source.slice(start, source.indexOf("private struct PermissionHelperForeground", start));
  // SHA601 EBF D9C radius7 / GOT1012852C8 continuous / EBF EC8 linewidth1;
  // EC0020/0044 application row horizontal4, vertical5 padding.
  expect(row).toContain("RoundedRectangle(cornerRadius: 7, style: .continuous)");
  expect(row).toContain("lineWidth: 1");
  expect(row).toContain(".padding(.horizontal, 4)");
  expect(row).toContain(".padding(.vertical, 5)");
  expect(row).not.toContain("PermissionEmbeddedView");
  expect(row).not.toContain(".clipShape");
});

test.skipIf(process.platform !== "darwin").each(["current", "forced-hosting-compatibility"])("helper snapshot renders transparent foreground without windows (%s)", (mode) => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-helper-snapshot-"));
  try {
    let nativeSource = join(import.meta.dir, "..", "native/macos/permission-views.swift");
    if (mode === "forced-hosting-compatibility") {
      // Exercise the macOS 12 rendering path on the current OS without adding
      // a product switch or windows. This does not claim an actual macOS 12 run.
      const source = readFileSync(nativeSource, "utf8");
      const availability = "if #available(macOS 13.0, *), !useHostingView {";
      expect(source.split(availability)).toHaveLength(2);
      nativeSource = join(directory, "permission-views.swift");
      writeFileSync(nativeSource, source.replace(availability, "if #available(macOS 13.0, *), false {"));
    }
    const executable = join(directory, "snapshot");
    const architecture = process.arch === "arm64" ? "arm64" : "x86_64";
    const build = spawnSync("xcrun", [
      "swiftc", "-parse-as-library", "-target", `${architecture}-apple-macos12`,
      "-module-name", "IncodexHelperSnapshotTest",
      nativeSource, "tests/native/permission-helper-snapshot-smoke.swift",
      "-o", executable,
    ], { cwd: join(import.meta.dir, ".."), encoding: "utf8", timeout: 60_000 });
    expect(build.status, build.stderr || String(build.error ?? "snapshot compilation failed")).toBe(0);
    if (build.status !== 0) return;
    const run = spawnSync(executable, [], { encoding: "utf8", timeout: 20_000 });
    expect(run.status, run.stderr || String(run.error ?? "snapshot smoke failed")).toBe(0);
    expect(run.stdout).toContain("permission helper snapshot smoke passed");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 90_000);

test.skipIf(process.platform !== "darwin").each(["current", "forced-hosting-compatibility"])("permission card snapshot renders transparent foreground without windows (%s)", (mode) => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-permission-card-snapshot-"));
  try {
    let nativeSource = join(import.meta.dir, "..", "native/macos/permission-views.swift");
    if (mode === "forced-hosting-compatibility") {
      // Exercise the macOS 12 NSHostingView fallback on the current OS. This
      // is a test-only source copy; it does not add a product compatibility flag.
      const source = readFileSync(nativeSource, "utf8");
      const availability = "if #available(macOS 13.0, *), !useHostingView {";
      expect(source.split(availability)).toHaveLength(2);
      nativeSource = join(directory, "permission-views.swift");
      writeFileSync(nativeSource, source.replace(availability, "if #available(macOS 13.0, *), false {"));
    }
    const executable = join(directory, "permission-card-snapshot");
    const architecture = process.arch === "arm64" ? "arm64" : "x86_64";
    const build = spawnSync("xcrun", [
      "swiftc", "-parse-as-library", "-target", `${architecture}-apple-macos12`,
      "-module-name", "IncodexPermissionCardSnapshotTest",
      nativeSource, "tests/native/permission-card-snapshot-smoke.swift",
      "-o", executable,
    ], { cwd: join(import.meta.dir, ".."), encoding: "utf8", timeout: 60_000 });
    expect(build.status, build.stderr || String(build.error ?? "permission-card snapshot compilation failed")).toBe(0);
    if (build.status !== 0) return;
    const run = spawnSync(executable, [], { encoding: "utf8", timeout: 20_000 });
    expect(run.status, `${run.stdout ?? ""}${run.stderr ?? ""}` || String(run.error ?? "permission-card snapshot smoke failed")).toBe(0);
    expect(run.stdout).toContain("permission card snapshot smoke passed");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 90_000);

test.skipIf(process.platform !== "darwin")("flight images preserve native size, compositing and rounded clipping without windows", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-flight-pixels-"));
  try {
    const executable = join(directory, "flight-pixels");
    const architecture = process.arch === "arm64" ? "arm64" : "x86_64";
    const build = spawnSync("xcrun", [
      "swiftc", "-parse-as-library", "-target", `${architecture}-apple-macos12`,
      "-module-name", "IncodexFlightPixelsTest",
      "native/macos/permission-views.swift", "tests/native/permission-flight-pixels-smoke.swift",
      "-o", executable,
    ], { cwd: join(import.meta.dir, ".."), encoding: "utf8", timeout: 60_000 });
    expect(build.status, build.stderr || String(build.error ?? "flight pixels compilation failed")).toBe(0);
    if (build.status !== 0) return;
    const run = spawnSync(executable, [], { encoding: "utf8", timeout: 30_000 });
    expect(run.status, `${run.stdout ?? ""}${run.stderr ?? ""}` || String(run.error ?? "flight pixels smoke failed")).toBe(0);
    expect(run.stdout).toContain("permission flight pixels smoke passed (windowless;");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 100_000);

test("helper instruction uses the original semantic body font and matching native measurement", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const start = source.indexOf("Text(state.styledInstruction)");
  expect(start).toBeGreaterThanOrEqual(0);
  const end = source.indexOf("private struct PermissionHelperForeground", start);
  expect(end).toBeGreaterThan(start);
  const instruction = source.slice(start, end);
  // Original DragHintView: Font.body at 0x100EBD108, Text.font at
  // 0x100EBD120. A matching default 13pt value is not the same font API.
  expect(instruction).toContain(".font(.body)");
  expect(instruction).not.toContain(".font(.system(size: 13))");
  const measurement = source.slice(source.indexOf("var instructionHeight:"), source.indexOf("var extraHeight:"));
  expect(measurement).toContain("Text(styledInstruction)");
  expect(measurement).toContain(".font(.body)");
  expect(measurement).toContain("NSHostingView(rootView: label).fittingSize.height");
  expect(measurement).not.toContain("(instruction as NSString).boundingRect");
});

test.skipIf(process.platform !== "darwin")("native helper instruction preserves localized semantic runs without windows", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  expect(source).toContain("Text(state.styledInstruction)");
  const directory = mkdtempSync(join(tmpdir(), "incodex-instruction-"));
  try {
    const executable = join(directory, "instruction");
    const build = spawnSync("xcrun", [
      "swiftc", "-parse-as-library", "-module-name", "IncodexInstructionTest",
      "native/macos/permission-views.swift", "tests/native/permission-instruction-smoke.swift",
      "-o", executable,
    ], { cwd: join(import.meta.dir, ".."), encoding: "utf8", timeout: 60_000 });
    expect(build.status, build.stderr || String(build.error ?? "instruction compilation failed")).toBe(0);
    if (build.status !== 0) return;
    const run = spawnSync(executable, [], { encoding: "utf8", timeout: 20_000 });
    expect(run.status, run.stderr || String(run.error ?? "instruction smoke failed")).toBe(0);
    expect(run.stdout).toContain("semantic runs passed (no windows)");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 90_000);

test.skipIf(process.platform !== "darwin")("compiles the real placeholder hover reset smoke (runtime is opt-in)", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-placeholder-hover-"));
  try {
    const executable = join(directory, "placeholder-hover");
    const build = spawnSync("xcrun", [
      "swiftc", "-parse-as-library", "-module-name", "IncodexPlaceholderHoverTest",
      "native/macos/permission-views.swift", "tests/native/permission-placeholder-hover-smoke.swift",
      "-o", executable,
    ], { cwd: join(import.meta.dir, ".."), encoding: "utf8", timeout: 60_000 });
    expect(build.status, build.stderr || String(build.error ?? "hover smoke compilation failed")).toBe(0);
    if (build.status !== 0 || process.env.INCODEX_RUN_NATIVE_LAYOUT_SMOKE !== "1") return;
    const run = spawnSync(executable, [], { encoding: "utf8", timeout: 20_000 });
    const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
    expect(run.status, output || String(run.error ?? "hover reset smoke failed")).toBe(0);
    expect(output).toContain("placeholder hover reset smoke passed");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 90_000);

test("placeholder pressed style transforms the whole SwiftUI button, not only its outline", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const start = source.indexOf("private struct PermissionPlaceholderLabel");
  const end = source.indexOf("private func permissionPlaceholderText", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  const placeholder = source.slice(start, end);
  expect(placeholder).toContain(".scaleEffect(pressed ? 0.99 : 1)");
  expect(placeholder).toContain(".opacity(pressed ? 0.88 : 1)");
});

function permissionCardSource(): string {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const start = source.indexOf("private struct PermissionCardRoot");
  const end = source.indexOf("private struct PermissionPlaceholderLabel", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  return source.slice(start, end);
}

test("Back uses the original 13pt primary chevron and semantic tertiary system fill", () => {
  const source = readFileSync(join(import.meta.dir, "..", "native/macos/permission-views.swift"), "utf8");
  const start = source.indexOf('Image(systemName: "chevron.left")');
  expect(start).toBeGreaterThanOrEqual(0);
  const label = source.slice(start, source.indexOf('.buttonStyle(.plain)', start));
  // Original build1001067: label 0x100ec0680, font 0x100ec0730,
  // primary 0x100ec0768, tertiarySystemFillColor 0x100ec07b4.
  // These are source guards; hover/pressed pixels remain separate live gates.
  expect(label).toContain('.font(.system(size: 13, weight: .semibold))');
  expect(label).toContain('.foregroundStyle(.primary)');
  expect(label).toContain('.background(permissionBackFill, in: Circle())');
  expect(source).toContain('Color(nsColor: .tertiarySystemFill)');
  expect(source).toContain('if #available(macOS 14.0, *)');
});

test("PermissionCardRoot keeps title and description line limits unconstrained by the original source contract", () => {
  const card = permissionCardSource();
  // The shipped binary's PermissionRow title/description chains have no lineLimit(1)/(2).
  // This is a source-structure contract only; it does not claim long-text or pixel parity.
  expect(card).not.toContain(".lineLimit(1)");
  expect(card).not.toContain(".lineLimit(2)");
});

test("PermissionCardRoot states the original continuous Capsule shape on Allow", () => {
  const card = permissionCardSource();
  // The binary chain is Capsule(style: .continuous), not the implicit Capsule default.
  // This asserts source structure only, not runtime pixels.
  expect(card).toMatch(/\.clipShape\(\s*Capsule\(style:\s*\.continuous\)\s*\)/);
});

test("macOS permission host ABI is headless by default (Return UI is opt-in)", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-swiftui-test-"));
  try {
    const executable = join(directory, "permission-ui-test");
    const build = spawnSync("xcrun", [
      "swiftc", "-parse-as-library", "-module-name", "IncodexPermissionUITest",
      "native/macos/permission-views.swift", "tests/native/permission-views-smoke.swift",
      "-o", executable,
    ], { cwd: join(import.meta.dir, ".."), encoding: "utf8", timeout: 60_000 });
    expect(build.status, build.stderr || String(build.error ?? "Swift compilation failed")).toBe(0);
    const run = spawnSync(executable, [], { encoding: "utf8", timeout: 20_000 });
    expect(run.status, run.stderr || String(run.error ?? "Swift host smoke failed")).toBe(0);
    expect(run.stdout).toContain("permission SwiftUI host smoke passed");
    if (process.env.INCODEX_RUN_NATIVE_LAYOUT_SMOKE !== "1") {
      expect(run.stdout).toContain("headless; Return UI not run");
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 90_000);

test.skipIf(process.platform !== "darwin")(
  "compiles the P11 real AX title-layout smoke (runtime is opt-in)",
  () => {
    const directory = mkdtempSync(join(tmpdir(), "incodex-layout-smoke-"));
    try {
      const architecture = process.arch === "arm64" ? "arm64" : process.arch === "x64" ? "x86_64" : "";
      expect(architecture).not.toBe("");
      if (!architecture) return;

      const executable = join(directory, "permission-views-layout-smoke");
      const build = spawnSync("/usr/bin/clang", [
        "-arch", architecture,
        "-fobjc-arc",
        "-framework", "Cocoa",
        "-framework", "ApplicationServices",
        layoutSmokeSource,
        "-o", executable,
      ], { cwd: join(import.meta.dir, ".."), encoding: "utf8", timeout: 60_000 });
      const buildOutput = `${build.stdout ?? ""}${build.stderr ?? ""}`;
      expect(build.status, buildOutput || String(build.error ?? "P11 Objective-C compilation failed")).toBe(0);
      if (build.status !== 0 || process.env.INCODEX_RUN_NATIVE_LAYOUT_SMOKE !== "1") return;

      const dylibPath = join(import.meta.dir, "..", "native", "macos", "dist", "incodex-permission-ui.dylib");
      const run = spawnSync(executable, [dylibPath], {
        cwd: join(import.meta.dir, ".."),
        encoding: "utf8",
        timeout: 20_000,
      });
      const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
      expect(run.status, output || String(run.error ?? "P11 real AX layout smoke failed")).toBe(0);
      expect(output).toContain("P11_CHECK");
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
  90_000,
);

test.skipIf(process.platform !== "darwin")(
  "compiles the C03/C11 real AX permission-card geometry smoke (runtime is opt-in)",
  () => {
    const directory = mkdtempSync(join(tmpdir(), "incodex-card-layout-smoke-"));
    try {
      const architecture = process.arch === "arm64" ? "arm64" : process.arch === "x64" ? "x86_64" : "";
      expect(architecture).not.toBe("");
      if (!architecture) return;

      const executable = join(directory, "permission-views-layout-smoke");
      const build = spawnSync("/usr/bin/clang", [
        "-arch", architecture,
        "-fobjc-arc",
        "-framework", "Cocoa",
        "-framework", "ApplicationServices",
        layoutSmokeSource,
        "-o", executable,
      ], { cwd: join(import.meta.dir, ".."), encoding: "utf8", timeout: 60_000 });
      const buildOutput = `${build.stdout ?? ""}${build.stderr ?? ""}`;
      expect(build.status, buildOutput || String(build.error ?? "C03/C11 Objective-C compilation failed")).toBe(0);
      if (build.status !== 0 || process.env.INCODEX_RUN_NATIVE_LAYOUT_SMOKE !== "1") return;

      const dylibPath = join(import.meta.dir, "..", "native", "macos", "dist", "incodex-permission-ui.dylib");
      const run = spawnSync(executable, [dylibPath, "card"], {
        cwd: join(import.meta.dir, ".."),
        encoding: "utf8",
        timeout: 20_000,
      });
      const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
      expect(run.status, output || String(run.error ?? "C03/C11 real AX card smoke failed")).toBe(0);
      expect(output).toContain("C03_C11_CHECK");
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
  90_000,
);

test.skipIf(process.platform !== "darwin")(
  "compiles the real AX Back geometry and action smoke (runtime is opt-in)",
  () => {
    const directory = mkdtempSync(join(tmpdir(), "incodex-back-layout-smoke-"));
    try {
      const architecture = process.arch === "arm64" ? "arm64" : process.arch === "x64" ? "x86_64" : "";
      expect(architecture).not.toBe("");
      if (!architecture) return;

      const executable = join(directory, "permission-views-layout-smoke");
      const build = spawnSync("/usr/bin/clang", [
        "-arch", architecture,
        "-fobjc-arc",
        "-framework", "Cocoa",
        "-framework", "ApplicationServices",
        layoutSmokeSource,
        "-o", executable,
      ], { cwd: join(import.meta.dir, ".."), encoding: "utf8", timeout: 60_000 });
      const buildOutput = `${build.stdout ?? ""}${build.stderr ?? ""}`;
      expect(build.status, buildOutput || String(build.error ?? "Back Objective-C compilation failed")).toBe(0);
      if (build.status !== 0 || process.env.INCODEX_RUN_NATIVE_LAYOUT_SMOKE !== "1") return;

      const dylibPath = join(import.meta.dir, "..", "native", "macos", "dist", "incodex-permission-ui.dylib");
      const run = spawnSync(executable, [dylibPath, "back"], {
        cwd: join(import.meta.dir, ".."),
        encoding: "utf8",
        timeout: 20_000,
      });
      const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
      expect(run.status, output || String(run.error ?? "Back real AX smoke failed")).toBe(0);
      expect(output).toContain("BACK_CHECK");
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
  90_000,
);
