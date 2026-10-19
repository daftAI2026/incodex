/*
 * [INPUT]: 依赖既有目标锁、私有原子文件与NormalHost进程/端口证据。
 * [OUTPUT]: 串行化普通宿主启动；重复入口仅重激活已捕获的同一宿主。
 * [POS]: launch的App作用域owner记录；不持有会话或增加IPC服务。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos_launch_process::NormalHost;
    use std::net::TcpListener;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn repeated_launch_reuses_the_same_live_host_and_releases_its_record() {
        let root = std::env::temp_dir().join(format!(
            "incodex-launch-owner-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let app = root.join("Official.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/Info.plist"), "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>Host</string></dict></plist>").unwrap();
        symlink(
            std::env::current_exe().unwrap(),
            app.join("Contents/MacOS/Host"),
        )
        .unwrap();
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let host = NormalHost::current_process_for_test(port).unwrap();
        host.wait_for_test_listener_owner(port).unwrap();
        let LaunchClaim::Primary(mut owner) = claim(&root, &app).unwrap() else {
            panic!("first entry did not own launch");
        };
        owner.publish(&host).unwrap();
        let LaunchClaim::Existing(existing) = claim(&root, &app).unwrap() else {
            panic!("second entry created another owner");
        };
        assert_eq!(existing, host);
        drop(owner);
        assert!(matches!(
            claim(&root, &app).unwrap(),
            LaunchClaim::Primary(_)
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}
