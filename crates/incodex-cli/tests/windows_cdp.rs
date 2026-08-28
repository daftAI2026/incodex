#![cfg(target_os = "windows")]

use incodex_cli::cdp::{
    allocate_debug_port, debug_launch_args, inject_source, validate_cdp_websocket_url,
};

#[test]
fn windows_debug_launch_is_explicitly_bound_to_ipv4_loopback() {
    let port = allocate_debug_port().expect("allocate localhost debug port");
    let user_data = r"C:\Users\测试 User\AppData\Local\Incodex\chromium";
    let args = debug_launch_args(user_data, port);

    assert!(args.contains(&format!("--user-data-dir={user_data}")));
    assert!(args.contains(&"--remote-debugging-address=127.0.0.1".to_string()));
    assert!(args.contains(&format!("--remote-debugging-port={port}")));
    assert_eq!(
        args.last().map(String::as_str),
        Some("codex://new?mode=codex")
    );
    assert!(!args.iter().any(|arg| arg.starts_with("-NS")));
}

#[test]
fn windows_cdp_accepts_only_the_allocated_loopback_endpoint() {
    validate_cdp_websocket_url("ws://127.0.0.1:43123/devtools/page/codex", 43123)
        .expect("accept exact IPv4 loopback endpoint");
    validate_cdp_websocket_url("ws://[::1]:43123/devtools/page/codex", 43123)
        .expect("accept exact IPv6 loopback endpoint");

    for untrusted in [
        "ws://192.0.2.10:43123/devtools/page/codex",
        "ws://127.0.0.1:43124/devtools/page/codex",
        "ws://localhost:43123/devtools/page/codex",
    ] {
        assert!(
            validate_cdp_websocket_url(untrusted, 43123).is_err(),
            "accepted untrusted CDP endpoint: {untrusted}"
        );
    }
}

#[test]
fn windows_uses_the_committed_shared_runtime_injection() {
    let source = inject_source();
    assert!(source.contains("window.__incodexIncognito=true"));
    assert!(source.contains("data-incodex-privacy-toggle"));
}
