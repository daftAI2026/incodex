import { expect, test } from "bun:test";
import { readFileSync, rmSync, mkdtempSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

function isolatedViewsSource(): string {
  const path = join(root, "native", "macos", "permission-views.swift");
  let source = readFileSync(path, "utf8");
  const setter = "state.setContent(title: title, body: body, allowEnabled: allowEnabled, settingsPlaceholder: settingsPlaceholder)";
  if (!source.includes(setter)) throw new Error("G10 view content instrumentation seam was not found");
  source = source.replace(setter, `${setter}\n        permissionHostG10Content = (title, body, allowEnabled, settingsPlaceholder)`);
  return source;
}

function isolatedPresenterSource(): string {
  const path = join(root, "native", "macos", "permission-host-presenter.swift");
  let source = readFileSync(path, "utf8");
  source = source.replace(
    /@MainActor\nfunc permissionHostAppIcon\(\) -> NSImage\? \{[\s\S]*?\n\}/,
    "@MainActor\nfunc permissionHostAppIcon() -> NSImage? { NSImage(size: NSSize(width: 32, height: 32)) }",
  );
  source = source.replace(
    "guard let screen = NSScreen.screens.first else { return nil }\n    let screenFrame = screen.frame",
    "let screenFrame = NSScreen.screens.first?.frame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)",
  );
  const endpoint = / {4}private func captureInitialEndpoint\(\) -> PermissionHostFlightEndpoint\? \{[\s\S]*?\n {4}\}\n\n {4}private func helperEndpoint/;
  if (!endpoint.test(source)) throw new Error("G10 endpoint injection seam was not found");
  source = source.replace(endpoint, `    private func captureInitialEndpoint() -> PermissionHostFlightEndpoint? {
        guard let cardView else { return nil }
        return PermissionHostFlightEndpoint(
            view: cardView,
            frame: NSRect(x: 20, y: 30, width: 518, height: 80),
            radius: 24,
            image: NSImage(size: NSSize(width: 518, height: 80)),
        )
    }

    private func helperEndpoint`);

  // Keep AppKit objects real while ensuring this regression never orders a
  // window onto the user's desktop.
  for (const [before, after] of [
    ["panel.makeKeyAndOrderFront(nil)", "// hidden-window G10 smoke"],
    ["initialPanel?.orderFront(nil)", "// hidden-window G10 smoke"],
    ["helperPanel?.orderFrontRegardless()", "// hidden-window G10 smoke"],
    ["arrowPanel?.orderFront(nil)", "// hidden-window G10 smoke"],
    ["initialPanel?.makeKeyAndOrderFront(nil)", "// hidden-window G10 smoke"],
  ]) {
    if (!source.includes(before)) throw new Error(`G10 window suppression seam was not found: ${before}`);
    source = source.replaceAll(before, after);
  }
  const timeout = "Timer.scheduledTimer(withTimeInterval: 5, repeats: false)";
  if (!source.includes(timeout)) throw new Error("G10 back-flight timeout seam was not found");
  source = source.replace(timeout, "Timer.scheduledTimer(withTimeInterval: 0.02, repeats: false)");
  return source;
}

const harness = `
import AppKit
import Foundation

@MainActor var permissionHostG10Content: (String, String, Bool, Bool)?

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
    static var reverseBehavior = "failure"
    private let reverse: Bool
    private let onComplete: () -> Void
    private let onError: (String) -> Void
    private var disposed = false

    init(source: PermissionHostFlightEndpoint, target: @escaping () -> PermissionHostFlightEndpoint, reverse: Bool = false, isClosed: @escaping () -> Bool, onComplete: @escaping () -> Void, onError: @escaping (String) -> Void) {
        self.reverse = reverse
        self.onComplete = onComplete
        self.onError = onError
    }

    func start() {
        guard reverse else { dispose(); return }
        if Self.reverseBehavior == "failure" {
            onError("injected handoff failure")
            dispose()
        }
    }

    func dispose() {
        guard !disposed else { return }
        disposed = true
        onComplete()
    }
}

@MainActor
struct PermissionHostSettingsFrame {
    let frame: NSRect
    let pid: pid_t
    let windowID: CGWindowID
}

@MainActor
final class SettingsLocator {
    static let bundleIdentifier = "com.incodex.G10.SettingsFixture"
    func locate() -> PermissionHostSettingsFrame? {
        PermissionHostSettingsFrame(frame: NSRect(x: 240, y: 180, width: 840, height: 620), pid: 1, windowID: 1)
    }
    func prepareHandoff() {}
}

@MainActor
@main
enum PermissionHostG10HandoffSmoke {
    static func main() {
        do { try run() }
        catch {
            FileHandle.standardError.write(Data("G10 handoff smoke failed: \\(error)\\n".utf8))
            exit(1)
        }
    }

    private static func run() throws {
        guard CommandLine.arguments.count == 2 else { throw SmokeFailure("expected failure or timeout") }
        PermissionHostFlight.reverseBehavior = CommandLine.arguments[1]
        let application = NSApplication.shared
        guard application.setActivationPolicy(.accessory) else { throw SmokeFailure("could not set accessory policy") }
        application.finishLaunching()
        guard visibleWindows() == 0 else { throw SmokeFailure("G10 smoke started with a visible window") }
        var events: [String] = []
        var errors: [String] = []
        let presenter = PermissionHostPresenter(
            copy: [
                "title": "Enable ChatGPT scripting",
                "body": "Allow ChatGPT to access Accessibility.",
                "permissionTitle": "Accessibility",
                "permissionDescription": "Read and control app interfaces",
                "repair": "Allow",
                "later": "Skip",
                "back": "Back",
                "addedTitle": "ChatGPT added",
                "addedBody": "ChatGPT is in the list.",
                "dragInstruction": "Drag ChatGPT into the app list above.",
                "completeInSettings": "Complete in Settings",
                "checking": "Checking",
                "repairing": "Preparing System Settings",
                "openSettings": "Open Settings",
                "errorTitle": "Permission needs attention",
                "errorBody": "The permission guide could not continue.",
            ] as NSDictionary,
            layoutDirection: "leftToRight",
            onEvent: { events.append($0) },
            onError: { errors.append($0) },
        )
        guard presenter.present() else { throw SmokeFailure("initial page did not present") }
        try sendAction("allow:", to: presenter)
        guard events == ["allow"] else { throw SmokeFailure("initial Allow did not settle") }
        presenter.setState("repairing")
        presenter.setState("awaiting-user")
        guard stored("helperPanel", in: presenter) is NSPanel else { throw SmokeFailure("Settings helper was not created") }
        try sendAction("later:", to: presenter)

        if PermissionHostFlight.reverseBehavior == "timeout" {
            let deadline = Date().addingTimeInterval(1)
            while stateString("state", in: presenter) != "pending" && Date() < deadline {
                RunLoop.current.run(until: Date().addingTimeInterval(0.01))
            }
        }

        guard stateString("state", in: presenter) == "pending" else { throw SmokeFailure("back handoff did not restore pending initial state") }
        guard stateBool("retryReady", in: presenter) == true else { throw SmokeFailure("initial Allow was not enabled for retry") }
        guard stateBool("returning", in: presenter) == false else { throw SmokeFailure("presenter remained in the return transition") }
        guard stored("helperPanel", in: presenter) == nil, stored("arrowPanel", in: presenter) == nil else {
            throw SmokeFailure("failed handoff left helper or arrow windows alive")
        }
        guard let content = permissionHostG10Content,
              content.2 == true, content.3 == false else {
            throw SmokeFailure("recovered initial page did not enable Allow and restore the permission card")
        }
        guard visibleWindows() == 0 else { throw SmokeFailure("G10 smoke ordered a window onscreen") }
        try sendAction("allow:", to: presenter)
        guard events == ["allow", "retry"] else { throw SmokeFailure("recovered initial Allow did not dispatch Retry") }
        guard errors.isEmpty else { throw SmokeFailure("handoff fallback reported a fatal host error: \\(errors)") }
        presenter.close()
        guard visibleWindows() == 0 else { throw SmokeFailure("closing hidden smoke left a visible window") }
        print("G10_HANDOFF outcome=\\(PermissionHostFlight.reverseBehavior) initial=interactive helper=zero retry=dispatched visibleWindows=0")
    }

    private static func sendAction(_ selectorName: String, to presenter: PermissionHostPresenter) throws {
        guard let initialView = stored("initialView", in: presenter),
              let viewState = stored("state", in: initialView),
              let target = stored("actionTarget", in: viewState) as? NSObject else {
            throw SmokeFailure("native SwiftUI action target is unavailable")
        }
        let selector = Selector(selectorName)
        guard target.responds(to: selector), NSApplication.shared.sendAction(selector, to: target, from: nil) else {
            throw SmokeFailure("native action did not dispatch: \\(selectorName)")
        }
    }

    private static func stored(_ key: String, in value: Any) -> Any? {
        var mirror: Mirror? = Mirror(reflecting: value)
        while let current = mirror {
            if let field = current.children.first(where: { $0.label == key })?.value {
                var unwrapped = field
                while Mirror(reflecting: unwrapped).displayStyle == .optional {
                    guard let child = Mirror(reflecting: unwrapped).children.first else { return nil }
                    unwrapped = child.value
                }
                return unwrapped
            }
            mirror = current.superclassMirror
        }
        return nil
    }

    private static func stateString(_ key: String, in presenter: PermissionHostPresenter) -> String? {
        stored(key, in: presenter) as? String
    }

    private static func stateBool(_ key: String, in presenter: PermissionHostPresenter) -> Bool? {
        stored(key, in: presenter) as? Bool
    }

    private static func visibleWindows() -> Int {
        NSApplication.shared.windows.filter(\\.isVisible).count
    }
}

private struct SmokeFailure: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}
`;

test.skipIf(process.platform !== "darwin")("G10 Swift guide handoff failure and timeout restore a retryable initial page without showing windows", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-permission-host-g10-"));
  try {
    const presenter = join(directory, "permission-host-presenter-g10.swift");
    const views = join(directory, "permission-views-g10.swift");
    const smoke = join(directory, "permission-host-g10-smoke.swift");
    const executable = join(directory, "permission-host-g10-smoke");
    writeFileSync(presenter, isolatedPresenterSource());
    writeFileSync(views, isolatedViewsSource());
    writeFileSync(smoke, harness);
    const architecture = process.arch === "arm64" ? "arm64" : "x86_64";
    const build = spawnSync("xcrun", ["swiftc", "-parse-as-library", "-target", `${architecture}-apple-macos12`,
      "-module-name", "IncodexPermissionHostG10Test", views, presenter, smoke, "-o", executable],
    { cwd: root, encoding: "utf8", timeout: 90_000 });
    const buildOutput = `${build.stdout ?? ""}${build.stderr ?? ""}`;
    expect(build.status, buildOutput || String(build.error ?? "Swift G10 handoff smoke failed to compile")).toBe(0);
    expect(buildOutput).not.toContain("warning:");
    if (build.status !== 0) return;

    for (const outcome of ["failure", "timeout"]) {
      const run = spawnSync(executable, [outcome], { cwd: root, encoding: "utf8", timeout: 15_000 });
      const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
      expect(run.status, output || String(run.error ?? `G10 ${outcome} recovery failed`)).toBe(0);
      expect(run.stdout).toContain(`G10_HANDOFF outcome=${outcome} initial=interactive helper=zero retry=dispatched visibleWindows=0`);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 120_000);
