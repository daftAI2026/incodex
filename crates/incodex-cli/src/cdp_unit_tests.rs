use super::*;
use crate::profile_mask::{ProfileAvatar, ProfileMask};
use std::net::Ipv4Addr;
use std::time::Instant;

#[test]
fn prefers_codex_app_page_and_skips_chrome_and_prewarm() {
    let targets = vec![
        CdpTarget {
            id: "a".into(),
            r#type: "page".into(),
            url: "chrome://newtab".into(),
            ws: "ws://127.0.0.1:1/devtools/page/a".into(),
        },
        CdpTarget {
            id: "d".into(),
            r#type: "page".into(),
            url: "app://-/index.html?initialRoute=%2Favatar-overlay".into(),
            ws: "ws://127.0.0.1:1/devtools/page/d".into(),
        },
        CdpTarget {
            id: "b".into(),
            r#type: "page".into(),
            url: "app://-/index.html?initialRoute=%2Fchatgpt%2Fquick-chat-prewarm".into(),
            ws: "ws://127.0.0.1:1/devtools/page/b".into(),
        },
        CdpTarget {
            id: "c".into(),
            r#type: "page".into(),
            url: "app://-/index.html".into(),
            ws: "ws://127.0.0.1:1/devtools/page/c".into(),
        },
    ];
    let picked = pick_codex_page_target(&targets).unwrap();
    assert_eq!(picked.url, "app://-/index.html");
    assert!(inject_source().contains("__incodexIncognito=true"));
    assert!(inject_source().contains("data-incodex-privacy-toggle"));
}

#[test]
fn isolated_launch_uses_the_official_new_codex_deep_link() {
    let args = debug_launch_args("/tmp/incodex-chromium", 43123);
    assert_eq!(
        args.last().map(String::as_str),
        Some("codex://new?mode=codex")
    );
}

#[test]
fn platform_cdp_networking_preserves_the_macos_contract() {
    let mac = debug_launch_args_for_platform("/tmp/incodex-chromium", 43123, false);
    assert!(!mac
        .iter()
        .any(|arg| arg.starts_with("--remote-debugging-address=")));
    assert_eq!(cdp_hosts_for_platform(false), &["127.0.0.1", "[::1]"]);

    let windows = debug_launch_args_for_platform("C:\\tmp\\incodex", 43123, true);
    assert!(windows
        .iter()
        .any(|arg| arg == "--remote-debugging-address=127.0.0.1"));
    assert_eq!(cdp_hosts_for_platform(true), &["127.0.0.1"]);
}

#[test]
#[cfg(not(target_os = "windows"))]
fn isolated_launch_disables_the_native_window_birth_animation() {
    let args = debug_launch_args("/tmp/incodex-chromium", 43123);
    assert_eq!(&args[..2], ["-NSAutomaticWindowAnimationsEnabled", "false"]);
}

#[test]
fn cdp_http_finishes_at_content_length_even_when_chromium_keeps_socket_open() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = "[]";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        thread::sleep(Duration::from_millis(2_500));
    });

    let started = Instant::now();
    let value = http_get_json_host("127.0.0.1", port, "/json/list")
        .expect("a complete Content-Length body must not wait for EOF");
    assert_eq!(value, json!([]));
    assert!(started.elapsed() < Duration::from_millis(500));
    server.join().unwrap();
}

#[test]
fn cdp_http_has_an_overall_deadline_for_a_slow_local_endpoint() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]";
        for byte in response {
            if stream.write_all(&[*byte]).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    });

    let started = Instant::now();
    let result = http_get_json_host("127.0.0.1", port, "/json/list");
    assert!(
        result.is_err(),
        "a slow endpoint must not complete normally"
    );
    assert!(
        started.elapsed() < Duration::from_millis(2_500),
        "slow CDP endpoint exceeded its bounded operation deadline"
    );
    server.join().unwrap();
}

#[test]
fn cdp_websocket_handshake_has_a_finite_timeout() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let response = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: invalid\r\n\r\n";
        for byte in response {
            if stream.write_all(&[*byte]).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(60));
        }
    });

    let started = Instant::now();
    let result = connect_cdp_websocket(&format!("ws://127.0.0.1:{port}/devtools/page/test"), port);
    assert!(result.is_err());
    assert!(
        started.elapsed() < Duration::from_millis(2_500),
        "CDP WebSocket handshake was not bounded"
    );
    server.join().unwrap();
}

#[test]
fn cdp_command_read_has_an_overall_deadline_for_a_fragmented_frame() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        let payload = br#"{"id":1,"result":{"value":"0123456789abcdef"}}"#;
        let mut frame = vec![0x81, payload.len() as u8];
        frame.extend_from_slice(payload);
        let raw = socket.get_mut();
        for byte in frame {
            if raw.write_all(&[byte]).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(70));
        }
    });

    let mut socket =
        connect_cdp_websocket(&format!("ws://127.0.0.1:{port}/devtools/page/test"), port).unwrap();
    let started = Instant::now();
    let result = send_cdp(&mut socket, 1, "Runtime.evaluate", json!({}));
    assert!(result.is_err(), "a slow fragmented response must time out");
    assert!(
        started.elapsed() < Duration::from_millis(2_500),
        "fragmented CDP command exceeded its overall deadline"
    );
    server.join().unwrap();
}

#[test]
fn cdp_websocket_is_confined_to_the_allocated_loopback_port() {
    assert!(validate_cdp_websocket_url("ws://127.0.0.1:43123/devtools/page/a", 43123).is_ok());
    assert!(validate_cdp_websocket_url("ws://[::1]:43123/devtools/page/a", 43123).is_ok());
    assert!(validate_cdp_websocket_url("ws://127.0.0.1:43124/devtools/page/a", 43123).is_err());
    assert!(validate_cdp_websocket_url("ws://example.com:43123/devtools/page/a", 43123).is_err());
}

#[test]
fn injected_ui_carries_locale_and_requires_button_and_banner_health() {
    let source = inject_source_for_locale(Some("zh-CN"));
    assert!(source.contains("window.__incodexIncognito=true"));
    assert!(source.contains("window.__incodexLocale=\"zh-CN\""));
    let health = ui_ready_expression();
    assert!(health.contains("data-incodex-privacy-toggle"));
    assert!(health.contains("data-incodex-banner-host"));
}

#[test]
fn injected_ui_carries_profile_mask_as_a_json_bootstrap_value() {
    let source = inject_source_for_options(&InjectionOptions {
        locale: Some("en-US".into()),
        profile_mask: Some(ProfileMask {
            name: "Temporary".into(),
            avatar: ProfileAvatar::Generated,
        }),
    });

    assert!(source.contains(
        "window.__incodexProfileMask=(window.top===window&&window.location.href===\"app://-/index.html\")?{\"name\":\"Temporary\",\"avatar\":{\"kind\":\"generated\"}}:null"
    ));
    assert!(source.contains("window.__incodexLocale=\"en-US\""));
}

#[test]
fn profile_payload_is_null_outside_the_exact_top_level_codex_page() {
    let source = inject_source_for_options(&InjectionOptions {
        locale: None,
        profile_mask: Some(ProfileMask {
            name: "Quiet Otter".into(),
            avatar: ProfileAvatar::Generated,
        }),
    });

    assert!(source.contains("window.top===window"));
    assert!(source.contains("window.location.href===\"app://-/index.html\""));
    assert!(source.contains(":null;"));
}

#[test]
fn browser_close_uses_the_cdp_browser_command() {
    assert_eq!(
        browser_close_message(),
        json!({ "id": 1, "method": "Browser.close", "params": {} })
    );
}

#[test]
fn profile_mask_transport_grace_is_windows_only() {
    assert_eq!(profile_mask_transport_failure_polls(false), 2);
    assert_eq!(profile_mask_transport_failure_polls(true), 4);
}

#[test]
fn macos_profile_mask_failures_keep_shared_consecutive_timing() {
    let mut failures = ProfileMaskFailureCounters::for_platform(false);
    assert!(!failures.record(ProfileMaskFailureKind::Unhealthy, 2));
    assert!(
        failures.record(ProfileMaskFailureKind::Transport, 2),
        "macOS must preserve the original two consecutive failures across failure kinds"
    );
}

#[test]
fn profile_mask_failure_kinds_do_not_borrow_each_others_threshold() {
    let mut failures = ProfileMaskFailureCounters::for_platform(true);
    assert!(!failures.record(ProfileMaskFailureKind::Unhealthy, 2));
    for _ in 0..3 {
        assert!(!failures.record(ProfileMaskFailureKind::Transport, 4));
    }

    let mut reverse = ProfileMaskFailureCounters::for_platform(true);
    for _ in 0..3 {
        assert!(!reverse.record(ProfileMaskFailureKind::Transport, 4));
    }
    assert!(!reverse.record(ProfileMaskFailureKind::Unhealthy, 2));
}
