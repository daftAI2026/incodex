/*
 * [INPUT]: 依赖安装事务的存活证明、私有目录与所有权锁。
 * [OUTPUT]: 提供单 owner、续接 CAS 与同代 Skip 去重的短锁请求。
 * 每次更新重读旧 granted；完成记录不缓存当前权限。
 * 机器交接先释放 owner 再通知；用户终态提交前保留 owner。
 * [POS]: 复用既有 AX marker 绑定双权限引导；不改宿主或 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
//! Installation-bound requests for the host's permission setup flow.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::validate_committed_live_snapshot;
use serde::{Deserialize, Serialize};

const MARKER_NAME: &str = "accessibility-setup.json";
const MARKER_SCHEMA_VERSION: u32 = 1;
const MARKER_MODE: u32 = 0o600;
const PRIVATE_DIR_WRITE_BITS: u32 = 0o022;
const MAX_MARKER_BYTES: u64 = 8 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExistingMarker {
    schema_version: u32,
    install_id: String,
    app_path: String,
    requested_at_ms: u64,
    state: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingMarker<'a> {
    schema_version: u32,
    install_id: &'a str,
    app_path: &'a str,
    requested_at_ms: u64,
    request_id: String,
    state: &'static str,
    presentation_owner: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    update_runtime_sha256: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resume_reason: Option<ResumeReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ResumeReason {
    PostUpdate,
    PermissionSetup,
    ScreenAuthorization,
}

// 续接只记录等待原因，不缓存授权，也不创建新请求。
pub(crate) fn mark_continuation(
    root: &Path,
    app: &Path,
    install_id: &str,
    request_id: &str,
    reason: ResumeReason,
) -> Result<(), String> {
    with_cli_setup(root, app, install_id, request_id, |value| {
        if !matches!(value["state"].as_str(), Some("pending" | "awaiting-user")) {
            return Err("permission request is no longer resumable".into());
        }
        value["state"] = "awaiting-user".into();
        value["resumeReason"] = serde_json::to_value(reason).map_err(|e| e.to_string())?;
        Ok(())
    })
}

pub(crate) fn resume_setup(
    root: &Path,
    app: &Path,
    install_id: &str,
    request_id: &str,
) -> Result<ResumeReason, String> {
    with_cli_setup(root, app, install_id, request_id, |value| {
        if !matches!(value["state"].as_str(), Some("pending" | "awaiting-user")) {
            return Err("permission request is no longer resumable".into());
        }
        serde_json::from_value(value["resumeReason"].clone())
            .map_err(|_| "permission continuation reason is missing or unsupported".into())
    })
}

pub(crate) fn is_permission_request_id(value: &str) -> bool {
    (is_uuid(value)
        && value.as_bytes()[14] == b'4'
        && matches!(
            value.as_bytes()[19].to_ascii_lowercase(),
            b'8' | b'9' | b'a' | b'b'
        ))
        || (value.len() == 32
            && value
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')))
}

/// Bind a CLI-owned request without starting a second in-app guide.
/// 同一 App 的卡片所有者。实现须与 App 危险事务锁分开。
pub(crate) fn acquire_guide_owner(
    root: &Path,
    app: &Path,
) -> Result<incodex_transaction::TargetLock, String> {
    let target = root
        .join("permission-guides")
        .join(crate::macos_update_assets::sha256_hex(
            incodex_core::canonical_path(app)
                .to_string_lossy()
                .as_bytes(),
        ));
    incodex_transaction::acquire_target_lock(root, &target, "permission-guide", None)
}

///
/// The transaction journal remains the authority for the installed app.  The
/// marker is only a durable, installation-bound handoff to the Runtime; this
/// function never changes the app bundle or the host's TCC database.
pub(crate) fn request_setup(root: &Path, app: &Path, install_id: &str) -> Result<String, String> {
    request_setup_inner(root, app, install_id, None)?
        .ok_or_else(|| "explicit permission request was unexpectedly suppressed".into())
}

// ---- 更新重读权限；同代 Skip 保留人的决定，pending 保留原请求 ----
pub(crate) fn request_update_setup(
    root: &Path,
    app: &Path,
    install_id: &str,
    runtime_sha256: &str,
) -> Result<Option<String>, String> {
    if !crate::macos_update_assets::is_sha256(runtime_sha256) {
        return Err("invalid permission update Runtime identity".into());
    }
    request_setup_inner(root, app, install_id, Some(runtime_sha256))
}

fn request_setup_inner(
    root: &Path,
    app: &Path,
    install_id: &str,
    runtime_sha256: Option<&str>,
) -> Result<Option<String>, String> {
    if !root.is_absolute() {
        return Err(format!(
            "Incodex state root must be absolute: {}",
            root.display()
        ));
    }
    if !app.is_absolute() {
        return Err(format!(
            "Accessibility setup app path must be absolute: {}",
            app.display()
        ));
    }
    if !is_uuid(install_id) {
        return Err("install id must be an RFC 4122 UUID".into());
    }

    let app_meta = fs::symlink_metadata(app).map_err(|error| {
        format!(
            "cannot inspect Accessibility setup app {}: {error}",
            app.display()
        )
    })?;
    if app_meta.file_type().is_symlink() {
        return Err(format!(
            "Accessibility setup app is a symlink: {}",
            app.display()
        ));
    }
    if !app_meta.file_type().is_dir() {
        return Err(format!(
            "Accessibility setup app is not a directory: {}",
            app.display()
        ));
    }
    let canonical_app = fs::canonicalize(app).map_err(|error| {
        format!(
            "cannot canonicalize Accessibility setup app {}: {error}",
            app.display()
        )
    })?;

    let _mutation = incodex_transaction::acquire_target_lock(
        root,
        app,
        "permission-request",
        Some(install_id),
    )?;
    let transactions = root.join("transactions");
    let transaction = transactions.join(install_id);
    ensure_private_directory(root, "Incodex state root")?;
    ensure_private_directory(&transactions, "transaction root")?;
    ensure_private_directory(&transaction, "transaction directory")?;

    // A marker cannot establish that an app was installed.  Require the
    // committed journal and its sealed live-tree proof before writing one.
    validate_committed_live_snapshot(root, install_id, &canonical_app).map_err(|error| {
        format!(
            "cannot bind Accessibility setup request to committed transaction {install_id}: {error}"
        )
    })?;

    let marker = transaction.join(MARKER_NAME);
    let existing = read_existing_marker(&marker, install_id, &canonical_app)?;
    if let (Some(runtime), Some(mut value)) = (runtime_sha256, existing) {
        if value
            .get("presentationOwner")
            .is_some_and(|owner| owner != "cli")
        {
            return Err("permission update cannot replace an unknown guide owner".into());
        }
        if value.get("updateRuntimeSha256").is_some_and(|hash| {
            hash.as_str()
                .is_none_or(|hash| !crate::macos_update_assets::is_sha256(hash))
        }) {
            return Err("invalid permission request Runtime identity".into());
        }
        if value["presentationOwner"] == "cli" {
            let state = value["state"]
                .as_str()
                .ok_or("invalid permission request state")?;
            if value["updateRuntimeSha256"] == runtime && state == "deferred" {
                return Ok(None);
            }
            if matches!(state, "pending" | "awaiting-user") {
                let request = value["requestId"]
                    .as_str()
                    .filter(|id| is_permission_request_id(id))
                    .ok_or("invalid pending permission request identity")?
                    .to_owned();
                let before = value.clone();
                let reason = value
                    .get("resumeReason")
                    .map(|raw| {
                        serde_json::from_value::<ResumeReason>(raw.clone())
                            .map_err(|_| "unsupported permission continuation reason")
                    })
                    .transpose()?
                    .unwrap_or(ResumeReason::PostUpdate);
                value["updateRuntimeSha256"] = runtime.into();
                value["resumeReason"] = serde_json::to_value(reason).map_err(|e| e.to_string())?;
                if before != value {
                    value["updatedAtMs"] = unix_now_ms()?.into();
                    let body = format!(
                        "{}\n",
                        serde_json::to_string(&value).map_err(|e| e.to_string())?
                    );
                    write_marker_atomically(&marker, body.as_bytes(), install_id, &canonical_app)?;
                }
                return Ok(Some(request));
            }
        }
    }

    let requested_at_ms = unix_now_ms()?;
    let request_id = new_request_id()?;
    let app_path = canonical_app.to_string_lossy().into_owned();
    let body = format!(
        "{}\n",
        serde_json::to_string(&PendingMarker {
            schema_version: MARKER_SCHEMA_VERSION,
            install_id,
            app_path: &app_path,
            requested_at_ms,
            request_id: request_id.clone(),
            state: "pending",
            presentation_owner: "cli",
            update_runtime_sha256: runtime_sha256,
            resume_reason: runtime_sha256.map(|_| ResumeReason::PostUpdate),
        })
        .map_err(|error| format!("cannot encode Accessibility setup request: {error}"))?
    );
    write_marker_atomically(&marker, body.as_bytes(), install_id, &canonical_app)?;
    Ok(Some(request_id))
}

// 独立 seam 让隔离测试使用真实 owner 锁检验写入先后。
fn finish_with_owner<T>(
    owner: incodex_transaction::TargetLock,
    machine_pending: bool,
    finish: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    if machine_pending {
        drop(owner);
        finish()
    } else {
        let _owner = owner;
        finish()
    }
}

pub(crate) fn finish_owned_setup(
    root: &Path,
    app: &Path,
    install_id: &str,
    request_id: &str,
    owner: incodex_transaction::TargetLock,
    state: &str,
) -> Result<(), String> {
    finish_with_owner(owner, state == "pending", || {
        finish_cli_setup(root, app, install_id, request_id, state)
    })
}

/// 在短 App 锁内只更新原请求；终态禁止回退。
/// 生产调用方经 finish_owned_setup 区分机器交接与用户终态。
pub(crate) fn finish_cli_setup(
    root: &Path,
    app: &Path,
    install_id: &str,
    request_id: &str,
    state: &str,
) -> Result<(), String> {
    if !matches!(
        state,
        "granted" | "deferred" | "error" | "awaiting-user" | "pending"
    ) {
        return Err("invalid CLI Accessibility request state".into());
    }
    with_cli_setup(root, app, install_id, request_id, |value| {
        let current = value["state"]
            .as_str()
            .ok_or("invalid permission request state")?;
        if matches!(current, "granted" | "deferred" | "error") && current != state {
            return Err("permission request already has a terminal conclusion".into());
        }
        if state == "pending" {
            let _: ResumeReason = serde_json::from_value(value["resumeReason"].clone())
                .map_err(|_| "machine continuation requires a supported reason")?;
        }
        value["state"] = state.into();
        if !matches!(state, "awaiting-user" | "pending") {
            value
                .as_object_mut()
                .ok_or("invalid permission request")?
                .remove("resumeReason");
        }
        Ok(())
    })
}

// ---- 短 App 锁内校验原请求；同态读写不发布新的 watcher 边沿 ----
fn with_cli_setup<T>(
    root: &Path,
    app: &Path,
    install_id: &str,
    request_id: &str,
    edit: impl FnOnce(&mut serde_json::Value) -> Result<T, String>,
) -> Result<T, String> {
    if !root.is_absolute()
        || !app.is_absolute()
        || !is_uuid(install_id)
        || !is_permission_request_id(request_id)
    {
        return Err("invalid CLI Accessibility request identity".into());
    }
    let _mutation =
        incodex_transaction::acquire_target_lock(root, app, "permission-result", Some(install_id))?;
    let transaction = root.join("transactions").join(install_id);
    for directory in [
        root.to_path_buf(),
        root.join("transactions"),
        transaction.clone(),
    ] {
        ensure_private_directory(&directory, "CLI Accessibility request directory")?;
    }
    let canonical_app = fs::canonicalize(app).map_err(|error| error.to_string())?;
    validate_committed_live_snapshot(root, install_id, &canonical_app)
        .map_err(|error| error.to_string())?;
    let marker = transaction.join(MARKER_NAME);
    validate_existing_marker(&marker, install_id, &canonical_app)?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&marker)
        .map_err(|e| e.to_string())?;
    let stat = file.metadata().map_err(|e| e.to_string())?;
    if !stat.is_file()
        || stat.uid() != unsafe { libc::geteuid() }
        || stat.permissions().mode() & 0o077 != 0
        || stat.len() > MAX_MARKER_BYTES
    {
        return Err("unsafe CLI permission request".into());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_MARKER_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_MARKER_BYTES {
        return Err("CLI Accessibility request is oversized".into());
    }
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let existing: ExistingMarker =
        serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    if existing.schema_version != MARKER_SCHEMA_VERSION
        || existing.install_id != install_id
        || existing.app_path != canonical_app.to_string_lossy()
        || !matches!(
            existing.state.as_str(),
            "pending" | "awaiting-user" | "granted" | "deferred" | "error"
        )
        || value["requestId"].as_str() != Some(request_id)
        || value["presentationOwner"].as_str() != Some("cli")
    {
        return Err("CLI Accessibility request was replaced".into());
    }
    let before = value.clone();
    let result = edit(&mut value)?;
    if value != before {
        value["updatedAtMs"] = unix_now_ms()?.into();
        let body = format!(
            "{}\n",
            serde_json::to_string(&value).map_err(|e| e.to_string())?
        );
        write_marker_atomically(&marker, body.as_bytes(), install_id, &canonical_app)?;
    }
    Ok(result)
}

fn new_request_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| format!("cannot create Accessibility setup request identity: {error}"))?;
    // 请求 ID 遵循已安装 responder 的 UUID 协议，不改变 installId。
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

fn ensure_private_directory(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {label} {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{label} is a symlink: {}", path.display()));
    }
    if !metadata.file_type().is_dir() {
        return Err(format!("{label} is not a directory: {}", path.display()));
    }
    let uid = unsafe { libc::geteuid() as u32 };
    if metadata.uid() != uid {
        return Err(format!(
            "{label} is not owned by the current user: {}",
            path.display()
        ));
    }
    if metadata.permissions().mode() & PRIVATE_DIR_WRITE_BITS != 0 {
        return Err(format!(
            "{label} is group/world writable: {}",
            path.display()
        ));
    }
    Ok(())
}

fn validate_existing_marker(
    marker: &Path,
    install_id: &str,
    canonical_app: &Path,
) -> Result<(), String> {
    read_existing_marker(marker, install_id, canonical_app).map(|_| ())
}

fn read_existing_marker(
    marker: &Path,
    install_id: &str,
    canonical_app: &Path,
) -> Result<Option<serde_json::Value>, String> {
    let metadata = match fs::symlink_metadata(marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot inspect Accessibility setup marker {}: {error}",
                marker.display()
            ))
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "Accessibility setup marker is a symlink: {}",
            marker.display()
        ));
    }
    if !metadata.file_type().is_file() {
        return Err(format!(
            "Accessibility setup marker is not a regular file: {}",
            marker.display()
        ));
    }
    let uid = unsafe { libc::geteuid() as u32 };
    if metadata.uid() != uid {
        return Err(format!(
            "Accessibility setup marker is not owned by the current user: {}",
            marker.display()
        ));
    }
    if metadata.permissions().mode() & PRIVATE_DIR_WRITE_BITS != 0 {
        return Err(format!(
            "Accessibility setup marker is group/world writable: {}",
            marker.display()
        ));
    }
    if metadata.len() > MAX_MARKER_BYTES {
        return Err(format!(
            "Accessibility setup marker is too large: {}",
            marker.display()
        ));
    }

    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let mut file = options.open(marker).map_err(|error| {
        format!(
            "cannot read Accessibility setup marker {}: {error}",
            marker.display()
        )
    })?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    if !opened.is_file()
        || opened.uid() != uid
        || opened.permissions().mode() & PRIVATE_DIR_WRITE_BITS != 0
        || opened.len() > MAX_MARKER_BYTES
        || opened.dev() != metadata.dev()
        || opened.ino() != metadata.ino()
    {
        return Err("permission marker changed while opening".into());
    }
    let mut body = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_MARKER_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| {
            format!(
                "cannot read Accessibility setup marker {}: {error}",
                marker.display()
            )
        })?;
    if body.len() as u64 > MAX_MARKER_BYTES {
        return Err(format!(
            "Accessibility setup marker is too large: {}",
            marker.display()
        ));
    }
    let existing: ExistingMarker = serde_json::from_slice(&body).map_err(|error| {
        format!(
            "malformed Accessibility setup marker {}: {error}",
            marker.display()
        )
    })?;
    if existing.schema_version != MARKER_SCHEMA_VERSION {
        return Err(format!(
            "unsupported Accessibility setup marker schema in {}",
            marker.display()
        ));
    }
    if existing.install_id != install_id {
        return Err(format!(
            "Accessibility setup marker install id does not match transaction: {}",
            marker.display()
        ));
    }
    if !Path::new(&existing.app_path).is_absolute() {
        return Err(format!(
            "Accessibility setup marker app path is not absolute: {}",
            marker.display()
        ));
    }
    let marker_app = fs::canonicalize(&existing.app_path).map_err(|error| {
        format!(
            "cannot canonicalize Accessibility setup marker app path {}: {error}",
            existing.app_path
        )
    })?;
    if marker_app != canonical_app || existing.app_path != canonical_app.to_string_lossy().as_ref()
    {
        return Err(format!(
            "Accessibility setup marker app path does not match the transaction: {}",
            marker.display()
        ));
    }
    if !matches!(
        existing.state.as_str(),
        "pending" | "granted" | "deferred" | "error" | "awaiting-user"
    ) {
        return Err(format!(
            "unknown Accessibility setup marker state in {}",
            marker.display()
        ));
    }
    // `requestedAtMs` is required even when Runtime has added no optional
    // result fields yet.  Deserialization above also rejects non-integers.
    let _ = existing.requested_at_ms;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| e.to_string())
}

fn write_marker_atomically(
    marker: &Path,
    body: &[u8],
    install_id: &str,
    canonical_app: &Path,
) -> Result<(), String> {
    let parent = marker.parent().ok_or_else(|| {
        format!(
            "Accessibility setup marker has no parent: {}",
            marker.display()
        )
    })?;
    let temporary = temporary_marker_path(parent, install_id);
    let write_result = (|| {
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(MARKER_MODE)
            .custom_flags(libc::O_NOFOLLOW);
        let mut file = options.open(&temporary).map_err(|error| {
            format!(
                "cannot create temporary Accessibility setup marker {}: {error}",
                temporary.display()
            )
        })?;
        file.write_all(body).map_err(|error| {
            format!(
                "cannot write temporary Accessibility setup marker {}: {error}",
                temporary.display()
            )
        })?;
        file.sync_data().map_err(|error| {
            format!(
                "cannot flush temporary Accessibility setup marker {}: {error}",
                temporary.display()
            )
        })?;
        let metadata = file.metadata().map_err(|error| {
            format!(
                "cannot inspect temporary Accessibility setup marker {}: {error}",
                temporary.display()
            )
        })?;
        if metadata.permissions().mode() & 0o777 != MARKER_MODE {
            return Err(format!(
                "temporary Accessibility setup marker has unsafe permissions: {}",
                temporary.display()
            ));
        }
        drop(file);

        // Recheck before replacing a previously validated marker.  In
        // particular, never follow a marker symlink that appeared after the
        // initial read.
        validate_existing_marker(marker, install_id, canonical_app)?;
        fs::rename(&temporary, marker).map_err(|error| {
            format!(
                "cannot publish Accessibility setup marker {}: {error}",
                marker.display()
            )
        })?;
        sync_directory(parent)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

fn temporary_marker_path(parent: &Path, install_id: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    parent.join(format!(
        ".{MARKER_NAME}.{install_id}.{}.{}.tmp",
        std::process::id(),
        nanos
    ))
}

fn sync_directory(path: &Path) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY);
    let directory = options
        .open(path)
        .map_err(|error| format!("cannot open directory {} for sync: {error}", path.display()))?;
    directory
        .sync_all()
        .map_err(|error| format!("cannot flush directory {}: {error}", path.display()))
}

fn unix_now_ms() -> Result<u64, String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before the Unix epoch: {error}"))?;
    u64::try_from(duration.as_millis()).map_err(|_| "system clock value is too large".into())
}

fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
#[path = "accessibility_setup_tests.rs"]
mod tests;
