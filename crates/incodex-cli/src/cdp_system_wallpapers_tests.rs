//! [INPUT]: 系统壁纸 CDP 白名单请求解析器。
//! [OUTPUT]: 固定无任意文件读取、未知字段拒绝的回归契约。
//! [POS]: cdp_system_wallpapers 的传输边界测试。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::*;

#[test]
fn system_wallpaper_requests_allow_only_catalog_and_opaque_id() {
    assert_eq!(
        parse_request(&json!({"id":"request-1","kind":"list"})),
        Some(Request::List {
            id: "request-1".into()
        })
    );
    assert_eq!(
        parse_request(&json!({"id":"request-2","kind":"load","wallpaperId":"system-0"})),
        Some(Request::Load {
            id: "request-2".into(),
            wallpaper_id: "system-0".into()
        })
    );
    for value in [
        json!({"id":"request-1","kind":"list","path":"/tmp/private"}),
        json!({"id":"request-1","kind":"load","wallpaperId":"../../private"}),
        json!({"id":"request-1","kind":"download"}),
        json!({"id":"","kind":"list"}),
    ] {
        assert!(parse_request(&value).is_none());
    }
}

#[test]
fn restore_accepts_no_user_supplied_preference_path() {
    assert_eq!(parse_request(&json!({"id":"restore-1","kind":"restore"})), Some(Request::Restore { id: "restore-1".into() }));
    assert!(parse_request(&json!({"id":"restore-1","kind":"restore","path":"/tmp/other"})).is_none());
}
