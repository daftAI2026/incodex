/**
 * [INPUT]: 依赖共用 Swift 呈现适配、合成系统词表和已构建文案。
 * [OUTPUT]: 验证 26/27 名称、模板、语言回退与图标选择。
 * [POS]: 无窗呈现回归；不读取或申请 TCC 权限。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import AppKit
import Foundation

@main
struct PermissionPlatformSmoke {
    @MainActor
    static func main() throws {
        let data = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let catalog = try JSONSerialization.jsonObject(with: data) as! [String: [String: String]]
        let labels = [
            "en": ["ACCESSIBILITY": "Device Control and Data Access", "SCREEN_CAPTURE_HEADER": "Screen & System Audio Recording"],
            "fr": ["ACCESSIBILITY": "Contrôle de l’appareil et accès aux données", "SCREEN_CAPTURE_HEADER": "Enregistrement de l’écran et des sons du système"],
            "fr_CA": ["ACCESSIBILITY": "Contrôle de l’appareil et accès aux données", "SCREEN_CAPTURE_HEADER": "Enregistrement de l’écran et de l’audio système"],
            "zh_CN": ["ACCESSIBILITY": "设备控制和数据访问", "SCREEN_CAPTURE_HEADER": "录屏与系统录音"],
        ]
        for (locale, source) in catalog {
            let copy = source as NSDictionary
            for version in [0, 12, 26] {
                precondition(IncodexPermissionPlatform.resolve(copy, majorVersion: version, labels: labels).isEqual(copy))
                precondition(IncodexPermissionPlatform.accessibilityIconType(majorVersion: version) == nil)
            }
            let current = IncodexPermissionPlatform.resolve(copy, majorVersion: 27, labels: labels)
            precondition(copy.isEqual(source as NSDictionary), "input changed: \(locale)")
            precondition(current["permissionDescription"] as? String == source["permissionDescription"])
            precondition(current["body"] as? String == source["body"])
            precondition(current["repair"] as? String == source["repair"])
            precondition(current["layoutDirection"] as? String == source["layoutDirection"])
            precondition(current.allValues.allSatisfy { $0 is String })
            let title = current["permissionTitle"] as! String
            let instruction = current["dragInstruction"] as! String
            let rawRuns = (current["dragInstructionRuns"] as! String).data(using: .utf8)!
            let runs = try JSONSerialization.jsonObject(with: rawRuns) as! [[String: String]]
            let canonicalRuns = try JSONSerialization.data(withJSONObject: runs, options: [.sortedKeys])
            precondition(canonicalRuns == rawRuns, "runs serialization must be deterministic")
            precondition(runs.map { $0["text"]! }.joined() == instruction)
            precondition(!instruction.contains("{permissionTitle}"))
            precondition(current["addedBody"] as? String == instruction)
            precondition(instruction.contains("/Applications/ChatGPT.app"))
            precondition((current["errorBody"] as! String).contains(title))
            precondition((current["errorBody"] as! String).contains("incodex accessibility"))
            switch locale {
            case "fr-FR", "fr-CA": precondition(title == labels["fr"]!["ACCESSIBILITY"]!)
            case "zh-CN": precondition(title == labels["zh_CN"]!["ACCESSIBILITY"]!)
            case "en", "am": precondition(title == labels["en"]!["ACCESSIBILITY"]!)
            default: precondition(labels.values.contains { $0["ACCESSIBILITY"] == title })
            }
        }
        let french = catalog["fr-FR"]! as NSDictionary
        let canadian = IncodexPermissionPlatform.resolve(catalog["fr-CA"]! as NSDictionary, majorVersion: 27, labels: labels)
        precondition(canadian["screenTitle"] as? String == labels["fr_CA"]!["SCREEN_CAPTURE_HEADER"])
        precondition(IncodexPermissionPlatform.resolve(french, majorVersion: 27, labels: [:]).isEqual(french))
        precondition(IncodexPermissionPlatform.resolve(french, majorVersion: 27, labels: ["fr": ["ACCESSIBILITY": " "]]).isEqual(french))
        precondition(IncodexPermissionPlatform.resolve(catalog["am"]! as NSDictionary, majorVersion: 27, labels: ["de": ["ACCESSIBILITY": "Gerätesteuerung und Datenzugriff"]]).isEqual(catalog["am"]! as NSDictionary))
        let nested: [String: Any] = ["fr": ["ACCESSIBILITY": labels["fr"]!["ACCESSIBILITY"]!, "plural": ["one": "one item"]]]
        precondition(IncodexPermissionPlatform.stringLabels(nested)["fr"]?["ACCESSIBILITY"] == labels["fr"]!["ACCESSIBILITY"])
        let onlyAX = IncodexPermissionPlatform.resolve(french, majorVersion: 27, labels: ["fr": ["ACCESSIBILITY": labels["fr"]!["ACCESSIBILITY"]!]])
        precondition(onlyAX["screenTitle"] as? String == french["screenTitle"] as? String)
        precondition(onlyAX["screenDragInstructionRuns"] as? String == french["screenDragInstructionRuns"] as? String)
        let quoted = IncodexPermissionPlatform.resolve(catalog["en"]! as NSDictionary, majorVersion: 27, labels: ["en": ["ACCESSIBILITY": "Device \"Control\""]])
        let quotedRuns = try JSONSerialization.jsonObject(with: (quoted["dragInstructionRuns"] as! String).data(using: .utf8)!) as! [[String: String]]
        precondition(quotedRuns.map { $0["text"]! }.joined() == quoted["dragInstruction"] as! String)
        let renamedScreen = IncodexPermissionPlatform.resolve(catalog["en"]! as NSDictionary, majorVersion: 27, labels: [
            "en": ["ACCESSIBILITY": "Device Control and Data Access", "SCREEN_CAPTURE_HEADER": "New Screen Name"],
        ])
        precondition((renamedScreen["screenDragInstruction"] as! String).contains("New Screen Name"))
        precondition((renamedScreen["screenChecking"] as! String).contains("New Screen Name"))
        let screenRuns = try JSONSerialization.jsonObject(with: (renamedScreen["screenDragInstructionRuns"] as! String).data(using: .utf8)!) as! [[String: String]]
        precondition(screenRuns.map { $0["text"]! }.joined() == renamedScreen["screenDragInstruction"] as! String)
        precondition(IncodexPermissionPlatform.accessibilityIconType(majorVersion: 27) == "com.apple.settings.PrivacySecurity.extension.privacy-accessibility")
        precondition(IncodexPermissionPlatform.accessibilityIconType(majorVersion: 28) == "com.apple.settings.PrivacySecurity.extension.privacy-accessibility")
        precondition(IncodexPermissionPlatform.accessibilityIcon() != nil)
        print("permission platform passed (26 unchanged; 27 names, templates, fallback, icons; no TCC)")
    }
}
