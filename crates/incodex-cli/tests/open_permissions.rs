//! [INPUT]: 依赖实际CLI二进制的Mach-O导入表与系统nm工具。
//! [OUTPUT]: 锁定CLI不重新引入无障碍授权提示入口。
//! [POS]: open权限归属回归；只读二进制，不启动宿主或请求权限。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

#[cfg(target_os = "macos")]
fn imports_accessibility_prompt(symbols: &str) -> bool {
    symbols
        .lines()
        .any(|line| line.split_whitespace().last() == Some("_AXIsProcessTrustedWithOptions"))
}

#[cfg(target_os = "macos")]
#[test]
fn prompt_import_detector_has_a_positive_control() {
    assert!(imports_accessibility_prompt(
        "_AXIsProcessTrustedWithOptions\n"
    ));
    assert!(!imports_accessibility_prompt("_AXIsProcessTrusted\n"));
}

#[cfg(target_os = "macos")]
#[test]
fn native_cli_does_not_import_its_own_accessibility_prompt() {
    let output = std::process::Command::new("/usr/bin/nm")
        .args(["-u", env!("CARGO_BIN_EXE_incodex")])
        .output()
        .expect("只读CLI的Mach-O导入表");
    assert!(output.status.success(), "nm失败不能作为没有导入的证据");
    let symbols = std::str::from_utf8(&output.stdout).expect("nm UTF-8输出");
    assert!(
        !imports_accessibility_prompt(symbols),
        "CLI不得导入自动请求自身无障碍授权的入口"
    );
}
