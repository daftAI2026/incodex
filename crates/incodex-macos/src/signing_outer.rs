use std::path::Path;
use std::process::Command;

use crate::signature_inspection::inspect_codesign;
use crate::signing::SignedComponent;

/// 读取并严格验证 outer 签名，不递归枚举或 deep 验证 nested components。
pub fn inspect_outer_signing(path: &Path) -> Result<SignedComponent, String> {
    inspect_codesign(path, verify_outer_strict)
}

fn verify_outer_strict(path: &Path) -> bool {
    Command::new("codesign")
        .args(["--verify", "--strict", "--verbose=4", "--"])
        .arg(path)
        .output()
        .is_ok_and(|output| output.status.success())
}
