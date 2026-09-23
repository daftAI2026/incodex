import AppKit
import SwiftUI

private func isHostingView(_ view: NSView) -> Bool {
    var candidate: AnyClass? = type(of: view)
    while let current = candidate {
        if NSStringFromClass(current).contains("NSHostingView") { return true }
        candidate = class_getSuperclass(current)
    }
    return false
}

@MainActor
private func laidOutCardFrame(_ view: IncodexPermissionInitialView) -> NSRect {
    // Match the production fittingSize -> setContentSize handoff before
    // inspecting child frames; leaving the constructor's 340pt frame would
    // center two natural heights in a stale proposal and halve the delta.
    view.setFrameSize(view.preferredContentSize)
    view.layoutSubtreeIfNeeded()
    for subview in view.subviews {
        subview.layoutSubtreeIfNeeded()
    }
    let card = view.permissionCardView
    card.layoutSubtreeIfNeeded()
    precondition(card.superview != nil, "permission card must be embedded in the live SwiftUI host")
    return card.convert(card.bounds, to: view)
}

@MainActor
private final class PermissionActionSink: NSObject {
    var allowCount = 0
    @objc func allow(_ sender: Any?) { allowCount += 1 }
}

@main
enum PermissionViewsSmoke {
    @MainActor
    static func main() {
        _ = NSApplication.shared
        let view = IncodexPermissionFlightView(frame: NSRect(x: 0, y: 0, width: 518, height: 80))
        precondition(NSStringFromClass(type(of: view)) == "IncodexPermissionFlightView")
        precondition(view.subviews.count == 1)
        let host = view.subviews[0]
        precondition(isHostingView(host))
        let source = NSImage(size: NSSize(width: 518, height: 80))
        let target = NSImage(size: NSSize(width: 531, height: 110))
        view.setSourceImage(source, targetImage: target)
        for (progress, radius) in [(0.0, 24.0), (0.5, 18.0), (1.0, 12.0)] {
            view.updateProgress(progress, cornerRadius: radius, reduceTransparency: false)
            view.setFrameSize(NSSize(width: 526, height: 100))
            view.layoutSubtreeIfNeeded()
            precondition(view.subviews[0] === host, "keep one SwiftUI host across display ticks")
            precondition(host.frame == view.bounds)
            precondition(view.flightProgress == progress)
            precondition(view.flightCornerRadius == radius)
            precondition(source.size == NSSize(width: 518, height: 80))
            precondition(target.size == NSSize(width: 531, height: 110))
        }
        view.updateProgress(-1, cornerRadius: -1, reduceTransparency: true)
        precondition(view.flightProgress == 0 && view.flightCornerRadius == 0)
        view.updateProgress(2, cornerRadius: 12, reduceTransparency: false)
        precondition(view.flightProgress == 1)
        view.updateProgress(.nan, cornerRadius: .infinity, reduceTransparency: false)
        precondition(view.flightProgress == 1 && view.flightCornerRadius == 12)
        precondition(view.hitTest(NSPoint(x: 50, y: 30)) == nil)
        view.setSourceImage(nil, targetImage: nil)
        let copy: NSDictionary = [
            "title": "Enable ChatGPT scripting", "body": "Allow Accessibility access.",
            "permissionTitle": "Accessibility", "permissionDescription": "Read and control app interfaces",
            "repair": "Allow", "later": "Skip", "completeInSettings": "Complete in Settings",
            "back": "Back", "dragInstruction": "Drag ChatGPT into the app list above.",
        ]
        let initial = IncodexPermissionInitialView(frame: NSRect(x: 0, y: 0, width: 600, height: 340))
        initial.configure(copy: copy, appIcon: source, permissionIcon: target, actionTarget: nil)
        let initialHost = initial.subviews[0]
        precondition(isHostingView(initialHost))
        precondition(isHostingView(initial.permissionCardView))
        precondition(initial.preferredContentSize.width == 600)
        precondition(initial.preferredContentSize.height > 0)

        // The reference measures the body text naturally. A one-line localized
        // body therefore moves the card up by one body line (18pt), while the
        // natural window grows and the bottom-anchored Skip gap stays stable.
        // Keep this headless: these are two real NSHostingView trees, not a
        // source-string or mocked layout assertion.
        let shortCopy: NSDictionary = [
            "title": "Enable ChatGPT scripting", "body": "允许 ChatGPT 访问辅助功能。",
            "permissionTitle": "Accessibility", "permissionDescription": "Read and control app interfaces",
            "repair": "Allow", "later": "Skip", "completeInSettings": "Complete in Settings",
            "back": "Back", "dragInstruction": "Drag ChatGPT into the app list above",
        ]
        let longCopy: NSDictionary = [
            "title": "Enable ChatGPT scripting", "body": "Allow Accessibility access.\nThis second line is intentional.",
            "permissionTitle": "Accessibility", "permissionDescription": "Read and control app interfaces",
            "repair": "Allow", "later": "Skip", "completeInSettings": "Complete in Settings",
            "back": "Back", "dragInstruction": "Drag ChatGPT into the app list above",
        ]
        let shortInitial = IncodexPermissionInitialView(frame: NSRect(x: 0, y: 0, width: 600, height: 340))
        shortInitial.configure(copy: shortCopy, appIcon: source, permissionIcon: target, actionTarget: nil)
        let longInitial = IncodexPermissionInitialView(frame: NSRect(x: 0, y: 0, width: 600, height: 340))
        longInitial.configure(copy: longCopy, appIcon: source, permissionIcon: target, actionTarget: nil)
        let shortSize = shortInitial.preferredContentSize
        let longSize = longInitial.preferredContentSize
        let shortCard = laidOutCardFrame(shortInitial)
        let longCard = laidOutCardFrame(longInitial)
        precondition(shortSize.width == 600 && longSize.width == 600)
        precondition(shortSize.height > 0 && longSize.height > 0)
        precondition(abs((longSize.height - shortSize.height) - 18) <= 1.0,
                     "natural window height did not grow by one body line: short=\(shortSize), long=\(longSize)")
        precondition(abs((longCard.minY - shortCard.minY) - 18) <= 1.0,
                     "natural body line did not move card by 18pt: short=\(shortSize)/\(shortCard), long=\(longSize)/\(longCard)")
        precondition(abs((shortSize.height - shortCard.maxY) - (longSize.height - longCard.maxY)) <= 1.0,
                     "bottom Skip/edge anchor did not retain its natural bottom gap: short=\(shortSize), shortCard=\(shortCard), long=\(longSize), longCard=\(longCard)")

        // Reusing the same host must first enter the two-line geometry and then
        // return to the one-line geometry rather than retaining either height.
        shortInitial.setContent(title: "Enable ChatGPT scripting", body: "Allow Accessibility access.\nThis second line is intentional.", allowEnabled: true, settingsPlaceholder: false)
        let transitionedLongSize = shortInitial.preferredContentSize
        let transitionedLongCard = laidOutCardFrame(shortInitial)
        precondition(abs(transitionedLongSize.height - longSize.height) <= 0.5
                     && abs(transitionedLongCard.minY - longCard.minY) <= 0.5,
                     "long-body geometry did not apply on state transition: expected=\(longSize)/\(longCard), actual=\(transitionedLongSize)/\(transitionedLongCard)")
        shortInitial.setContent(title: "Enable ChatGPT scripting", body: "允许 ChatGPT 访问辅助功能。", allowEnabled: true, settingsPlaceholder: false)
        let restoredSize = shortInitial.preferredContentSize
        let restoredCard = laidOutCardFrame(shortInitial)
        precondition(abs(restoredSize.height - shortSize.height) <= 0.5
                     && abs(restoredCard.minY - shortCard.minY) <= 0.5,
                     "short-body geometry did not recover after a state transition: before=\(shortSize)/\(shortCard), after=\(restoredSize)/\(restoredCard)")

        // Direction changes must not turn the natural vertical measurement
        // into a locale-specific special case.
        let rtlShortCopy = NSMutableDictionary(dictionary: shortCopy)
        rtlShortCopy["layoutDirection"] = "rightToLeft"
        let rtlLongCopy = NSMutableDictionary(dictionary: longCopy)
        rtlLongCopy["layoutDirection"] = "rightToLeft"
        let rtlShortInitial = IncodexPermissionInitialView(frame: NSRect(x: 0, y: 0, width: 600, height: 340))
        rtlShortInitial.configure(copy: rtlShortCopy, appIcon: source, permissionIcon: target, actionTarget: nil)
        let rtlLongInitial = IncodexPermissionInitialView(frame: NSRect(x: 0, y: 0, width: 600, height: 340))
        rtlLongInitial.configure(copy: rtlLongCopy, appIcon: source, permissionIcon: target, actionTarget: nil)
        let rtlShortSize = rtlShortInitial.preferredContentSize
        let rtlLongSize = rtlLongInitial.preferredContentSize
        let rtlShortCard = laidOutCardFrame(rtlShortInitial)
        let rtlLongCard = laidOutCardFrame(rtlLongInitial)
        precondition(abs((rtlLongSize.height - rtlShortSize.height) - 18) <= 1.0
                     && abs((rtlLongCard.minY - rtlShortCard.minY) - 18) <= 1.0
                     && abs((rtlShortSize.height - rtlShortCard.maxY)
                            - (rtlLongSize.height - rtlLongCard.maxY)) <= 1.0,
                     "RTL natural body geometry diverged: short=\(rtlShortSize)/\(rtlShortCard), long=\(rtlLongSize)/\(rtlLongCard)")

        initial.setContent(title: "Error", body: "Retry after fixing permissions.", allowEnabled: false, settingsPlaceholder: true)
        initial.layoutSubtreeIfNeeded()
        precondition(initial.subviews[0] === initialHost)
        precondition(initial.permissionCardView.bounds.size == NSSize(width: 518, height: 80))
        let helper = IncodexPermissionHelperView(frame: NSRect(x: 0, y: 0, width: 531, height: 110))
        helper.configure(copy: copy, appIcon: source, actionTarget: nil)
        precondition(helper.preferredContentSize.width == 531)
        precondition(helper.preferredContentSize.height >= 110)
        precondition(helper.appRowFrame.size == NSSize(width: 459, height: 42))
        precondition(isHostingView(helper.appRowView))
        precondition(isHostingView(helper.subviews[0]))
        let helperShortSize = helper.preferredContentSize
        let helperLongCopy = NSMutableDictionary(dictionary: copy)
        helperLongCopy["dragInstruction"] = "Drag ChatGPT into the app list above.\nThen return to this window.\nA third instruction line."
        let freshLongHelper = IncodexPermissionHelperView(frame: NSRect(x: 0, y: 0, width: 531, height: 110))
        freshLongHelper.configure(copy: helperLongCopy, appIcon: source, actionTarget: nil)
        let helperLongSize = freshLongHelper.preferredContentSize
        precondition(helperLongSize.height > helperShortSize.height,
                     "fresh multiline helper must grow before any run-loop turn")
        helper.configure(copy: helperLongCopy, appIcon: source, actionTarget: nil)
        precondition(abs(helper.preferredContentSize.height - helperLongSize.height) <= 0.5,
                     "helper immediate measurement retained its previous short layout")
        helper.configure(copy: copy, appIcon: source, actionTarget: nil)
        precondition(abs(helper.preferredContentSize.height - helperShortSize.height) <= 0.5,
                     "helper immediate measurement retained its previous long layout")
        // The hint gets the natural width left by the verified 531pt parent:
        // leading18 + Back28 + spacing16 + hint leading4 + arrow28 + spacing8.
        // Its rendered Text must govern the extra height, not an older 408pt
        // NSString estimate. Urdu is a known wrap-boundary case.
        let urInstruction = "قابلِ رسائی کی اجازت دینے کے لیے ChatGPT کو اوپر موجود فہرست میں گھسیٹیں"
        let urRuns = #"[{"text":"قابلِ رسائی","role":"primary"},{"text":" کی اجازت دینے کے لیے ","role":"secondary"},{"text":"ChatGPT","role":"primary"},{"text":" کو اوپر موجود فہرست میں گھسیٹیں","role":"secondary"}]"#
        let naturalHintWidth: CGFloat = 531 - 18 - 28 - 16 - 4 - 28 - 8
        let urText = Text(permissionStyledInstruction(urInstruction, runsJSON: urRuns))
            .font(.body)
            .frame(width: naturalHintWidth, alignment: .leading)
            .environment(\.layoutDirection, LayoutDirection.rightToLeft)
        let oneLineText = Text("ChatGPT")
            .font(.body)
            .frame(width: naturalHintWidth, alignment: .leading)
            .environment(\.layoutDirection, LayoutDirection.rightToLeft)
        let urRenderedHeight = NSHostingView(rootView: urText).fittingSize.height
        let oneLineHeight = NSHostingView(rootView: oneLineText).fittingSize.height
        precondition(urRenderedHeight <= oneLineHeight + 1,
                     "Urdu hint is not one natural line: rendered=\(urRenderedHeight), baseline=\(oneLineHeight)")
        let urCopy = NSMutableDictionary(dictionary: copy)
        urCopy["dragInstruction"] = urInstruction
        urCopy["dragInstructionRuns"] = urRuns
        urCopy["layoutDirection"] = "rightToLeft"
        let urHelper = IncodexPermissionHelperView(frame: NSRect(x: 0, y: 0, width: 531, height: 110))
        urHelper.configure(copy: urCopy, appIcon: source, actionTarget: nil)
        precondition(abs(urHelper.preferredContentSize.height - helperShortSize.height) <= 0.5,
                     "one-line natural Urdu hint overexpanded helper: rendered=\(urRenderedHeight), helper=\(urHelper.preferredContentSize)")
        let catalogURL = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
            .appendingPathComponent("dist/incodex-permission-copy.json")
        let catalogData = try! Data(contentsOf: catalogURL)
        let catalog = try! JSONSerialization.jsonObject(with: catalogData) as! [String: [String: String]]
        precondition(catalog.count == 65, "permission copy catalog must retain all 65 locales")
        for locale in catalog.keys.sorted() {
            let localized = IncodexPermissionHelperView(frame: NSRect(x: 0, y: 0, width: 531, height: 110))
            localized.configure(copy: catalog[locale]! as NSDictionary, appIcon: source, actionTarget: nil)
            let size = localized.preferredContentSize
            let row = localized.appRowFrame
            precondition(size.width == 531 && size.height >= 110,
                         "invalid natural helper size for \(locale): \(size)")
            precondition(abs(row.maxY + 20 - size.height) <= 1,
                         "row lost its bottom anchor for \(locale): helper=\(size), row=\(row)")
        }
        // Routine checks must not repeatedly open a synthetic window on the
        // user's desktop. Keep actual Return behavior as an explicit UI run.
        guard ProcessInfo.processInfo.environment["INCODEX_RUN_NATIVE_LAYOUT_SMOKE"] == "1" else {
            precondition(NSApp.windows.allSatisfy { !$0.isVisible }, "headless smoke must not show windows")
            print("permission SwiftUI host smoke passed (headless; Return UI not run)")
            return
        }
        // Preserve the existing product Return-key contract in the real host;
        // a mocked TS callback cannot establish SwiftUI keyboard behavior.
        let sink = PermissionActionSink()
        initial.configure(copy: copy, appIcon: source, permissionIcon: target, actionTarget: sink)
        initial.setContent(title: "Keyboard", body: "Local contract", allowEnabled: true, settingsPlaceholder: false)
        let panel = NSPanel(contentRect: initial.bounds, styleMask: [.titled, .closable, .fullSizeContentView], backing: .buffered, defer: false)
        panel.isReleasedWhenClosed = false
        panel.contentView = initial
        panel.makeKeyAndOrderFront(nil)
        RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: panel.windowNumber, context: nil, characters: "\r", charactersIgnoringModifiers: "\r", isARepeat: false, keyCode: 36)!
        let handled = panel.performKeyEquivalent(with: event)
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        panel.orderOut(nil)
        panel.close()
        precondition(handled && sink.allowCount == 1, "Return must activate Allow exactly once")
        print("permission SwiftUI host smoke passed")
    }
}
