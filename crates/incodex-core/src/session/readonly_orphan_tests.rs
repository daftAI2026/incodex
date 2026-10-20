// ============================================================================
// readonly_orphan_tests: 验证同步 orphan sweep 不授权只读 session 子树
// ============================================================================
// [INPUT]: 依赖 session_tests 的真实临时目录与 Unix 权限断言
// [OUTPUT]: 验证 dead-owner sweep 保留 0500 目录及其内容
// [POS]: session_tests 的窄权限回归；真实 close 的 quiesced burn 另测
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use super::*;

#[test]
fn orphan_sweep_preserves_readonly_tree_without_deleting_it() {
    if skip_if_root("orphan_sweep_preserves_readonly_tree_without_deleting_it") {
        return;
    }

    let root = temp_root();
    let user_root = root.join(".incodex");
    let session = create_session_home(&user_root, None, 999_999, "").unwrap();
    let readonly = session
        .chromium
        .join("artifact-sessions/generation-X/runtime");
    fs::create_dir_all(&readonly).unwrap();
    let payload = readonly.join("runtime.bin");
    fs::write(&payload, b"owner helper may still be writing").unwrap();
    fs::set_permissions(&readonly, fs::Permissions::from_mode(0o500)).unwrap();
    let original_identity = fs::symlink_metadata(&session.root).unwrap();

    let swept = sweep_orphan_sessions_with_probe(&user_root, None, |_| ProcessProbe::Dead);
    let remaining_identity = fs::symlink_metadata(&session.root).ok();
    let remaining_mode = fs::symlink_metadata(&readonly)
        .ok()
        .map(|metadata| metadata.permissions().mode() & 0o777);
    let remaining_payload = fs::read(&payload).ok();

    if remaining_mode.is_some() {
        fs::set_permissions(&readonly, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::remove_dir_all(&root).unwrap();

    assert_eq!(swept, 0, "synchronous sweep must retain readonly residue");
    assert!(remaining_identity.is_some_and(|metadata| {
        metadata.ino() == original_identity.ino() && metadata.dev() == original_identity.dev()
    }));
    assert_eq!(remaining_mode, Some(0o500));
    assert_eq!(
        remaining_payload.as_deref(),
        Some(&b"owner helper may still be writing"[..])
    );
}
