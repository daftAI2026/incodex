//! [INPUT]: 依赖 cdp 的受控连接与系统壁纸主机装载器。
//! [OUTPUT]: 提供系统壁纸目录和原图请求的封闭路由。
//! [POS]: Shot 专用 adapter，只接受不透明资源 ID；下载在有界后台任务中执行，不阻塞截图轮询。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::{send_cdp, TcpStream, WebSocket};
#[cfg(not(target_os = "macos"))]
use crate::system_wallpapers::SystemWallpaperLibrary;
use serde_json::{json, Value};

pub(super) struct Host {
    #[cfg(not(target_os = "macos"))]
    library: SystemWallpaperLibrary,
    #[cfg(target_os = "macos")]
    pair: PairHost,
}
impl Host {
    pub(super) fn new(current: Option<std::path::PathBuf>) -> Self {
        #[cfg(target_os = "macos")]
        {
            let _ = current;
            Self {
                pair: PairHost::new(),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self {
                library: SystemWallpaperLibrary::new(current),
            }
        }
    }
}

#[cfg(target_os = "macos")]
struct PairHost {
    major: u32,
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tx: std::sync::mpsc::Sender<(String, Result<String, String>)>,
    rx: std::sync::mpsc::Receiver<(String, Result<String, String>)>,
    pending: std::collections::HashMap<String, Vec<String>>,
    ready: std::collections::HashMap<String, String>,
    workers: std::collections::HashMap<String, std::thread::JoinHandle<()>>,
}
#[cfg(target_os = "macos")]
impl PairHost {
    fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            major: crate::macos_system_wallpapers::major_version().unwrap_or(0),
            alive: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            tx,
            rx,
            pending: Default::default(),
            ready: Default::default(),
            workers: Default::default(),
        }
    }
    fn load(&mut self, request_id: String, wallpaper_id: String) -> Option<Value> {
        if crate::macos_system_wallpapers::source_for_version(self.major, &wallpaper_id).is_err() {
            return Some(json!({"id":request_id,"ok":false}));
        }
        if let Some(data) = self.ready.get(&wallpaper_id) {
            return Some(json!({"id":request_id,"ok":true,"dataUrl":data}));
        }
        if self.pending.values().map(Vec::len).sum::<usize>() >= 4 {
            return Some(json!({"id":request_id,"ok":false}));
        }
        if let Some(waiters) = self.pending.get_mut(&wallpaper_id) {
            waiters.push(request_id);
            return None;
        }
        self.pending.insert(wallpaper_id.clone(), vec![request_id]);
        let (tx, alive, major) = (self.tx.clone(), self.alive.clone(), self.major);
        let worker_id = wallpaper_id.clone();
        let worker = std::thread::spawn(move || {
            let result = crate::macos_system_wallpapers::load(major, &wallpaper_id, &alive);
            let _ = tx.send((wallpaper_id, result));
        });
        self.workers.insert(worker_id, worker);
        None
    }
    fn completed(&mut self) -> Vec<Value> {
        let mut replies = Vec::new();
        for (wallpaper_id, result) in self.rx.try_iter() {
            if let Some(worker) = self.workers.remove(&wallpaper_id) {
                let _ = worker.join();
            }
            let result = result.and_then(|data| {
                crate::shot_wallpaper_preference::remember(&incodex_core::paths::user_root())?;
                Ok(data)
            });
            if let Ok(data) = &result {
                self.ready.insert(wallpaper_id.clone(), data.clone());
            }
            for id in self.pending.remove(&wallpaper_id).unwrap_or_default() {
                replies.push(match &result {
                    Ok(data) => json!({"id":id,"ok":true,"dataUrl":data}),
                    Err(_) => {
                        json!({"id":id,"ok":false,"error":"system wallpaper acquisition failed"})
                    }
                });
            }
        }
        replies
    }
}
#[cfg(target_os = "macos")]
impl Drop for PairHost {
    fn drop(&mut self) {
        self.alive
            .store(false, std::sync::atomic::Ordering::Release);
        for (_, worker) in self.workers.drain() {
            let _ = worker.join();
        }
    }
}

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
    host: &mut Host,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    for reply in host.pair.completed() {
        resolve(socket, next_id, reply)?;
    }
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
    #[cfg(target_os = "macos")]
    {
        let result = match request {
            Request::List { id } => Some(
                json!({"id":id,"ok":true,"entries":crate::macos_system_wallpapers::entries(host.pair.major)}),
            ),
            // 恢复只恢复入口，不会在用户未点击时发起下载。
            Request::Restore { id } => {
                let entries =
                    if crate::shot_wallpaper_preference::enabled(&incodex_core::paths::user_root())
                    {
                        crate::macos_system_wallpapers::entries(host.pair.major)
                    } else {
                        json!([])
                    };
                Some(json!({"id":id,"ok":true,"entries":entries}))
            }
            Request::Load { id, wallpaper_id } => host.pair.load(id, wallpaper_id),
        };
        if let Some(result) = result {
            resolve(socket, next_id, result)?;
        }
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let library = &mut host.library;
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
            Request::Load { id, wallpaper_id } => {
                match library.load(&wallpaper_id).and_then(|data| {
                    crate::shot_wallpaper_preference::remember(&root)?;
                    Ok(data)
                }) {
                    Ok(data_url) => json!({ "id": id, "ok": true, "dataUrl": data_url }),
                    Err(_) => {
                        json!({ "id": id, "ok": false, "error": "system wallpaper image unavailable" })
                    }
                }
            }
        };
        resolve(socket, next_id, result)
    }
}

fn resolve(
    socket: &mut WebSocket<TcpStream>,
    next_id: &mut u64,
    result: Value,
) -> Result<(), String> {
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
