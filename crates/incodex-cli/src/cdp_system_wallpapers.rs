//! [INPUT]: 依赖 cdp 的受控连接与系统壁纸主机装载器。
//! [OUTPUT]: 提供系统壁纸目录和原图请求的封闭路由。
//! [POS]: cdp 的 Shot 专用 adapter，不暴露任意路径或下载动作。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::{send_cdp, TcpStream, WebSocket};
use crate::system_wallpapers::SystemWallpaperLibrary;
use serde_json::{json, Value};

#[derive(Debug, PartialEq, Eq)]
enum Request {
    List { id: String },
    Restore { id: String },
    Load { id: String, wallpaper_id: String },
}
fn opaque_id(value: &Value) -> Option<String> {
    let value = value.as_str()?;
    (!value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
    .then(|| value.to_owned())
}
fn parse_request(value: &Value) -> Option<Request> {
    let object = value.as_object()?;
    let id = opaque_id(object.get("id")?)?;
    match object.get("kind")?.as_str()? {
        "list" if object.len() == 2 => Some(Request::List { id }),
        "restore" if object.len() == 2 => Some(Request::Restore { id }),
        "load" if object.len() == 3 => Some(Request::Load {
            id,
            wallpaper_id: opaque_id(object.get("wallpaperId")?)?,
        }),
        _ => None,
    }
}

pub(super) fn poll(
    socket: &mut WebSocket<TcpStream>,
    next_id: &mut u64,
    library: &mut SystemWallpaperLibrary,
) -> Result<(), String> {
    let response = send_cdp(
        socket,
        *next_id,
        "Runtime.evaluate",
        json!({
            "expression": "window.__incodexTakeSystemWallpaperRequest?.() ?? null",
            "returnByValue": true,
        }),
    )?;
    *next_id += 1;
    let Some(request) = response
        .pointer("/result/result/value")
        .and_then(parse_request)
    else {
        return Ok(());
    };
    let root = incodex_core::paths::user_root();
    let result = match request {
        Request::Restore { id } => {
            let entries = if crate::shot_wallpaper_preference::enabled(&root) {
                library.list().unwrap_or_default()
            } else {
                Vec::new()
            };
            json!({ "id": id, "ok": true, "entries": entries })
        }
        Request::List { id } => match library.list() {
            Ok(entries) => json!({ "id": id, "ok": true, "entries": entries }),
            Err(_) => {
                json!({ "id": id, "ok": false, "error": "system wallpaper catalog unavailable" })
            }
        },
        Request::Load { id, wallpaper_id } => match library.load(&wallpaper_id).and_then(|data| {
            crate::shot_wallpaper_preference::remember(&root)?;
            Ok(data)
        }) {
            Ok(data_url) => json!({ "id": id, "ok": true, "dataUrl": data_url }),
            Err(_) => {
                json!({ "id": id, "ok": false, "error": "system wallpaper image unavailable" })
            }
        },
    };
    send_cdp(
        socket,
        *next_id,
        "Runtime.evaluate",
        json!({
            "expression": format!("window.__incodexResolveSystemWallpaper?.({result})"),
            "returnByValue": true,
        }),
    )?;
    *next_id += 1;
    Ok(())
}

#[cfg(test)]
#[path = "cdp_system_wallpapers_tests.rs"]
mod tests;
