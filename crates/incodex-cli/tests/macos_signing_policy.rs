#![cfg(target_os = "macos")]
/*
 * [INPUT]: 依赖安装签名上下文的选择边界与临时空 root。
 * [OUTPUT]: 约束仅显式官方安装创建身份，后台恢复缺失注册硬失败，自定义 app 保持旧行为。
 * [POS]: CLI 与稳定证书 backend 的接入合同，不签名或启动真实宿主。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use incodex_cli::macos_signing::context_for_install;
use incodex_macos::SigningContext;
fn missing_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "incodex-signing-policy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
#[test]
fn custom_install_does_not_create_or_read_official_signing_state() {
    let root = missing_root();
    assert_eq!(
        context_for_install(&root, false, false).unwrap(),
        SigningContext::Adhoc
    );
    assert!(!root.exists());
}
#[test]
fn background_restore_missing_identity_fails_without_creating_it() {
    let root = missing_root();
    assert!(context_for_install(&root, true, true).is_err());
    assert!(!root.exists());
}
