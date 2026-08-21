use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SESSIONS_NAME: &str = "sessions";
const LOGS_NAME: &str = "logs";
const OWNER_NAME: &str = "owner.json";
const LOCK_NAME: &str = "lock";
const SETTINGS_FILES: &[&str] = &["auth.json", "config.toml"];
const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;

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

pub fn create_session_home(
    user_root: &Path,
    target_id: Option<&str>,
    pid: i32,
    source_home: &str,
) -> Result<SessionHome, String> {
    let parent = user_root.parent().ok_or("session user root has no parent")?;
    ensure_private_dir(user_root, parent)?;
    ensure_private_dir(&user_root.join(LOGS_NAME), user_root)?;
    let session_parent = sessions_base(user_root, target_id)?;
    let root = mkdtemp(&session_parent)?;
    chmod(&root, DIR_MODE)?;
    let root_stat = assert_not_symlink(&root, "session root")?;
    let root_stat = root_stat.ok_or_else(|| format!("session root is not a directory: {}", root.display()))?;
    if !root_stat.is_dir() {
        return Err(format!("session root is not a directory: {}", root.display()));
    }
    let real_root = real_existing(&root)?;
    assert_inside_parent(&real_root, &session_parent)?;
    let home = ensure_private_dir(&real_root.join("codex-home"), &real_root)?;
    let chromium = ensure_private_dir(&real_root.join("chromium"), &real_root)?;
    let session_id = file_name(&real_root)?;
    write_private_file(&real_root.join(LOCK_NAME), format!("{pid}\n").as_bytes(), true)?;
    let owner = serde_json::json!({
        "sessionId": session_id,
        "targetId": target_id.unwrap_or(""),
        "pid": pid,
        "sourceHome": source_home,
        "createdAt": unix_now(),
        "ino": root_stat.ino(),
        "dev": root_stat.dev(),
    });
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

fn unix_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn copy_settings(home: &Path, source_home: &Path) -> Result<usize, String> {
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
    Ok(copied)
}

pub fn burn_session_home(target: &Path, expected: &BurnExpected<'_>) -> Result<bool, String> {
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
    fs::remove_dir_all(&home).map_err(|err| err.to_string())?;
    Ok(true)
}

pub fn sweep_orphan_sessions(user_root: &Path, target_id: Option<&str>) -> usize {
    let sessions = user_root.join(SESSIONS_NAME);
    let stats = match lstat_or_null(&sessions) {
        Some(stats) if stats.is_dir() && !stats.file_type().is_symlink() => stats,
        _ => return 0,
    };
    let _ = stats;
    let roots = list_session_roots(&sessions, target_id);
    let mut swept = 0;
    for root in roots {
        let owner_path = root.join(OWNER_NAME);
        let body = match fs::read_to_string(&owner_path) {
            Ok(body) => body,
            Err(_) => continue,
        };
        let owner = match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(owner) => owner,
            Err(_) => continue,
        };
        let session_id = match owner.get("sessionId").and_then(|v| v.as_str()) {
            Some(value) if !value.is_empty() => value.to_string(),
            _ => continue,
        };
        let pid = match owner
            .get("pid")
            .and_then(|v| v.as_i64())
            .and_then(|value| i32::try_from(value).ok())
        {
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
        if pid_alive(pid) {
            continue;
        }
        let expected = BurnExpected {
            user_root,
            session_id: Some(&session_id),
            ino: Some(ino),
            dev: Some(dev),
        };
        if burn_session_home(&root, &expected).is_ok_and(|removed| removed) {
            swept += 1;
        }
    }
    swept
}

fn pid_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    unsafe { libc::kill(pid, 0) == 0 }
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
    let start_stat = match lstat_or_null(&start) {
        Some(stats) if stats.is_dir() && !stats.file_type().is_symlink() => stats,
        _ => return Vec::new(),
    };
    let _ = start_stat;
    let mut roots = Vec::new();
    let entries = match fs::read_dir(&start) {
        Ok(entries) => entries,
        Err(_) => return roots,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let child = start.join(name.as_ref());
        let stats = match lstat_or_null(&child) {
            Some(stats) if stats.is_dir() && !stats.file_type().is_symlink() => stats,
            _ => continue,
        };
        let _ = stats;
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
        return Err(format!("refuse to overwrite existing file: {}", dest.display()));
    }
    let data = fs::read(src).map_err(|err| err.to_string())?;
    write_private_file(dest, &data, true)
}

fn write_private_file(dest: &Path, data: &[u8], exclusive: bool) -> Result<(), String> {
    if let Some(prior) = assert_not_symlink(dest, "file")? {
        if prior.file_type().is_symlink() {
            return Err(format!("refuse to overwrite symlink file: {}", dest.display()));
        }
        if exclusive {
            return Err(format!("refuse to overwrite existing file: {}", dest.display()));
        }
    }
    let mut opts = OpenOptions::new();
    opts.write(true).mode(FILE_MODE).custom_flags(libc::O_NOFOLLOW);
    if exclusive {
        opts.create_new(true);
    } else {
        opts.create(true).truncate(true);
    }
    let mut file = opts.open(dest).map_err(|err| err.to_string())?;
    file.write_all(data).map_err(|err| err.to_string())?;
    let mut perms = file.metadata().map_err(|err| err.to_string())?.permissions();
    perms.set_mode(FILE_MODE);
    fs::set_permissions(dest, perms).map_err(|err| err.to_string())?;
    Ok(())
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
    let mut perms = fs::metadata(path).map_err(|err| err.to_string())?.permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms).map_err(|err| err.to_string())
}

fn assert_not_symlink(target: &Path, label: &str) -> Result<Option<fs::Metadata>, String> {
    match lstat_or_null(target) {
        Some(stats) if stats.file_type().is_symlink() => {
            Err(format!("refuse to use symlink {label}: {}", target.display()))
        }
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
            let owner: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&file).map_err(|err| err.to_string())?)
                    .map_err(|err| err.to_string())?;
            let actual = owner.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");
            if actual != session_id {
                Err("session id mismatch; refusing to burn".into())
            } else {
                Ok(())
            }
        }
    }
}

fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| format!("invalid path: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = TEMP_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("incodex-session-{pid}-{n}-{counter}"));
        fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn create_session_uses_random_directory_under_sessions() {
        let root = temp_root();
        let user_root = root.join(".incodex");
        let first = create_session_home(&user_root, Some("t1"), 1, "").unwrap();
        let second = create_session_home(&user_root, Some("t1"), 1, "").unwrap();
        assert_ne!(first.home, second.home);
        assert_ne!(first.session_id, second.session_id);
        let sessions = fs::canonicalize(user_root.join("sessions")).unwrap();
        assert!(first.root.starts_with(sessions));
        assert!(first.home.ends_with("codex-home"));
        assert!(first.chromium.ends_with("chromium"));
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&user_root), 0o700);
        assert_eq!(mode(&first.root), 0o700);
        assert_eq!(mode(&first.root.join("owner.json")), 0o600);
    }

    #[test]
    fn copy_settings_then_burn_removes_the_session() {
        let root = temp_root();
        let user_root = root.join(".incodex");
        let source = root.join("codex");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("auth.json"), "{\"token\":\"x\"}").unwrap();
        let session = create_session_home(&user_root, None, 0, "").unwrap();
        assert_eq!(copy_settings(&session.home, &source).unwrap(), 1);
        assert_eq!(
            fs::read_to_string(session.home.join("auth.json")).unwrap(),
            "{\"token\":\"x\"}"
        );
        burn_session_home(
            &session.root,
            &BurnExpected {
                user_root: &user_root,
                session_id: Some(&session.session_id),
                ino: Some(session.ino),
                dev: Some(session.dev),
            },
        )
        .unwrap();
        assert!(!session.root.exists());
        assert_eq!(
            fs::read_to_string(source.join("auth.json")).unwrap(),
            "{\"token\":\"x\"}"
        );
    }

    #[test]
    fn session_lifecycle_does_not_create_or_mutate_identity_cache() {
        let root = temp_root();
        let user_root = root.join(".incodex");
        let source = root.join("codex");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("auth.json"), "{\"token\":\"source\"}\n").unwrap();
        fs::write(source.join("config.toml"), "localeOverride = \"zh-CN\"\n").unwrap();
        let identity = user_root.join("identity");
        fs::create_dir_all(&identity).unwrap();
        fs::write(identity.join("auth.json"), "legacy-cache\n").unwrap();

        let source_before = (
            fs::read(source.join("auth.json")).unwrap(),
            fs::read(source.join("config.toml")).unwrap(),
        );
        let session = create_session_home(&user_root, None, 0, "").unwrap();
        assert_eq!(copy_settings(&session.home, &source).unwrap(), 2);
        assert_eq!(fs::read(identity.join("auth.json")).unwrap(), b"legacy-cache\n");
        assert_eq!(fs::read(session.home.join("auth.json")).unwrap(), source_before.0);
        assert_eq!(fs::read(session.home.join("config.toml")).unwrap(), source_before.1);

        burn_session_home(
            &session.root,
            &BurnExpected {
                user_root: &user_root,
                session_id: Some(&session.session_id),
                ino: Some(session.ino),
                dev: Some(session.dev),
            },
        )
        .unwrap();
        assert_eq!(fs::read(source.join("auth.json")).unwrap(), source_before.0);
        assert_eq!(fs::read(source.join("config.toml")).unwrap(), source_before.1);
        assert_eq!(fs::read(identity.join("auth.json")).unwrap(), b"legacy-cache\n");
    }

    #[test]
    fn orphan_sweep_refuses_a_replaced_session_root_without_recorded_identity() {
        let root = temp_root();
        let user_root = root.join(".incodex");
        let session = create_session_home(&user_root, None, 999999, "").unwrap();
        fs::remove_dir_all(&session.root).unwrap();
        fs::create_dir(&session.root).unwrap();
        fs::write(session.root.join("replacement.txt"), "keep-me").unwrap();

        assert_eq!(sweep_orphan_sessions(&user_root, None), 0);
        assert!(session.root.exists());
        assert_eq!(fs::read_to_string(session.root.join("replacement.txt")).unwrap(), "keep-me");
    }

    #[test]
    fn burn_refuses_a_session_id_mismatch() {
        let root = temp_root();
        let user_root = root.join(".incodex");
        let session = create_session_home(&user_root, None, 0, "").unwrap();
        let err = burn_session_home(
            &session.root,
            &BurnExpected {
                user_root: &user_root,
                session_id: Some("other"),
                ino: None,
                dev: None,
            },
        )
        .unwrap_err();
        assert!(err.contains("mismatch"));
        assert!(session.root.exists());
    }
}
