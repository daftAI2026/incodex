//! Installation-bound requests for the host's Accessibility setup flow.

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
}

/// Bind a CLI-owned request without starting a second in-app guide.
///
/// The transaction journal remains the authority for the installed app.  The
/// marker is only a durable, installation-bound handoff to the Runtime; this
/// function never changes the app bundle or the host's TCC database.
pub(crate) fn request_setup(root: &Path, app: &Path, install_id: &str) -> Result<String, String> {
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
    validate_existing_marker(&marker, install_id, &canonical_app)?;

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
        })
        .map_err(|error| format!("cannot encode Accessibility setup request: {error}"))?
    );
    write_marker_atomically(&marker, body.as_bytes(), install_id, &canonical_app)?;
    Ok(request_id)
}

/// Update only the request owned by this CLI operation. The caller keeps the
/// target transaction lock while the one-shot guide is alive.
pub(crate) fn finish_cli_setup(
    root: &Path,
    app: &Path,
    install_id: &str,
    request_id: &str,
    state: &str,
) -> Result<(), String> {
    if !root.is_absolute() || !app.is_absolute() || !is_uuid(install_id) {
        return Err("invalid CLI Accessibility request identity".into());
    }
    if !matches!(state, "granted" | "deferred" | "error" | "awaiting-user") {
        return Err("invalid CLI Accessibility request state".into());
    }
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
        .custom_flags(libc::O_NOFOLLOW)
        .open(&marker)
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_MARKER_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_MARKER_BYTES {
        return Err("CLI Accessibility request is oversized".into());
    }
    let mut value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if value["requestId"].as_str() != Some(request_id)
        || value["presentationOwner"].as_str() != Some("cli")
    {
        return Err("CLI Accessibility request was replaced".into());
    }
    value["state"] = state.into();
    value["updatedAtMs"] = unix_now_ms()?.into();
    let body = format!(
        "{}\n",
        serde_json::to_string(&value).map_err(|error| error.to_string())?
    );
    write_marker_atomically(&marker, body.as_bytes(), install_id, &canonical_app)
}

fn new_request_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| format!("cannot create Accessibility setup request identity: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
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
    let metadata = match fs::symlink_metadata(marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
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
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(marker).map_err(|error| {
        format!(
            "cannot read Accessibility setup marker {}: {error}",
            marker.display()
        )
    })?;
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
    Ok(())
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
