/*
 * [INPUT]: 依赖安装请求、精确主进程身份和 Runtime preflight 回传。
 * [OUTPUT]: 读取 UUIDv4 或历史 32hex 请求；只消费绑定新 nonce 的录屏状态。
 * [POS]: CLI 到现有主 Runtime 的一次性状态适配，不代查终端权限。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use crate::accessibility_guide_host::ScreenPermissionStatus;
use serde::Serialize;
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const REQUEST_NAME: &str = "permission-probe-request.json";
const RESPONSE_NAME: &str = "permission-probe-response.json";
const MAX_BYTES: u64 = 8192;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScreenRequest {
    pub(crate) schema_version: u32,
    pub(crate) nonce: String,
    pub(crate) install_id: String,
    pub(crate) request_id: String,
    pub(crate) app_path: String,
    pub(crate) pid: i32,
    pub(crate) process_start_identity: String,
    pub(crate) requested_at_ms: u64,
}

pub(crate) struct InstalledScreenProbe {
    root: PathBuf,
    directory: PathBuf,
    install_id: String,
    request_id: String,
    last_nonce: Option<String>,
}

fn private_file(path: &Path) -> Option<Value> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let stat = file.metadata().ok()?;
    if !stat.is_file()
        || stat.uid() != unsafe { libc::getuid() }
        || stat.permissions().mode() & 0o077 != 0
        || stat.len() > MAX_BYTES
    {
        return None;
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

impl InstalledScreenProbe {
    pub(crate) fn for_app(root: &Path, app: &Path) -> Option<Self> {
        let archive = incodex_asar::Archive::open(app.join("Contents/Resources/app.asar")).ok()?;
        let install_id = archive.read_package_main().ok()?.install_id?;
        if !uuid(&install_id) {
            return None;
        }
        let directory = root.join("transactions").join(&install_id);
        let marker = private_file(&directory.join("accessibility-setup.json"))?;
        let request_id = marker["requestId"].as_str()?.to_string();
        let canonical = uuid(&request_id)
            && request_id.as_bytes()[14] == b'4'
            && matches!(
                request_id.as_bytes()[19].to_ascii_lowercase(),
                b'8' | b'9' | b'a' | b'b'
            );
        let legacy = request_id.len() == 32
            && request_id
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
        // 旧 marker 保留可读。旧 responder 的交接须由正常 reentry 生成 UUIDv4。
        if !canonical && !legacy {
            return None;
        }
        let probe = Self {
            root: root.into(),
            directory,
            install_id,
            request_id,
            last_nonce: None,
        };
        probe.marker(app)?;
        Some(probe)
    }

    fn directories(&self) -> Option<Vec<(u64, u64)>> {
        [
            self.root.clone(),
            self.root.join("transactions"),
            self.directory.clone(),
        ]
        .iter()
        .map(|path| {
            let stat = fs::symlink_metadata(path).ok()?;
            (stat.is_dir()
                && !stat.file_type().is_symlink()
                && stat.uid() == unsafe { libc::getuid() }
                && stat.permissions().mode() & 0o077 == 0)
                .then_some((stat.dev(), stat.ino()))
        })
        .collect()
    }

    fn marker(&self, app: &Path) -> Option<Value> {
        self.directories()?;
        let marker = private_file(&self.directory.join("accessibility-setup.json"))?;
        (marker["schemaVersion"] == 1
            && marker["installId"] == self.install_id
            && marker["requestId"] == self.request_id
            && marker["appPath"].as_str() == app.to_str()
            && marker["presentationOwner"] == "cli"
            && matches!(marker["state"].as_str(), Some("pending" | "awaiting-user")))
        .then_some(marker)
    }

    pub(crate) fn probe(&mut self, app: &Path) -> ScreenPermissionStatus {
        self.read_status(app)
            .unwrap_or(ScreenPermissionStatus::Unknown)
    }

    fn read_status(&mut self, app: &Path) -> Option<ScreenPermissionStatus> {
        let ancestry = self.directories()?;
        self.marker(app)?;
        let report = incodex_macos::inspect_accessibility_for_app(app);
        let pid = report.pid?;
        let identity = incodex_core::session::process_start_identity(pid)?;
        let mut random = [0_u8; 16];
        File::open("/dev/urandom")
            .ok()?
            .read_exact(&mut random)
            .ok()?;
        let nonce: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let request = ScreenRequest {
            schema_version: 1,
            nonce: nonce.clone(),
            install_id: self.install_id.clone(),
            request_id: self.request_id.clone(),
            app_path: app.to_str()?.into(),
            pid,
            process_start_identity: identity.clone(),
            requested_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_millis()
                .try_into()
                .ok()?,
        };
        let temporary = self
            .directory
            .join(format!(".permission-probe-request.{nonce}.tmp"));
        let published = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
                .ok()?;
            file.write_all(&serde_json::to_vec(&request).ok()?).ok()?;
            file.sync_all().ok()?;
            if self.directories()? != ancestry {
                return None;
            }
            self.marker(app)?;
            fs::rename(&temporary, self.directory.join(REQUEST_NAME)).ok()
        })();
        let _ = fs::remove_file(&temporary);
        published?;
        self.last_nonce = Some(nonce);
        let deadline = Instant::now() + Duration::from_millis(600);
        while Instant::now() < deadline {
            if let Some(value) = private_file(&self.directory.join(RESPONSE_NAME)) {
                if value["nonce"].as_str() == Some(&request.nonce) {
                    self.marker(app)?;
                    let live = incodex_core::session::process_start_identity(pid)?;
                    if live != identity || self.directories()? != ancestry {
                        return None;
                    }
                    let after = incodex_macos::inspect_accessibility_for_app(app);
                    if after.pid != Some(pid) {
                        return None;
                    }
                    return Some(decode_response(&value, &request));
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for InstalledScreenProbe {
    fn drop(&mut self) {
        let Some(nonce) = &self.last_nonce else {
            return;
        };
        if self.directories().is_none() {
            return;
        }
        for name in [REQUEST_NAME, RESPONSE_NAME] {
            let file = self.directory.join(name);
            if private_file(&file)
                .is_some_and(|value| value["nonce"].as_str() == Some(nonce.as_str()))
            {
                let _ = fs::remove_file(file);
            }
        }
    }
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

pub(crate) fn decode_response(value: &Value, request: &ScreenRequest) -> ScreenPermissionStatus {
    let Ok(expected) = serde_json::to_value(request) else {
        return ScreenPermissionStatus::Unknown;
    };
    if expected
        .as_object()
        .is_none_or(|fields| fields.iter().any(|(key, field)| &value[key] != field))
        || value["observedAtMs"].as_u64().is_none_or(|time| {
            time < request.requested_at_ms || time - request.requested_at_ms > 5000
        })
    {
        return ScreenPermissionStatus::Unknown;
    }
    match (value["rawStatus"].as_str(), value["status"].as_str()) {
        (Some("granted"), Some("granted")) => ScreenPermissionStatus::Granted,
        (Some("denied" | "not-determined" | "restricted"), Some("not-granted")) => {
            ScreenPermissionStatus::NotGranted
        }
        _ => ScreenPermissionStatus::Unknown,
    }
}
