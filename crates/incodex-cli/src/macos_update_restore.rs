use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use incodex_core::canonical::is_official_app;
use incodex_macos::{process_executable_path, read_plist_info, AppQuiescence};
use incodex_transaction::acquire_target_lock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const REGISTRATION_SCHEMA_VERSION: u32 = 1;
const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const HELPER_FILE_MODE: u32 = 0o700;
const HELPER_FILE_NAME: &str = "incodex";
const ORDINARY_EXIT_GRACE: Duration = Duration::from_secs(45);
const REPLACEMENT_GAP_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const IDLE_RECHECK_INTERVAL: Duration = Duration::from_millis(250);
const PROCESS_RECHECK_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRegistration {
    pub schema_version: u32,
    pub install_id: String,
    pub app_path: PathBuf,
    pub helper_path: PathBuf,
    pub helper_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRequest {
    pub install_id: String,
    pub parent_pid: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinatorSnapshot {
    pub source_build: u64,
    pub observed_build: Option<u64>,
    pub parent_running: bool,
    pub app_running: bool,
    pub integration_installed: bool,
    pub registered: bool,
    pub grace_expired: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorAction {
    Wait,
    ExitNoUpdate,
    ExitCancelled,
    Reinstall { expected_build: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorOutcome {
    NoUpdate,
    Cancelled,
    Reinstalled { build: u64 },
}

struct SystemCoordinator {
    root: PathBuf,
    request: WorkerRequest,
    registration: UpdateRegistration,
    source_build: u64,
    executable: PathBuf,
    parent_exit_at: Option<Instant>,
    missing_since: Option<Instant>,
    watched_pids: Vec<i32>,
}

pub fn parse_worker_request(
    marker: Option<&str>,
    install_id: Option<&str>,
    parent_pid: Option<&str>,
) -> Option<Result<WorkerRequest, String>> {
    if marker != Some("1") {
        return None;
    }
    let request = (|| {
        let install_id = install_id
            .filter(|value| !value.is_empty())
            .ok_or("macOS update worker has no install epoch")?;
        let parent_pid = parent_pid
            .ok_or("macOS update worker has no parent PID")?
            .parse::<i32>()
            .map_err(|error| format!("macOS update worker has an invalid parent PID: {error}"))?;
        if parent_pid <= 0 {
            return Err("macOS update worker parent PID must be positive".into());
        }
        Ok(WorkerRequest {
            install_id: install_id.to_string(),
            parent_pid,
        })
    })();
    Some(request)
}

pub fn try_run_worker() -> Option<Result<(), String>> {
    let request = parse_worker_request(
        std::env::var("INCODEX_MACOS_UPDATE_WORKER").ok().as_deref(),
        std::env::var("INCODEX_MACOS_UPDATE_INSTALL_ID")
            .ok()
            .as_deref(),
        std::env::var("INCODEX_MACOS_UPDATE_PARENT_PID")
            .ok()
            .as_deref(),
    )?;
    Some(request.and_then(run_system_worker))
}

pub fn next_action(snapshot: CoordinatorSnapshot) -> CoordinatorAction {
    if !snapshot.registered {
        return CoordinatorAction::ExitCancelled;
    }
    if snapshot.parent_running || snapshot.app_running {
        return CoordinatorAction::Wait;
    }

    let Some(observed_build) = snapshot.observed_build else {
        return CoordinatorAction::Wait;
    };
    if !snapshot.integration_installed {
        return CoordinatorAction::Reinstall {
            expected_build: observed_build,
        };
    }
    if observed_build != snapshot.source_build {
        return CoordinatorAction::Wait;
    }
    if snapshot.grace_expired {
        CoordinatorAction::ExitNoUpdate
    } else {
        CoordinatorAction::Wait
    }
}

pub fn drive_coordinator<O, R, W>(
    mut observe: O,
    mut reinstall: R,
    mut wait: W,
) -> Result<CoordinatorOutcome, String>
where
    O: FnMut() -> Result<CoordinatorSnapshot, String>,
    R: FnMut(u64) -> Result<(), String>,
    W: FnMut() -> Result<(), String>,
{
    loop {
        match next_action(observe()?) {
            CoordinatorAction::Wait => wait()?,
            CoordinatorAction::ExitNoUpdate => return Ok(CoordinatorOutcome::NoUpdate),
            CoordinatorAction::ExitCancelled => return Ok(CoordinatorOutcome::Cancelled),
            CoordinatorAction::Reinstall { expected_build } => {
                reinstall(expected_build)?;
                return Ok(CoordinatorOutcome::Reinstalled {
                    build: expected_build,
                });
            }
        }
    }
}

fn run_system_worker(request: WorkerRequest) -> Result<(), String> {
    let root = incodex_core::paths::user_root();
    let coordinator = std::cell::RefCell::new(SystemCoordinator::new(root, request)?);
    drive_coordinator(
        || coordinator.borrow_mut().observe(),
        |expected_build| coordinator.borrow().reinstall(expected_build),
        || coordinator.borrow_mut().wait(),
    )?;
    Ok(())
}

impl SystemCoordinator {
    fn new(root: PathBuf, request: WorkerRequest) -> Result<Self, String> {
        let registration = read_registration(&root)?
            .ok_or("macOS update registration disappeared before worker startup")?;
        if registration.install_id != request.install_id {
            return Err("macOS update worker install epoch is stale".into());
        }
        if !is_official_app(&registration.app_path, None) {
            return Err("macOS update worker is not bound to the official Codex app".into());
        }
        verify_running_helper(&registration)?;
        let plist = read_plist_info(&registration.app_path)
            .ok_or("macOS update worker cannot read the source Codex build")?;
        let source_build = plist.app_build.parse::<u64>().map_err(|error| {
            format!("macOS update worker found an invalid Codex build: {error}")
        })?;
        let executable = registration
            .app_path
            .join("Contents")
            .join("MacOS")
            .join(plist.executable);

        Ok(Self {
            root,
            request,
            registration,
            source_build,
            executable,
            parent_exit_at: None,
            missing_since: None,
            watched_pids: Vec::new(),
        })
    }

    fn observe(&mut self) -> Result<CoordinatorSnapshot, String> {
        let registered = self.registration_is_current()?;
        if !registered {
            return Ok(CoordinatorSnapshot {
                source_build: self.source_build,
                observed_build: None,
                parent_running: false,
                app_running: false,
                integration_installed: false,
                registered: false,
                grace_expired: false,
            });
        }

        let parent_running = process_executable_path(self.request.parent_pid)
            .is_some_and(|path| path == self.executable);
        let app_processes =
            AppQuiescence::from_executable(self.executable.clone())?.running_pids()?;
        let app_running = !app_processes.is_empty();
        self.watched_pids = app_processes;
        if parent_running && !self.watched_pids.contains(&self.request.parent_pid) {
            self.watched_pids.push(self.request.parent_pid);
        }

        let now = Instant::now();
        if parent_running {
            self.parent_exit_at = None;
        } else {
            self.parent_exit_at.get_or_insert(now);
        }

        let observed_build = current_build(&self.registration.app_path);
        if observed_build.is_none() && !parent_running {
            let missing_since = self.missing_since.get_or_insert(now);
            if now.duration_since(*missing_since) >= REPLACEMENT_GAP_TIMEOUT {
                return Err("timed out waiting for the official Codex replacement bundle".into());
            }
        } else {
            self.missing_since = None;
        }

        let integration_installed =
            crate::install::update_restore_install_id(&self.registration.app_path)
                .is_some_and(|install_id| install_id == self.registration.install_id);
        let grace_expired = self
            .parent_exit_at
            .is_some_and(|started| now.duration_since(started) >= ORDINARY_EXIT_GRACE);

        Ok(CoordinatorSnapshot {
            source_build: self.source_build,
            observed_build,
            parent_running,
            app_running,
            integration_installed,
            registered,
            grace_expired,
        })
    }

    fn reinstall(&self, expected_build: u64) -> Result<(), String> {
        if !self.registration_is_current()? {
            return Err("macOS update registration changed before recovery".into());
        }
        crate::install::reinstall_after_official_update(
            &self.root,
            &self.registration.app_path,
            &self.registration.helper_path,
            expected_build,
        )?;
        Ok(())
    }

    fn wait(&mut self) -> Result<(), String> {
        if self.watched_pids.is_empty() {
            std::thread::sleep(IDLE_RECHECK_INTERVAL);
            return Ok(());
        }
        wait_for_process_exit(&self.watched_pids, PROCESS_RECHECK_INTERVAL)
    }

    fn registration_is_current(&self) -> Result<bool, String> {
        let Some(current) = read_registration(&self.root)? else {
            return Ok(false);
        };
        Ok(current.install_id == self.request.install_id
            && current.app_path == self.registration.app_path
            && current.helper_path == self.registration.helper_path
            && current.helper_sha256 == self.registration.helper_sha256)
    }
}

fn current_build(app: &Path) -> Option<u64> {
    read_plist_info(app)?.app_build.parse::<u64>().ok()
}

fn verify_running_helper(registration: &UpdateRegistration) -> Result<(), String> {
    let current = std::env::current_exe()
        .map_err(|error| format!("cannot locate the running macOS update helper: {error}"))?;
    let current = fs::canonicalize(&current)
        .map_err(|error| format!("cannot resolve the running macOS update helper: {error}"))?;
    let expected = fs::canonicalize(&registration.helper_path)
        .map_err(|error| format!("cannot resolve the registered macOS update helper: {error}"))?;
    if current != expected {
        return Err("macOS update worker executable does not match its registration".into());
    }
    let digest = sha256_hex(&read_regular_file(&current, "running macOS update helper")?);
    if digest != registration.helper_sha256 {
        return Err("running macOS update helper failed its content hash".into());
    }
    Ok(())
}

fn wait_for_process_exit(pids: &[i32], timeout: Duration) -> Result<(), String> {
    let queue = unsafe { libc::kqueue() };
    if queue < 0 {
        std::thread::sleep(timeout);
        return Ok(());
    }

    let changes = pids
        .iter()
        .copied()
        .filter(|pid| *pid > 0)
        .map(|pid| libc::kevent {
            ident: pid as libc::uintptr_t,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ONESHOT,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        })
        .collect::<Vec<_>>();
    if changes.is_empty() {
        unsafe { libc::close(queue) };
        std::thread::sleep(timeout);
        return Ok(());
    }

    let mut event = std::mem::MaybeUninit::<libc::kevent>::uninit();
    let timeout = libc::timespec {
        tv_sec: timeout.as_secs() as libc::time_t,
        tv_nsec: timeout.subsec_nanos() as libc::c_long,
    };
    let result = unsafe {
        libc::kevent(
            queue,
            changes.as_ptr(),
            changes.len() as i32,
            event.as_mut_ptr(),
            1,
            &timeout,
        )
    };
    let error = (result < 0).then(std::io::Error::last_os_error);
    unsafe { libc::close(queue) };
    match error {
        Some(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(()),
        Some(error) => Err(format!(
            "cannot wait for the Codex process to exit: {error}"
        )),
        None => Ok(()),
    }
}

pub fn publish_registration(
    root: &Path,
    helper_source: &Path,
    app_path: &Path,
    install_id: &str,
) -> Result<UpdateRegistration, String> {
    if install_id.is_empty() {
        return Err("macOS update registration needs an install epoch".into());
    }
    if !app_path.is_absolute() {
        return Err("macOS update registration needs an absolute app path".into());
    }

    ensure_private_dir(root)?;
    let registration_path = registration_path(root);
    let _lock = acquire_target_lock(
        root,
        &registration_path,
        "macos-update-registration",
        Some(install_id),
    )?;
    publish_registration_locked(root, helper_source, app_path, install_id)
}

pub fn refresh_registered_helper(root: &Path, helper_source: &Path) -> Result<bool, String> {
    let Some(observed) = read_registration(root)? else {
        return Ok(false);
    };
    let path = registration_path(root);
    let _lock = acquire_target_lock(
        root,
        &path,
        "macos-update-registration-refresh",
        Some(&observed.install_id),
    )?;
    let Some(current) = read_registration(root)? else {
        return Ok(false);
    };
    publish_registration_locked(root, helper_source, &current.app_path, &current.install_id)?;
    Ok(true)
}

fn publish_registration_locked(
    root: &Path,
    helper_source: &Path,
    app_path: &Path,
    install_id: &str,
) -> Result<UpdateRegistration, String> {
    let helper_bytes = read_regular_file(helper_source, "macOS update helper source")?;
    let helper_sha256 = sha256_hex(&helper_bytes);
    let helpers_dir = root.join("helpers");
    ensure_private_dir(&helpers_dir)?;
    let update_helpers_dir = helpers_dir.join("macos-update");
    ensure_private_dir(&update_helpers_dir)?;
    let release_dir = update_helpers_dir.join(&helper_sha256);
    ensure_private_dir(&release_dir)?;
    let helper_path = release_dir.join(HELPER_FILE_NAME);
    publish_helper(&helper_path, &helper_bytes, &helper_sha256)?;

    let registration = UpdateRegistration {
        schema_version: REGISTRATION_SCHEMA_VERSION,
        install_id: install_id.to_string(),
        app_path: app_path.to_path_buf(),
        helper_path,
        helper_sha256,
    };
    let body = format!(
        "{}\n",
        serde_json::to_string(&registration).map_err(|error| error.to_string())?
    );
    write_private_atomic(&registration_path(root), body.as_bytes())?;
    Ok(registration)
}

pub fn read_registration(root: &Path) -> Result<Option<UpdateRegistration>, String> {
    let path = registration_path(root);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect macOS update registration: {error}")),
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "refuse to read symlink macOS update registration: {}",
            path.display()
        ));
    }
    if !metadata.file_type().is_file() {
        return Err(format!(
            "macOS update registration is not a regular file: {}",
            path.display()
        ));
    }

    let body = read_regular_file(&path, "macOS update registration")?;
    let registration: UpdateRegistration =
        serde_json::from_slice(&body).map_err(|error| format!("invalid registration: {error}"))?;
    validate_registration(root, &registration)?;
    Ok(Some(registration))
}

pub fn remove_registration(root: &Path, install_id: &str) -> Result<(), String> {
    let path = registration_path(root);
    let _lock = acquire_target_lock(
        root,
        &path,
        "macos-update-registration-remove",
        Some(install_id),
    )?;
    let Some(registration) = read_registration(root)? else {
        return Ok(());
    };
    if registration.install_id != install_id {
        return Ok(());
    }
    match fs::remove_file(&path) {
        Ok(()) => sync_parent(&path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot remove macOS update registration: {error}")),
    }
}

fn registration_path(root: &Path) -> PathBuf {
    root.join("macos-update").join("registration.json")
}

fn validate_registration(root: &Path, registration: &UpdateRegistration) -> Result<(), String> {
    if registration.schema_version != REGISTRATION_SCHEMA_VERSION {
        return Err(format!(
            "unsupported macOS update registration schema: {}",
            registration.schema_version
        ));
    }
    if registration.install_id.is_empty() {
        return Err("macOS update registration has no install epoch".into());
    }
    if !registration.app_path.is_absolute() {
        return Err("macOS update registration app path is not absolute".into());
    }
    let expected_helper_path = root
        .join("helpers")
        .join("macos-update")
        .join(&registration.helper_sha256)
        .join(HELPER_FILE_NAME);
    if registration.helper_path != expected_helper_path {
        return Err("macOS update registration helper escaped its private root".into());
    }
    if registration.helper_sha256.len() != 64
        || !registration
            .helper_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("macOS update registration has an invalid helper hash".into());
    }
    Ok(())
}

fn publish_helper(path: &Path, bytes: &[u8], expected_sha256: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                return Err(format!(
                    "macOS update helper is not a regular file: {}",
                    path.display()
                ));
            }
            if sha256_hex(&read_regular_file(path, "macOS update helper")?) != expected_sha256 {
                return Err("macOS update helper does not match its content address".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_private_atomic_with_mode(path, bytes, HELPER_FILE_MODE)?;
        }
        Err(error) => return Err(format!("cannot inspect macOS update helper: {error}")),
    }
    set_mode(path, HELPER_FILE_MODE)
}

fn read_regular_file(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot inspect {label}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!("{label} is not a regular file: {}", path.display()));
    }
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|error| format!("cannot open {label}: {error}"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {label}: {error}"))?;
    Ok(bytes)
}

fn ensure_private_dir(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "refuse to use symlink directory: {}",
                path.display()
            ));
        }
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(format!("expected directory: {}", path.display()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path)
                .map_err(|error| format!("cannot create private directory: {error}"))?;
        }
        Err(error) => return Err(format!("cannot inspect private directory: {error}")),
    }
    set_mode(path, PRIVATE_DIR_MODE)
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_private_atomic_with_mode(path, bytes, PRIVATE_FILE_MODE)
}

fn write_private_atomic_with_mode(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("private file has no parent: {}", path.display()))?;
    ensure_private_dir(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let temporary = parent.join(format!(".tmp-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW);
        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("cannot stage private file: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("cannot write private file: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("cannot flush private file: {error}"))?;
        set_file_mode(&file, mode)?;
        drop(file);
        fs::rename(&temporary, path)
            .map_err(|error| format!("cannot publish private file: {error}"))?;
        sync_parent(path)
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn set_file_mode(file: &File, mode: u32) -> Result<(), String> {
    let result = unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    let mut permissions = fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions).map_err(|error| error.to_string())
}

fn sync_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("file has no parent: {}", path.display()))?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY);
    options
        .open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| error.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
