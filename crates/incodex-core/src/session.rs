use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const SESSIONS_NAME: &str = "sessions";
const LOGS_NAME: &str = "logs";
const OWNER_NAME: &str = "owner.json";
const LOCK_NAME: &str = "lock";
const SETTINGS_FILES: &[&str] = &["auth.json", "config.toml"];
const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;

mod window_state;
pub use window_state::{seed_window_state, seed_window_state_with_geometry, WindowGeometry};

#[derive(Debug, Clone)]
pub struct SessionHome {
    pub session_id: String,
    pub root: PathBuf,
    pub home: PathBuf,
    pub chromium: PathBuf,
    pub ino: u64,
    pub dev: u64,
}

#[derive(Debug, Clone)]
pub struct BurnExpected<'a> {
    pub user_root: &'a Path,
    pub session_id: Option<&'a str>,
    pub ino: Option<u64>,
    pub dev: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOwnerSnapshot {
    pub pid: i32,
    pub process_start_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProcessProbe {
    Live(String),
    Dead,
    Unknown,
}

/// 读取与 Electron Runtime owner 相同格式的 macOS process-start identity。
pub fn process_start_identity(pid: i32) -> Option<String> {
    match probe_process(pid) {
        ProcessProbe::Live(identity) => Some(identity),
        ProcessProbe::Dead | ProcessProbe::Unknown => None,
    }
}

fn probe_process(pid: i32) -> ProcessProbe {
    if pid <= 0 {
        return ProcessProbe::Dead;
    }
    if unsafe { libc::kill(pid, 0) } != 0 {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => ProcessProbe::Dead,
            _ => ProcessProbe::Unknown,
        };
    }
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .env("LC_ALL", "C")
        .output()
        .ok();
    let Some(output) = output else {
        return ProcessProbe::Unknown;
    };
    if !output.status.success() {
        return ProcessProbe::Unknown;
    }
    let identity = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !is_canonical_process_start_identity(&identity) {
        ProcessProbe::Unknown
    } else {
        ProcessProbe::Live(identity)
    }
}

pub fn create_session_home(
    user_root: &Path,
    target_id: Option<&str>,
    pid: i32,
    source_home: &str,
) -> Result<SessionHome, String> {
    create_session_home_inner(user_root, target_id, pid, source_home, false)
}

/// 为 `open` 创建带有短暂 owner handoff 保护的 session。
pub fn create_session_home_for_open(
    user_root: &Path,
    target_id: Option<&str>,
    pid: i32,
    source_home: &str,
) -> Result<SessionHome, String> {
    create_session_home_inner(user_root, target_id, pid, source_home, true)
}

fn create_session_home_inner(
    user_root: &Path,
    target_id: Option<&str>,
    pid: i32,
    source_home: &str,
    handoff_pending: bool,
) -> Result<SessionHome, String> {
    let parent = user_root
        .parent()
        .ok_or("session user root has no parent")?;
    ensure_private_dir(user_root, parent)?;
    ensure_private_dir(&user_root.join(LOGS_NAME), user_root)?;
    let session_parent = sessions_base(user_root, target_id)?;
    let root = mkdtemp(&session_parent)?;
    chmod(&root, DIR_MODE)?;
    let root_stat = assert_not_symlink(&root, "session root")?;
    let root_stat =
        root_stat.ok_or_else(|| format!("session root is not a directory: {}", root.display()))?;
    if !root_stat.is_dir() {
        return Err(format!(
            "session root is not a directory: {}",
            root.display()
        ));
    }
    let real_root = real_existing(&root)?;
    assert_inside_parent(&real_root, &session_parent)?;
    let home = ensure_private_dir(&real_root.join("codex-home"), &real_root)?;
    let chromium = ensure_private_dir(&real_root.join("chromium"), &real_root)?;
    let session_id = file_name(&real_root)?;
    write_private_file(
        &real_root.join(LOCK_NAME),
        format!("{pid}\n").as_bytes(),
        true,
    )?;
    let process_start_identity = process_start_identity(pid);
    let mut owner = serde_json::json!({
        "sessionId": session_id,
        "targetId": target_id.unwrap_or(""),
        "pid": pid,
        "sourceHome": source_home,
        "createdAt": unix_now(),
        "ino": root_stat.ino(),
        "dev": root_stat.dev(),
    });
    if let Some(identity) = process_start_identity.as_deref() {
        owner["processStartIdentity"] = serde_json::json!(identity);
    }
    if handoff_pending {
        owner["handoffPending"] = serde_json::json!(true);
    }
    write_private_file(
        &real_root.join(OWNER_NAME),
        format!("{owner}\n").as_bytes(),
        true,
    )?;
    Ok(SessionHome {
        session_id,
        root: real_root,
        home,
        chromium,
        ino: root_stat.ino(),
        dev: root_stat.dev(),
    })
}

/// 将 session owner 从 launcher 原子移交给已经 spawn 的 child。
pub fn handoff_session_owner(
    session_root: &Path,
    pid: i32,
) -> Result<SessionOwnerSnapshot, String> {
    let identity = process_start_identity(pid).ok_or_else(|| {
        format!("cannot hand off session owner: process identity unavailable for pid {pid}")
    })?;
    let root_stats = assert_not_symlink(session_root, "session root")?
        .ok_or_else(|| format!("session root missing: {}", session_root.display()))?;
    if !root_stats.is_dir() {
        return Err(format!(
            "session root is not a directory: {}",
            session_root.display()
        ));
    }
    let owner_path = session_root.join(OWNER_NAME);
    let owner_stats = assert_not_symlink(&owner_path, "owner manifest")?
        .ok_or_else(|| format!("session owner missing: {}", owner_path.display()))?;
    if !owner_stats.is_file() {
        return Err(format!(
            "session owner is not a file: {}",
            owner_path.display()
        ));
    }
    let mut owner = read_owner_manifest(&owner_path)?;
    if owner.get("ino").and_then(serde_json::Value::as_u64) != Some(root_stats.ino())
        || owner.get("dev").and_then(serde_json::Value::as_u64) != Some(root_stats.dev())
    {
        return Err("session root identity changed; refusing owner handoff".into());
    }
    owner["pid"] = serde_json::json!(pid);
    owner["processStartIdentity"] = serde_json::json!(&identity);
    if owner
        .get("handoffPending")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        owner["handoffPending"] = serde_json::json!(false);
    }
    write_private_file_atomic(&owner_path, format!("{owner}\n").as_bytes())?;
    Ok(SessionOwnerSnapshot {
        pid,
        process_start_identity: identity,
    })
}

pub fn session_owner_snapshot(session_root: &Path) -> Result<Option<SessionOwnerSnapshot>, String> {
    let owner_path = session_root.join(OWNER_NAME);
    let Some(stats) = assert_not_symlink(&owner_path, "owner manifest")? else {
        return Ok(None);
    };
    if !stats.is_file() {
        return Err(format!(
            "session owner is not a file: {}",
            owner_path.display()
        ));
    }
    let owner = read_owner_manifest(&owner_path)?;
    let Some(pid) = owner_pid(&owner) else {
        return Ok(None);
    };
    let Some(process_start_identity) =
        owner_start_identity(&owner).filter(|value| is_canonical_process_start_identity(value))
    else {
        return Ok(None);
    };
    Ok(Some(SessionOwnerSnapshot {
        pid,
        process_start_identity: process_start_identity.to_string(),
    }))
}

fn unix_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn copy_settings(home: &Path, source_home: &Path) -> Result<usize, String> {
    copy_settings_with_window_geometry(home, source_home, None)
}

pub fn copy_settings_with_window_geometry(
    home: &Path,
    source_home: &Path,
    live_geometry: Option<WindowGeometry>,
) -> Result<usize, String> {
    let home_stat = assert_not_symlink(home, "session home")?;
    if !home_stat.map(|s| s.is_dir()).unwrap_or(false) {
        return Err(format!("session home missing: {}", home.display()));
    }
    let mut copied = 0;
    for name in SETTINGS_FILES {
        let src = source_home.join(name);
        if assert_not_symlink(&src, "source setting")?.is_none() {
            continue;
        }
        exclusive_copy_file(&src, &home.join(name))?;
        copied += 1;
    }
    seed_window_state_with_geometry(home, source_home, live_geometry)?;
    Ok(copied)
}

pub fn burn_session_home(target: &Path, expected: &BurnExpected<'_>) -> Result<bool, String> {
    burn_session_home_inner(target, expected, None)
}

pub fn burn_session_home_with_owner(
    target: &Path,
    expected: &BurnExpected<'_>,
    owner: &SessionOwnerSnapshot,
) -> Result<bool, String> {
    burn_session_home_inner(target, expected, Some(owner))
}

fn burn_session_home_inner(
    target: &Path,
    expected: &BurnExpected<'_>,
    owner: Option<&SessionOwnerSnapshot>,
) -> Result<bool, String> {
    let home = session_root_from_home(target);
    let stats = match assert_not_symlink(&home, "session root")? {
        None => return Ok(false),
        Some(stats) => stats,
    };
    if !stats.is_dir() {
        return Err(format!("refuse to burn non-directory: {}", home.display()));
    }
    if let Some(ino) = expected.ino {
        if stats.ino() != ino {
            return Err("session home inode changed; refusing to burn".into());
        }
    }
    if let Some(dev) = expected.dev {
        if stats.dev() != dev {
            return Err("session home device changed; refusing to burn".into());
        }
    }
    let real_home = real_existing(&home)?;
    let sessions = real_existing(&expected.user_root.join(SESSIONS_NAME))?;
    assert_inside_parent(&real_home, &sessions)?;
    assert_burn_identity(&home, expected)?;
    if let Some(owner) = owner {
        assert_burn_owner(&home, owner)?;
    }
    fs::remove_dir_all(&home).map_err(|err| err.to_string())?;
    Ok(true)
}

pub fn sweep_orphan_sessions(user_root: &Path, target_id: Option<&str>) -> usize {
    sweep_orphan_sessions_with_probe(user_root, target_id, probe_process)
}

fn sweep_orphan_sessions_with_probe<F>(
    user_root: &Path,
    target_id: Option<&str>,
    mut probe: F,
) -> usize
where
    F: FnMut(i32) -> ProcessProbe,
{
    let sessions = user_root.join(SESSIONS_NAME);
    match lstat_or_null(&sessions) {
        Some(stats) if stats.is_dir() && !stats.file_type().is_symlink() => {}
        _ => return 0,
    }
    let roots = list_session_roots(&sessions, target_id);
    let mut swept = 0;
    for root in roots {
        let owner_path = root.join(OWNER_NAME);
        let owner = match read_owner_manifest(&owner_path) {
            Ok(owner) => owner,
            Err(_) => continue,
        };
        let session_id = match owner.get("sessionId").and_then(|v| v.as_str()) {
            Some(value) if !value.is_empty() => value.to_string(),
            _ => continue,
        };
        let pid = match owner_pid(&owner) {
            Some(value) => value,
            None => continue,
        };
        let ino = match owner.get("ino").and_then(|v| v.as_u64()) {
            Some(value) => value,
            None => continue,
        };
        let dev = match owner.get("dev").and_then(|v| v.as_u64()) {
            Some(value) => value,
            None => continue,
        };
        let expected_start = owner_start_identity(&owner).filter(|value| !value.is_empty());
        if expected_start.is_some_and(|value| !is_canonical_process_start_identity(value)) {
            continue;
        }
        if owner
            .get("handoffPending")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            continue;
        }
        let stale = match probe(pid) {
            ProcessProbe::Dead => true,
            ProcessProbe::Unknown => false,
            ProcessProbe::Live(live_start) => expected_start
                .map(|expected| expected != live_start.as_str())
                .unwrap_or(false),
        };
        if !stale {
            continue;
        }
        let expected = BurnExpected {
            user_root,
            session_id: Some(&session_id),
            ino: Some(ino),
            dev: Some(dev),
        };
        let removed = match expected_start {
            Some(process_start_identity) => burn_session_home_with_owner(
                &root,
                &expected,
                &SessionOwnerSnapshot {
                    pid,
                    process_start_identity: process_start_identity.to_string(),
                },
            ),
            None => burn_session_home(&root, &expected),
        };
        if removed.is_ok_and(|removed| removed) {
            swept += 1;
        }
    }
    swept
}

pub fn target_id_from_exec(exec_path: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(exec_path.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    hex[..12].to_string()
}

fn list_session_roots(sessions: &Path, target_id: Option<&str>) -> Vec<PathBuf> {
    let start = match target_id {
        Some(id) => sessions.join(id),
        None => sessions.to_path_buf(),
    };
    match lstat_or_null(&start) {
        Some(stats) if stats.is_dir() && !stats.file_type().is_symlink() => {}
        _ => return Vec::new(),
    }
    let mut roots = Vec::new();
    let entries = match fs::read_dir(&start) {
        Ok(entries) => entries,
        Err(_) => return roots,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let child = start.join(name.as_ref());
        match lstat_or_null(&child) {
            Some(stats) if stats.is_dir() && !stats.file_type().is_symlink() => {}
            _ => continue,
        }
        if name.starts_with("s-") {
            roots.push(child);
        } else if target_id.is_none() {
            if let Ok(nested) = fs::read_dir(&child) {
                for nest in nested.flatten() {
                    let nested_name = nest.file_name();
                    let nested_name = nested_name.to_string_lossy();
                    let nest_path = child.join(nested_name.as_ref());
                    if let Some(nest_stat) = lstat_or_null(&nest_path) {
                        if nest_stat.is_dir()
                            && !nest_stat.file_type().is_symlink()
                            && nested_name.starts_with("s-")
                        {
                            roots.push(nest_path);
                        }
                    }
                }
            }
        }
    }
    roots
}

fn sessions_base(user_root: &Path, target_id: Option<&str>) -> Result<PathBuf, String> {
    let sessions = user_root.join(SESSIONS_NAME);
    let session_parent = ensure_private_dir(&sessions, user_root)?;
    match target_id {
        None => Ok(session_parent),
        Some(id) => ensure_private_dir(&sessions.join(id), &session_parent),
    }
}

fn mkdtemp(parent: &Path) -> Result<PathBuf, String> {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for extra in 0..128u32 {
        let path = parent.join(format!("s-{n:x}{extra:x}"));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.to_string()),
        }
    }
    Err("failed to allocate a session directory".into())
}

fn exclusive_copy_file(src: &Path, dest: &Path) -> Result<(), String> {
    if assert_not_symlink(dest, "copy destination")?.is_some() {
        return Err(format!(
            "refuse to overwrite existing file: {}",
            dest.display()
        ));
    }
    let data = fs::read(src).map_err(|err| err.to_string())?;
    write_private_file(dest, &data, true)
}

fn write_private_file(dest: &Path, data: &[u8], exclusive: bool) -> Result<(), String> {
    if assert_not_symlink(dest, "file")?.is_some() && exclusive {
        return Err(format!(
            "refuse to overwrite existing file: {}",
            dest.display()
        ));
    }
    let mut opts = OpenOptions::new();
    opts.write(true)
        .mode(FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW);
    if exclusive {
        opts.create_new(true);
    } else {
        opts.create(true).truncate(true);
    }
    let mut file = opts.open(dest).map_err(|err| err.to_string())?;
    file.write_all(data).map_err(|err| err.to_string())?;
    let mut perms = file
        .metadata()
        .map_err(|err| err.to_string())?
        .permissions();
    perms.set_mode(FILE_MODE);
    fs::set_permissions(dest, perms).map_err(|err| err.to_string())?;
    Ok(())
}

fn write_private_file_atomic(dest: &Path, data: &[u8]) -> Result<(), String> {
    let parent = dest
        .parent()
        .ok_or_else(|| format!("file has no parent: {}", dest.display()))?;
    let sequence = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let temporary = parent.join(format!(
        ".{OWNER_NAME}.tmp.{}-{sequence}",
        std::process::id()
    ));
    let mut opts = OpenOptions::new();
    opts.write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW);
    let result = (|| {
        let mut file = opts.open(&temporary).map_err(|err| err.to_string())?;
        file.write_all(data).map_err(|err| err.to_string())?;
        file.sync_all().map_err(|err| err.to_string())?;
        drop(file);
        fs::rename(&temporary, dest).map_err(|err| err.to_string())
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn ensure_private_dir(dir: &Path, parent: &Path) -> Result<PathBuf, String> {
    match assert_not_symlink(dir, "directory")? {
        None => {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(DIR_MODE)
                .create(dir)
                .map_err(|err| err.to_string())?;
        }
        Some(existing) if !existing.is_dir() => {
            return Err(format!("expected directory: {}", dir.display()));
        }
        Some(_) => {}
    }
    chmod(dir, DIR_MODE)?;
    let again = assert_not_symlink(dir, "directory")?
        .ok_or_else(|| format!("directory vanished: {}", dir.display()))?;
    if !again.is_dir() {
        return Err(format!("directory vanished: {}", dir.display()));
    }
    let real_dir = real_existing(dir)?;
    let real_parent = real_existing(parent)?;
    assert_inside_parent(&real_dir, &real_parent)?;
    Ok(real_dir)
}

fn chmod(path: &Path, mode: u32) -> Result<(), String> {
    let mut perms = fs::metadata(path)
        .map_err(|err| err.to_string())?
        .permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms).map_err(|err| err.to_string())
}

fn assert_not_symlink(target: &Path, label: &str) -> Result<Option<fs::Metadata>, String> {
    match lstat_or_null(target) {
        Some(stats) if stats.file_type().is_symlink() => Err(format!(
            "refuse to use symlink {label}: {}",
            target.display()
        )),
        other => Ok(other),
    }
}

fn lstat_or_null(target: &Path) -> Option<fs::Metadata> {
    fs::symlink_metadata(target).ok()
}

fn real_existing(target: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(target).map_err(|err| err.to_string())
}

fn assert_inside_parent(real_path: &Path, parent_real: &Path) -> Result<(), String> {
    if real_path == parent_real {
        return Ok(());
    }
    let parent = parent_real.to_string_lossy();
    let prefix = if parent.ends_with('/') {
        parent.to_string()
    } else {
        format!("{parent}/")
    };
    if real_path.to_string_lossy().starts_with(&prefix) {
        Ok(())
    } else {
        Err(format!(
            "path escaped private parent: {}",
            real_path.display()
        ))
    }
}

fn session_root_from_home(home: &Path) -> PathBuf {
    match home.file_name().and_then(|name| name.to_str()) {
        Some("codex-home") | Some("chromium") => home.parent().unwrap_or(home).to_path_buf(),
        _ => home.to_path_buf(),
    }
}

fn assert_burn_identity(home: &Path, expected: &BurnExpected<'_>) -> Result<(), String> {
    let Some(session_id) = expected.session_id else {
        return Ok(());
    };
    let file = home.join(OWNER_NAME);
    match assert_not_symlink(&file, "owner manifest")? {
        None => {
            if file_name(home)? == session_id {
                Ok(())
            } else {
                Err(format!("missing session owner: {}", file.display()))
            }
        }
        Some(_) => {
            let owner = read_owner_manifest(&file)?;
            let actual = owner
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if actual != session_id {
                Err("session id mismatch; refusing to burn".into())
            } else {
                Ok(())
            }
        }
    }
}

fn assert_burn_owner(home: &Path, expected: &SessionOwnerSnapshot) -> Result<(), String> {
    let file = home.join(OWNER_NAME);
    let stats = assert_not_symlink(&file, "owner manifest")?
        .ok_or_else(|| format!("missing session owner: {}", file.display()))?;
    if !stats.is_file() {
        return Err(format!("session owner is not a file: {}", file.display()));
    }
    let owner = read_owner_manifest(&file)?;
    let pid = owner_pid(&owner);
    let start = owner_start_identity(&owner);
    if pid != Some(expected.pid) || start != Some(expected.process_start_identity.as_str()) {
        return Err("session owner changed; refusing to burn".into());
    }
    Ok(())
}

fn read_owner_manifest(path: &Path) -> Result<serde_json::Value, String> {
    let body = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&body).map_err(|error| error.to_string())
}

fn owner_pid(owner: &serde_json::Value) -> Option<i32> {
    owner
        .get("pid")
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn owner_start_identity(owner: &serde_json::Value) -> Option<&str> {
    owner
        .get("processStartIdentity")
        .or_else(|| owner.get("startedAt"))
        .and_then(serde_json::Value::as_str)
}

fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| format!("invalid path: {}", path.display()))
}

pub fn is_canonical_process_start_identity(value: &str) -> bool {
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() != 5
        || !matches!(
            parts[0],
            "Sun" | "Mon" | "Tue" | "Wed" | "Thu" | "Fri" | "Sat"
        )
        || !matches!(
            parts[1],
            "Jan"
                | "Feb"
                | "Mar"
                | "Apr"
                | "May"
                | "Jun"
                | "Jul"
                | "Aug"
                | "Sep"
                | "Oct"
                | "Nov"
                | "Dec"
        )
        || !(1..=2).contains(&parts[2].len())
        || !parts[2].bytes().all(|byte| byte.is_ascii_digit())
        || parts[3].len() != 8
        || !parts[3].bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 2 | 5) {
                byte == b':'
            } else {
                byte.is_ascii_digit()
            }
        })
        || parts[4].len() != 4
        || !parts[4].bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    true
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
