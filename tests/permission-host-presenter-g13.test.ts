import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const repositoryRoot = join(import.meta.dir, "..");
const viewsPath = join(repositoryRoot, "native", "macos", "permission-views.swift");
const presenterPath = join(repositoryRoot, "native", "macos", "permission-host-presenter.swift");

function isolatedPresenterSource(): string {
  let source = readFileSync(presenterPath, "utf8");
  source = source.replace(
    /@MainActor\nfunc permissionHostAppIcon\(\) -> NSImage\? \{[\s\S]*?\n\}/,
    "@MainActor\nfunc permissionHostAppIcon() -> NSImage? { NSImage(size: NSSize(width: 32, height: 32)) }",
  );
  source = source.replace(
    "let preferred = initialView.preferredContentSize",
    'let preferred = permissionHostInjectedPreferredSize("initial", initialView.preferredContentSize)',
  );
  source = source.replace(
    "let preferred = view.preferredContentSize",
    'let preferred = permissionHostInjectedPreferredSize("helper", view.preferredContentSize)',
  );
  source = source.replace("private func permissionHostFrame(", "func permissionHostFrame(");
  source = source.replace(
    "guard let screen = NSScreen.screens.first else { return nil }\n    let screenFrame = screen.frame",
    "let screenFrame = NSScreen.screens.first?.frame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)",
  );

  for (const expected of [
    "func permissionHostAppIcon() -> NSImage? { NSImage(size: NSSize(width: 32, height: 32)) }",
    'permissionHostInjectedPreferredSize("initial", initialView.preferredContentSize)',
    'permissionHostInjectedPreferredSize("helper", view.preferredContentSize)',
    "func permissionHostFrame(",
    "screenFrame.minY + screenFrame.height",
  ]) {
    if (!source.includes(expected)) throw new Error(`G13 isolated test seam was not applied: ${expected}`);
  }
  return source;
}

const harness = `
import AppKit
import Foundation

@MainActor var permissionHostInjectedPreferredSizes: [String: NSSize] = [:]
@MainActor var permissionHostInjectedPreferredSizeReads: [String: Int] = [:]
@MainActor func permissionHostInjectedPreferredSize(_ phase: String, _ actual: NSSize) -> NSSize {
    guard let injected = permissionHostInjectedPreferredSizes[phase] else { return actual }
    permissionHostInjectedPreferredSizeReads[phase, default: 0] += 1
    return injected
}

@MainActor
struct PermissionHostFlightEndpoint {
    let view: NSView
    let frame: NSRect
    let radius: CGFloat
    let image: NSImage?
    let captureImage: (() -> NSImage?)?

    init(view: NSView, frame: NSRect, radius: CGFloat, image: NSImage? = nil, captureImage: (() -> NSImage?)? = nil) {
        self.view = view
        self.frame = frame
        self.radius = radius
        self.image = image
        self.captureImage = captureImage
    }
}

@MainActor
final class PermissionHostFlight {
    init(source: PermissionHostFlightEndpoint, target: @escaping () -> PermissionHostFlightEndpoint, reverse: Bool = false, isClosed: @escaping () -> Bool, onComplete: @escaping () -> Void, onError: @escaping (String) -> Void) {}
    func start() {}
    func dispose() {}
}

@MainActor
struct PermissionHostSettingsFrame {
    let frame: NSRect
    let pid: pid_t
    let windowID: CGWindowID
}

@MainActor
final class SettingsLocator {
    static let bundleIdentifier = "com.apple.systempreferences"
    static var testFrame = NSRect(x: 240, y: 180, width: 840, height: 620)
    func locate() -> PermissionHostSettingsFrame? {
        PermissionHostSettingsFrame(frame: Self.testFrame, pid: 1, windowID: 1)
    }
    func prepareHandoff() {}
}

@MainActor
@main
enum PermissionHostPresenterG13Smoke {
    static func main() {
        if CommandLine.arguments.contains("--test-frames") {
            testFrames()
            print("G13 frame guards passed")
            return
        }
        if CommandLine.arguments.contains("--test-window-counts") {
            testFailureWindowCounts()
            print("G13 failure window counts passed")
            return
        }
        fatalError("expected --test-frames or --test-window-counts")
    }

    private static func testFrames() {
        let validTarget = NSRect(x: 240, y: 180, width: 840, height: 620)
        let validSize = NSSize(width: 531, height: 110)
        guard let valid = permissionHostFrame(validTarget, size: validSize) else { fatalError("valid geometry was rejected") }
        precondition(valid.origin.x.isFinite && valid.origin.y.isFinite)
        precondition(valid.width.isFinite && valid.width > 0)
        precondition(valid.height.isFinite && valid.height > 0)

        let invalidCases: [(String, NSRect, NSSize)] = [
            ("NaN target origin", NSRect(x: CGFloat.nan, y: 180, width: 840, height: 620), validSize),
            ("infinite target extent", NSRect(x: 240, y: 180, width: CGFloat.infinity, height: 620), validSize),
            ("zero target extent", NSRect(x: 240, y: 180, width: 0, height: 620), validSize),
            ("zero preferred width", validTarget, NSSize(width: 0, height: 110)),
            ("negative preferred height", validTarget, NSSize(width: 531, height: -1)),
            ("infinite preferred width", validTarget, NSSize(width: CGFloat.infinity, height: 110)),
        ]
        for (name, target, size) in invalidCases {
            precondition(permissionHostFrame(target, size: size) == nil, "\\(name) must return nil")
        }
    }

    private static func testFailureWindowCounts() {
        let application = NSApplication.shared
        guard application.setActivationPolicy(.accessory) else { fatalError("could not set accessory activation policy") }
        application.finishLaunching()
        let baselineVisibleWindowCount = application.windows.filter(\\.isVisible).count
        precondition(baselineVisibleWindowCount == 0, "test process unexpectedly started with a visible native window")
        var initialErrors: [String] = []
        permissionHostInjectedPreferredSizes["initial"] = NSSize(width: CGFloat.nan, height: 312)
        let invalidInitial = PermissionHostPresenter(
            copy: ["title": "G13 geometry test"] as NSDictionary,
            layoutDirection: "leftToRight",
            onEvent: { _ in },
            onError: { initialErrors.append($0) },
        )
        precondition(!invalidInitial.present(), "an invalid initial preferred size must fail presentation")
        precondition(permissionHostInjectedPreferredSizeReads["initial"] == 1, "initial invalid size injection was not consumed")
        precondition(initialErrors.contains(where: { $0.contains("initial view has invalid preferred size") }))
        precondition(application.windows.filter(\\.isVisible).count == 0, "initial size failure left a visible native window")

        permissionHostInjectedPreferredSizes.removeValue(forKey: "initial")
        permissionHostInjectedPreferredSizes["helper"] = NSSize(width: 531, height: 0)
        let validInitial = PermissionHostPresenter(
            copy: [
                "title": "G13 geometry test",
                "body": "Test only",
                "errorTitle": "Error",
                "errorBody": "Could not show the guide",
            ] as NSDictionary,
            layoutDirection: "leftToRight",
            onEvent: { _ in },
            onError: { _ in },
        )
        precondition(validInitial.present(), "valid initial preferred size should present")
        let initialVisibleWindowCount = application.windows.filter(\\.isVisible).count
        precondition(initialVisibleWindowCount == 1, "initial visible app count was \\(initialVisibleWindowCount)")
        validInitial.setState("awaiting-user", message: nil)
        let helperSizeInjectionConsumed = permissionHostInjectedPreferredSizeReads["helper"] == 1
        let stateMirror = Mirror(reflecting: validInitial)
        let helperFailureReachedErrorState = stateMirror.children.first(where: { $0.label == "state" })?.value as? String == "error"
        let helperFailureVisibleCount = application.windows.filter(\\.isVisible).count
        validInitial.close()
        let visibleWindowCountAfterClose = application.windows.filter(\\.isVisible).count
        precondition(helperSizeInjectionConsumed, "helper invalid preferred size was not consumed")
        precondition(helperFailureReachedErrorState, "helper invalid preferred size did not enter the error state")
        precondition(helperFailureVisibleCount == 1, "helper size failure left an extra visible app window")
        precondition(visibleWindowCountAfterClose == 0, "closing after helper size failure left a visible app window")
        print("G13_WINDOW_COUNTS initial-invalid=0 initial-valid=1 helper-invalid=1 closed=0")
    }
}
`;

test.skipIf(process.platform !== "darwin")(
  "guards native permission guide geometry and closes windows after invalid preferred sizes",
  () => {
    const directory = mkdtempSync(join(tmpdir(), "incodex-permission-host-g13-"));
    try {
      const presenter = join(directory, "permission-host-presenter-g13.swift");
      const smoke = join(directory, "permission-host-presenter-g13-smoke.swift");
      const executable = join(directory, "permission-host-presenter-g13-smoke");
      writeFileSync(presenter, isolatedPresenterSource());
      writeFileSync(smoke, harness);
      const architecture = process.arch === "arm64" ? "arm64" : process.arch === "x64" ? "x86_64" : "";
      expect(architecture).not.toBe("");
      const build = spawnSync("xcrun", [
        "swiftc",
        "-parse-as-library",
        "-target",
        `${architecture}-apple-macos12`,
        "-module-name",
        "IncodexPermissionHostPresenterG13Test",
        viewsPath,
        presenter,
        smoke,
        "-o",
        executable,
      ], { cwd: repositoryRoot, encoding: "utf8", timeout: 90_000 });
      const buildOutput = `${build.stdout ?? ""}${build.stderr ?? ""}`;
      expect(build.status, buildOutput || String(build.error ?? "Swift G13 presenter compilation failed")).toBe(0);
      expect(buildOutput).not.toContain("warning:");
      if (build.status !== 0) return;

      const geometry = spawnSync(executable, ["--test-frames"], { encoding: "utf8", timeout: 10_000 });
      const geometryOutput = `${geometry.stdout ?? ""}${geometry.stderr ?? ""}`;
      expect(geometry.status, geometryOutput || String(geometry.error ?? "G13 frame guard smoke failed")).toBe(0);
      expect(geometry.stdout).toContain("G13 frame guards passed");

      if (process.env.INCODEX_RUN_PERMISSION_HOST_G13_WINDOW_SMOKE === "1") {
        const windows = spawnSync(executable, ["--test-window-counts"], { cwd: repositoryRoot, encoding: "utf8", timeout: 20_000 });
        const windowOutput = `${windows.stdout ?? ""}${windows.stderr ?? ""}`;
        expect(windows.status, windowOutput || String(windows.error ?? "G13 native window count smoke failed")).toBe(0);
        expect(windows.stdout).toContain("G13_WINDOW_COUNTS initial-invalid=0 initial-valid=1 helper-invalid=1 closed=0");
        expect(windows.stdout).toContain("G13 failure window counts passed");
      }
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
  120_000,
);
