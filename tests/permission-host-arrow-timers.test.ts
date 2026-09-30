import { expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const native = join(root, "native", "macos");
const presenterPath = join(native, "permission-host-presenter.swift");
const presenterSource = readFileSync(presenterPath, "utf8");
const runVisible = process.env.INCODEX_RUN_PERMISSION_HOST_ARROW_TIMERS === "1";

function isolatedPresenterSource(): string {
  let source = presenterSource.replace(
    /@MainActor\nfunc permissionHostAppIcon\(\) -> NSImage\? \{[\s\S]*?\n\}/,
    "@MainActor\nfunc permissionHostAppIcon() -> NSImage? { NSImage(size: NSSize(width: 32, height: 32)) }",
  );
  for (const [from, to] of [
    ["fileprivate func handleArrowEntered()", "func handleArrowEntered()"],
    ["fileprivate func handleDragBegan()", "func handleDragBegan()"],
    ["fileprivate func handleDragEnded()", "func handleDragEnded()"],
    ["private func startBackFlight()", "func startBackFlight()"],
  ]) source = source.replace(from, to);

  for (const expected of [
    "func permissionHostAppIcon() -> NSImage? { NSImage(size: NSSize(width: 32, height: 32)) }",
    "func handleArrowEntered()",
    "func handleDragBegan()",
    "func handleDragEnded()",
    "func startBackFlight()",
  ]) {
    if (!source.includes(expected)) throw new Error(`timer test seam was not applied: ${expected}`);
  }
  return source;
}

const harness = `
import AppKit
import CoreGraphics
import Foundation

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
    private let reverse: Bool
    private let onComplete: () -> Void
    init(source: PermissionHostFlightEndpoint, target: @escaping () -> PermissionHostFlightEndpoint, reverse: Bool = false, isClosed: @escaping () -> Bool, onComplete: @escaping () -> Void, onError: @escaping (String) -> Void) {
        self.reverse = reverse
        self.onComplete = onComplete
    }
    // Only the external forward animation is replaced. Completing it here
    // exercises the presenter's real finishForwardFlight -> revealHelper path.
    func start() { if !reverse { onComplete() } }
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
    static let bundleIdentifier = "com.example.incodex.permission-arrow-timers-test"
    func locate() -> PermissionHostSettingsFrame? {
        PermissionHostSettingsFrame(frame: NSRect(x: 240, y: 180, width: 840, height: 620), pid: 1, windowID: 1)
    }
    func prepareHandoff() {}
}

@MainActor
@main
struct PermissionHostArrowTimersSmoke {
    private static let copy: NSDictionary = [
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
    ]

    static func main() {
        do { try run() }
        catch {
            FileHandle.standardError.write(Data(("arrow timer smoke failed: " + String(describing: error) + "\\n").utf8))
            exit(1)
        }
    }

    private static func run() throws {
        let application = NSApplication.shared
        guard application.setActivationPolicy(.accessory) else { throw SmokeFailure("could not set accessory policy") }
        application.finishLaunching()
        guard visibleWindowCount() == 0 else { throw SmokeFailure("test process started with visible windows") }
        guard !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion else {
            throw SmokeFailure("Reduce Motion is enabled; this timer run requires normal-motion system state")
        }

        try verifyPulseAndDragQueue()
        try verifyErrorCancelsReturnTimer()
        try verifyCloseCancelsFirstPulse()
        guard visibleWindowCount() == 0, application.activationPolicy() == .accessory else {
            throw SmokeFailure("test-owned windows remained visible or activation policy was not restored")
        }
        print("B04_B05_ARROW_TIMERS first=500ms return=250ms idle=4000ms dragEnd=4000ms back=cancel error=cancel close=cancel")
    }

    private static func verifyPulseAndDragQueue() throws {
        let presenter = try revealGuide()
        defer { presenter.close() }
        try requireTimer("arrowTimer", interval: 0.5, in: presenter, label: "first pulse after reveal")
        guard (stored("arrowPanel", in: presenter) as? NSPanel)?.isVisible == true else {
            throw SmokeFailure("first 500 ms timer was scheduled before the arrow window became visible")
        }

        try waitUntil("stretch return timer", timeout: 1) {
            stored("arrowReturnTimer", in: presenter) is Timer
        }
        try requireTimer("arrowReturnTimer", interval: 0.25, in: presenter, label: "stretch return")
        guard stored("arrowTimer", in: presenter) == nil else {
            throw SmokeFailure("stretch left the previous idle timer scheduled")
        }

        try waitUntil("identity return and next idle timer", timeout: 1) {
            stored("arrowTimer", in: presenter) is Timer && stored("arrowReturnTimer", in: presenter) == nil
        }
        try requireTimer("arrowTimer", interval: 4, in: presenter, label: "next pulse after return command")

        presenter.handleDragBegan()
        try requireNoArrowTimers(in: presenter, label: "drag begin")
        presenter.handleDragEnded()
        try requireTimer("arrowTimer", interval: 4, in: presenter, label: "drag end")
        guard stored("arrowReturnTimer", in: presenter) == nil else {
            throw SmokeFailure("drag end scheduled a return timer")
        }
        presenter.startBackFlight()
        try requireNoArrowTimers(in: presenter, label: "Back")
        guard let arrow = stored("arrowPanel", in: presenter) as? NSPanel,
              arrow.alphaValue == 0, !arrow.isVisible else {
            throw SmokeFailure("Back did not hide and order out the live arrow")
        }
    }

    private static func verifyErrorCancelsReturnTimer() throws {
        let presenter = try revealGuide()
        defer { presenter.close() }
        presenter.handleArrowEntered()
        try requireTimer("arrowReturnTimer", interval: 0.25, in: presenter, label: "pre-error return")
        presenter.setState("error")
        try requireNoArrowTimers(in: presenter, label: "error")
        guard stored("helperPanel", in: presenter) == nil,
              stored("arrowPanel", in: presenter) == nil,
              (stored("initialPanel", in: presenter) as? NSWindow)?.isVisible == true else {
            throw SmokeFailure("error did not retire the helper and arrow while retaining the guide")
        }
    }

    private static func verifyCloseCancelsFirstPulse() throws {
        let presenter = try revealGuide()
        try requireTimer("arrowTimer", interval: 0.5, in: presenter, label: "pre-close first pulse")
        presenter.close()
        try requireNoArrowTimers(in: presenter, label: "close")
        guard visibleWindowCount() == 0 else { throw SmokeFailure("close left a visible test window") }
    }

    private static func revealGuide() throws -> PermissionHostPresenter {
        var errors: [String] = []
        let presenter = PermissionHostPresenter(copy: copy, layoutDirection: "leftToRight", onEvent: { _ in }, onError: { errors.append($0) })
        do {
            guard presenter.present() else { throw SmokeFailure("initial guide did not present") }
            guard let initialView = stored("initialView", in: presenter),
                  let viewState = stored("state", in: initialView),
                  let actionTarget = stored("actionTarget", in: viewState) as? NSObject,
                  NSApplication.shared.sendAction(NSSelectorFromString("allow:"), to: actionTarget, from: nil) else {
                throw SmokeFailure("native Allow action did not capture the starting card")
            }
            presenter.setState("repairing")
            presenter.setState("awaiting-user")
            // Read the scheduled timer before pumping the run loop; otherwise
            // the real 500 ms callback may fire while waiting for visibility.
            guard stored("arrowTimer", in: presenter) is Timer else {
                let state = String(describing: stored("state", in: presenter))
                let presented = String(describing: stored("presented", in: presenter))
                let closed = String(describing: stored("closed", in: presenter))
                throw SmokeFailure("first pulse timer was absent; presenterState=" + state + " presented=" + presented + " closed=" + closed + " errors=" + errors.joined(separator: ";"))
            }
            try requireTimer("arrowTimer", interval: 0.5, in: presenter, label: "first pulse after forward reveal")
            guard (stored("helperPanel", in: presenter) as? NSPanel)?.isVisible == true,
                  (stored("arrowPanel", in: presenter) as? NSPanel)?.isVisible == true else {
                throw SmokeFailure("forward completion did not synchronously reveal the helper and arrow")
            }
            return presenter
        } catch {
            presenter.close()
            throw error
        }
    }

    private static func requireTimer(_ name: String, interval: TimeInterval, in presenter: PermissionHostPresenter, label: String) throws {
        guard let timer = stored(name, in: presenter) as? Timer,
              timer.isValid else {
            throw SmokeFailure(label + " did not hold a valid scheduled timer")
        }
        let remaining = timer.fireDate.timeIntervalSinceNow
        let tolerance = min(0.1, interval / 4)
        guard remaining > interval - tolerance, remaining <= interval + 0.02 else {
            throw SmokeFailure(label + " timer fire deadline was " + String(remaining) + " seconds away; expected about " + String(interval))
        }
    }

    private static func requireNoArrowTimers(in presenter: PermissionHostPresenter, label: String) throws {
        guard stored("arrowTimer", in: presenter) == nil,
              stored("arrowReturnTimer", in: presenter) == nil else {
            throw SmokeFailure(label + " left an arrow timer active")
        }
    }

    private static func waitUntil(_ label: String, timeout: TimeInterval, condition: () -> Bool) throws {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() { return }
            RunLoop.current.run(until: Date().addingTimeInterval(0.01))
        }
        if !condition() { throw SmokeFailure("timed out waiting for " + label) }
    }

    private static func visibleWindowCount() -> Int { NSApplication.shared.windows.filter(\\.isVisible).count }

    private static func stored(_ name: String, in object: Any) -> Any? {
        var mirror: Mirror? = Mirror(reflecting: object)
        while let current = mirror {
            if let value = current.children.first(where: { $0.label == name })?.value { return unwrapped(value) }
            mirror = current.superclassMirror
        }
        return nil
    }

    private static func unwrapped(_ value: Any) -> Any? {
        var value = value
        while Mirror(reflecting: value).displayStyle == .optional {
            guard let child = Mirror(reflecting: value).children.first else { return nil }
            value = child.value
        }
        return value
    }
}

private struct SmokeFailure: Error, CustomStringConvertible {
    let message: String
    init(_ message: String) { self.message = message }
    var description: String { message }
}
`;

function buildSmoke(): { executable: string; directory: string } {
  const directory = mkdtempSync(join(tmpdir(), "incodex-permission-arrow-timers-"));
  const isolatedPresenter = join(directory, "permission-host-presenter.swift");
  const smoke = join(directory, "permission-host-arrow-timers-smoke.swift");
  const executable = join(directory, "permission-host-arrow-timers-smoke");
  writeFileSync(isolatedPresenter, isolatedPresenterSource());
  writeFileSync(smoke, harness);
  const architecture = process.arch === "arm64" ? "arm64" : "x86_64";
  const build = spawnSync("xcrun", [
    "swiftc", "-parse-as-library", "-target", `${architecture}-apple-macos12.0`,
    "-module-name", "IncodexPermissionHostArrowTimersSmoke",
    join(native, "permission-views.swift"), isolatedPresenter, smoke,
    "-framework", "ApplicationServices", "-framework", "CoreGraphics", "-o", executable,
  ], { cwd: root, encoding: "utf8", timeout: 90_000 });
  const output = `${build.stdout ?? ""}${build.stderr ?? ""}`;
  expect(build.status, output || String(build.error ?? "arrow timer native smoke failed to compile")).toBe(0);
  return { executable, directory };
}

test("B04/B05 presenter source orders and cancels the live timer queue", () => {
  const reveal = presenterSource.split("private func revealHelper() {")[1]?.split("private func startForwardFlight")[0] ?? "";
  const pulse = presenterSource.split("private func stretchArrow() {")[1]?.split("private func stopArrow()")[0] ?? "";
  const dragStart = presenterSource.split("fileprivate func handleDragBegan() {")[1]?.split("fileprivate func handleDragEnded")[0] ?? "";
  const dragEnd = presenterSource.split("fileprivate func handleDragEnded() {")[1]?.split("fileprivate func recordDragSession")[0] ?? "";
  const back = presenterSource.split("private func startBackFlight() {")[1]?.split("private func finishBackFlight")[0] ?? "";
  const close = presenterSource.split("public func close() {")[1]?.split("fileprivate func handleAllow")[0] ?? "";
  const error = presenterSource.split('case "error":')[1]?.split("default:")[0] ?? "";
  const disposeHelper = presenterSource.split("private func disposeHelper() {")[1]?.split("private func stopBackFlightTimer")[0] ?? "";

  expect(reveal).toContain("helperPanel?.orderFrontRegardless()");
  expect(reveal).toContain("arrowPanel?.orderFront(nil)");
  expect(reveal.indexOf("arrowPanel?.orderFront(nil)")).toBeLessThan(reveal.indexOf("scheduleArrow(after: 0.5)"));
  expect(pulse).toContain("arrowReturnTimer = Timer.scheduledTimer(withTimeInterval: 0.25");
  expect(pulse).toContain("arrowView?.animate(toScaleX: 1.15, scaleY: 1.6)");
  expect(pulse).toContain("self.arrowView?.animate(toScaleX: 1, scaleY: 1)");
  expect(pulse).toContain("self.scheduleArrow(after: 4)");
  expect(dragStart).toContain("stopArrow()");
  expect(dragEnd).toContain("scheduleArrow(after: 4)");
  expect(back).toContain("stopArrow()");
  expect(error).toContain("disposeHelper()");
  expect(disposeHelper).toContain("stopArrow()");
  expect(close).toContain("stopArrow()");
});

test.skipIf(process.platform !== "darwin")(
  "B04/B05 current Swift presenter schedules and cancels real arrow timers",
  () => {
    const built = buildSmoke();
    try {
      expect(built.executable.length).toBeGreaterThan(0);
    } finally {
      rmSync(built.directory, { recursive: true, force: true });
    }
  },
  120_000,
);

test.skipIf(process.platform !== "darwin" || !runVisible)(
  "B04/B05 current Swift presenter runs its real arrow timer queue in isolated test windows",
  () => {
    const built = buildSmoke();
    try {
      const run = spawnSync(built.executable, [], { cwd: root, encoding: "utf8", timeout: 25_000 });
      const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
      expect(run.status, output || String(run.error ?? "arrow timer visible smoke failed")).toBe(0);
      expect(output).toContain("B04_B05_ARROW_TIMERS first=500ms return=250ms idle=4000ms dragEnd=4000ms back=cancel error=cancel close=cancel");
    } finally {
      rmSync(built.directory, { recursive: true, force: true });
    }
  },
  30_000,
);
