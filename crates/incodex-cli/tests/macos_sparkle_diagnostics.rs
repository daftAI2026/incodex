/**
 * [INPUT]: 依赖 tests/native 的离线 smoke runner 与 macOS Foundation 工具链。
 * [OUTPUT]: 将 interposer 原生编译/执行合同接入 macOS workspace 测试。
 * [POS]: crates/incodex-cli 的原生集成测试；不启动真实宿主、Sparkle 更新或权限流程。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#[cfg(target_os = "macos")]
#[test]
fn sparkle_interposer_diagnostics_preserve_original_callback_contract() {
    use std::path::PathBuf;
    use std::process::Command;

    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let runner = repository.join("tests/native/macos-sparkle-interpose-trace-smoke.sh");
    let output = Command::new("/bin/sh")
        .arg(runner)
        .output()
        .expect("start isolated native Sparkle diagnostic fixture");

    assert!(
        output.status.success(),
        "native Sparkle diagnostic fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
