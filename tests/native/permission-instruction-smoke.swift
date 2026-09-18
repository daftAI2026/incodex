import Foundation
import SwiftUI

@main
enum PermissionInstructionSmoke {
    static func main() {
        let text = "Drag ChatGPT to allow Accessibility"
        let json = #"[{"text":"Drag ","role":"secondary"},{"text":"ChatGPT","role":"primary"},{"text":" to allow ","role":"secondary"},{"text":"Accessibility","role":"primary"}]"#
        let styled = permissionStyledInstruction(text, runsJSON: json)
        precondition(String(styled.characters) == text)
        let runs = Array(styled.runs)
        precondition(runs.count == 4)
        precondition(runs[0].foregroundColor == Color.secondary)
        precondition(runs[1].foregroundColor == Color.primary)
        precondition(runs[2].foregroundColor == Color.secondary)
        precondition(runs[3].foregroundColor == Color.primary)

        let rtl = "اسحب ChatGPT إلى تسهيلات الاستخدام"
        let rtlJSON = #"[{"text":"اسحب ","role":"secondary"},{"text":"ChatGPT","role":"primary"},{"text":" إلى ","role":"secondary"},{"text":"تسهيلات الاستخدام","role":"primary"}]"#
        precondition(String(permissionStyledInstruction(rtl, runsJSON: rtlJSON).characters) == rtl)

        // Invalid metadata must never replace, truncate, reorder, or partly
        // recolor the chosen localized fallback instruction.
        for invalid in ["", "not-json", "[]", "{}", "null",
                        #"[{"text":"different","role":"primary"}]"#,
                        #"[{"text":"Drag ChatGPT to allow Accessibility","role":"unknown"}]"#,
                        #"[{"role":"primary"}]"#,
                        #"[{"text":42,"role":"primary"}]"#] {
            let fallback = permissionStyledInstruction(text, runsJSON: invalid)
            precondition(String(fallback.characters) == text)
            precondition(fallback.runs.allSatisfy { $0.foregroundColor == nil })
        }
        print("permission instruction semantic runs passed (no windows)")
    }
}
