//! Regression coverage for exact session markers and official-process isolation.
use incodex_macos::session_process_ids_from_ps;
use std::path::Path;

#[test]
fn session_process_filter_requires_the_exact_inherited_root_marker() {
    let root = Path::new("/Users/test/.incodex/sessions/target/s-123");
    let snapshot = "\
410  /Applications/ChatGPT.app/Contents/MacOS/ChatGPT INCODEX_SESSION_ROOT=/Users/test/.incodex/sessions/target/s-123 CODEX_HOME=/tmp/home\n\
411  /Applications/ChatGPT.app/Contents/Resources/codex app-server INCODEX_SESSION_ROOT=/Users/test/.incodex/sessions/target/s-123\n\
412  /Applications/ChatGPT.app/Contents/Helpers/browser_crashpad_handler --database=/Users/test/.incodex/sessions/target/s-123/chromium/Crashpad\n\
413  /Applications/ChatGPT.app/Contents/MacOS/ChatGPT INCODEX_SESSION_ROOT=/Users/test/.incodex/sessions/target/s-1234\n\
414  /Applications/ChatGPT.app/Contents/MacOS/ChatGPT CODEX_HOME=/Users/test/.codex\n\
415  /tmp/not-crashpad --database=/Users/test/.incodex/sessions/target/s-123/chromium/Crashpad\n\
416  /Applications/ChatGPT.app/Contents/Helpers/browser_crashpad_handler --database=/Users/test/.incodex/sessions/target/s-1234/chromium/Crashpad\n\
417  /Applications/ChatGPT.app/Contents/Helpers/browser_crashpad_handler --database=/Users/test/.incodex/sessions/target/s-123/chromium/Crashpad-old\n";

    assert_eq!(
        session_process_ids_from_ps(snapshot, root),
        vec![410, 411, 412],
        "已验证 session 的 Crashpad 不继承普通环境标记，但其精确数据库路径仍必须在烧毁前被回收"
    );
}

#[test]
fn session_process_filter_handles_roots_with_spaces_without_prefix_matches() {
    let root = Path::new("/Users/test/My Data/.incodex/sessions/target/s-9");
    let snapshot = "\
510 /helper INCODEX_SESSION_ROOT=/Users/test/My Data/.incodex/sessions/target/s-9 CODEX_HOME=/tmp\n\
511 /helper INCODEX_SESSION_ROOT=/Users/test/My Data/.incodex/sessions/target/s-90 CODEX_HOME=/tmp\n";

    assert_eq!(session_process_ids_from_ps(snapshot, root), vec![510]);
}
