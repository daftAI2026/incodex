//! [INPUT]: 依赖 cdp 的受控连接与系统壁纸主机装载器。
//! [OUTPUT]: 提供系统壁纸目录和原图请求的封闭路由。
//! [POS]: cdp 的 Shot 专用 adapter，不暴露任意路径或下载动作。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use serde_json::{json, Value};
#[derive(Debug, PartialEq, Eq)]
enum Request {
    List { id: String },
    Load { id: String, wallpaper_id: String },
}
fn parse_request(_value: &Value) -> Option<Request> { None }
#[cfg(test)]
#[path = "cdp_system_wallpapers_tests.rs"]
mod tests;
