use std::ffi::c_void;
use std::fs::{self, OpenOptions};
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    AclSizeInformation, AddAccessAllowedAceEx, EqualSid, GetAce, GetAclInformation, GetLengthSid,
    GetSecurityDescriptorControl, GetTokenInformation, InitializeAcl, TokenUser,
    ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, ACL_SIZE_INFORMATION, CONTAINER_INHERIT_ACE,
    DACL_SECURITY_INFORMATION, OBJECT_INHERIT_ACE, PROTECTED_DACL_SECURITY_INFORMATION, PSID,
    SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ALL_ACCESS,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::windows_path::{
    reject_reparse_ancestors, require_absolute, validate_existing_session_dir,
};

const SETTINGS_FILES: &[&str] = &["auth.json", "config.toml"];
const OWNER_NAME: &str = "owner.json";
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[path = "windows_session_owner.rs"]
mod owner;
use owner::{
    current_process_creation_time, probe_process, read_owner_manifest, write_owner_manifest,
    WindowsProcessProbe, WindowsSessionOwner,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsSessionIdentity {
    volume_serial_number: u32,
    file_index: u64,
}

#[derive(Debug, Clone)]
pub struct WindowsSessionHome {
    pub session_id: String,
    pub root: PathBuf,
    pub home: PathBuf,
    pub chromium: PathBuf,
    user_root: PathBuf,
    identity: WindowsSessionIdentity,
    owner_pid: u32,
    owner_creation_time: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsCleanupResult {
    Removed,
    Retained { reason: String },
    Unknown { reason: String },
}

pub fn create_windows_session(user_root: &Path) -> Result<WindowsSessionHome, String> {
    require_absolute(user_root, "Windows Incodex root")?;
    let parent = user_root.parent().ok_or_else(|| {
        format!(
            "Windows Incodex root has no parent: {}",
            user_root.display()
        )
    })?;
    reject_reparse_ancestors(parent)?;
    ensure_private_dir(user_root)?;
    let sessions = ensure_private_dir(&user_root.join("sessions"))?;
    let root = create_unique_private_dir(&sessions)?;
    let result = (|| {
        let home = ensure_private_dir(&root.join("codex-home"))?;
        let chromium = ensure_private_dir(&root.join("chromium"))?;
        let root = validate_existing_session_dir(user_root, &root)?;
        let identity = session_identity(&root)?;
        let session_id = root
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| name.starts_with("s-") && name.len() > 2)
            .ok_or_else(|| format!("invalid Windows session name: {}", root.display()))?
            .to_string();
        let owner_pid = std::process::id();
        let owner_creation_time = current_process_creation_time()?;
        write_owner_manifest(
            &root.join(OWNER_NAME),
            &WindowsSessionOwner {
                session_id: session_id.clone(),
                pid: owner_pid,
                process_creation_time: owner_creation_time,
                identity: identity.clone(),
            },
        )?;
        Ok(WindowsSessionHome {
            session_id,
            root,
            home,
            chromium,
            user_root: user_root.to_path_buf(),
            identity,
            owner_pid,
            owner_creation_time,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&root);
    }
    result
}

pub fn sweep_orphan_windows_sessions(user_root: &Path) -> usize {
    if require_absolute(user_root, "Windows Incodex root").is_err() {
        return 0;
    }
    let sessions = user_root.join("sessions");
    if reject_reparse_ancestors(&sessions).is_err() || verify_private_acl(&sessions).is_err() {
        return 0;
    }
    let entries = match fs::read_dir(&sessions) {
        Ok(entries) => entries,
        Err(_) => return 0,
    };
    let mut swept = 0;
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !name.starts_with("s-") || name.len() <= 2 {
            continue;
        }
        let root = entry.path();
        let validated = match validate_existing_session_dir(&sessions, &root) {
            Ok(path) => path,
            Err(_) => continue,
        };
        if verify_private_acl(&validated).is_err() {
            continue;
        }
        let owner = match read_owner_manifest(&validated.join(OWNER_NAME)) {
            Ok(owner) if owner.session_id == name => owner,
            _ => continue,
        };
        if session_identity(&validated).ok().as_ref() != Some(&owner.identity) {
            continue;
        }
        let stale = match probe_process(owner.pid) {
            WindowsProcessProbe::Dead => true,
            WindowsProcessProbe::Live(creation_time) => {
                creation_time != owner.process_creation_time
            }
            WindowsProcessProbe::Unknown => false,
        };
        if !stale {
            continue;
        }
        let session = WindowsSessionHome {
            session_id: owner.session_id,
            home: validated.join("codex-home"),
            chromium: validated.join("chromium"),
            root: validated,
            user_root: user_root.to_path_buf(),
            identity: owner.identity,
            owner_pid: owner.pid,
            owner_creation_time: owner.process_creation_time,
        };
        if burn_windows_session(&session) == WindowsCleanupResult::Removed {
            swept += 1;
        }
    }
    swept
}

pub fn copy_windows_settings(
    session: &WindowsSessionHome,
    source_home: &Path,
) -> Result<usize, String> {
    validate_session_identity(session)?;
    require_absolute(source_home, "Windows Codex source home")?;
    reject_reparse_ancestors(source_home)?;
    if !source_home.is_dir() {
        return Err(format!(
            "Windows Codex source home is not a directory: {}",
            source_home.display()
        ));
    }

    let mut copied = 0;
    for name in SETTINGS_FILES {
        let source = source_home.join(name);
        match fs::symlink_metadata(&source) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("cannot inspect {}: {error}", source.display())),
            Ok(_) => {}
        }
        reject_reparse_ancestors(&source)?;
        copy_private_file(&source, &session.home.join(name))?;
        copied += 1;
    }
    Ok(copied)
}

pub fn burn_windows_session(session: &WindowsSessionHome) -> WindowsCleanupResult {
    let sessions = session.user_root.join("sessions");
    let validated = match validate_existing_session_dir(&sessions, &session.root) {
        Ok(path) => path,
        Err(reason) => return classify_unsafe_cleanup(&session.root, reason),
    };
    if validated.file_name().and_then(|name| name.to_str()) != Some(&session.session_id) {
        return WindowsCleanupResult::Retained {
            reason: "Windows session id changed; refusing cleanup".to_string(),
        };
    }
    match session_identity(&validated) {
        Ok(identity) if identity == session.identity => {}
        Ok(_) => {
            return WindowsCleanupResult::Retained {
                reason: "Windows session file identity changed; refusing cleanup".to_string(),
            }
        }
        Err(reason) => return classify_unsafe_cleanup(&session.root, reason),
    }
    match read_owner_manifest(&validated.join(OWNER_NAME)) {
        Ok(owner)
            if owner.session_id == session.session_id
                && owner.pid == session.owner_pid
                && owner.process_creation_time == session.owner_creation_time
                && owner.identity == session.identity => {}
        Ok(_) => {
            return WindowsCleanupResult::Retained {
                reason: "Windows session owner changed; refusing cleanup".to_string(),
            }
        }
        Err(reason) => return classify_unsafe_cleanup(&session.root, reason),
    }

    let removal = fs::remove_dir_all(&validated);
    match fs::symlink_metadata(&session.root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => WindowsCleanupResult::Removed,
        Ok(_) => WindowsCleanupResult::Retained {
            reason: removal
                .err()
                .map(|error| format!("Windows session cleanup failed: {error}"))
                .unwrap_or_else(|| "Windows session still exists after cleanup".to_string()),
        },
        Err(error) => WindowsCleanupResult::Unknown {
            reason: format!("cannot verify Windows session cleanup: {error}"),
        },
    }
}

pub fn verify_private_acl(path: &Path) -> Result<(), String> {
    let sid = current_user_sid()?;
    let wide = wide_path(path)?;
    let mut dacl = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(format!(
            "cannot read private ACL for {}: {}",
            path.display(),
            io::Error::from_raw_os_error(status as i32)
        ));
    }
    let descriptor = LocalAllocation(descriptor);
    let result = inspect_private_acl(descriptor.0, dacl, sid.as_ptr().cast_mut().cast());
    result.map_err(|reason| format!("{}: {}", path.display(), reason))
}

fn ensure_private_dir(path: &Path) -> Result<PathBuf, String> {
    let created = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
            return Err(format!("directory is a reparse point: {}", path.display()))
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(format!("expected directory: {}", path.display()))
        }
        Ok(_) => false,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path)
                .map_err(|error| format!("cannot create directory {}: {error}", path.display()))?;
            true
        }
        Err(error) => {
            return Err(format!(
                "cannot inspect directory {}: {error}",
                path.display()
            ))
        }
    };
    let result = (|| {
        reject_reparse_ancestors(path)?;
        apply_private_acl(path)?;
        verify_private_acl(path)?;
        fs::canonicalize(path)
            .map_err(|error| format!("cannot resolve {}: {error}", path.display()))
    })();
    if created && result.is_err() {
        let _ = fs::remove_dir(path);
    }
    result
}

fn create_unique_private_dir(parent: &Path) -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for _ in 0..128 {
        let sequence = SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            "s-{timestamp:x}-{:x}-{sequence:x}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => {
                let result = apply_private_acl(&path)
                    .and_then(|_| verify_private_acl(&path))
                    .map(|_| path.clone());
                if result.is_err() {
                    let _ = fs::remove_dir(&path);
                }
                return result;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "cannot create Windows session directory {}: {error}",
                    path.display()
                ))
            }
        }
    }
    Err("cannot allocate a unique Windows session directory".to_string())
}

fn copy_private_file(source: &Path, destination: &Path) -> Result<(), String> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(format!(
            "refuse to overwrite Windows session setting: {}",
            destination.display()
        ));
    }
    let mut source_file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(source)
        .map_err(|error| format!("cannot open source setting {}: {error}", source.display()))?;
    let metadata = source_file.metadata().map_err(|error| {
        format!(
            "cannot inspect source setting {}: {error}",
            source.display()
        )
    })?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "source setting is not a plain file: {}",
            source.display()
        ));
    }
    let mut destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| {
            format!(
                "cannot create session setting {}: {error}",
                destination.display()
            )
        })?;
    if let Err(error) =
        io::copy(&mut source_file, &mut destination_file).and_then(|_| destination_file.sync_all())
    {
        drop(destination_file);
        let _ = fs::remove_file(destination);
        return Err(format!(
            "cannot copy session setting {}: {error}",
            destination.display()
        ));
    }
    drop(destination_file);
    if let Err(error) = apply_private_acl(destination).and_then(|_| verify_private_acl(destination))
    {
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    Ok(())
}

fn validate_session_identity(session: &WindowsSessionHome) -> Result<(), String> {
    let root = validate_existing_session_dir(&session.user_root.join("sessions"), &session.root)?;
    if session_identity(&root)? != session.identity {
        return Err("Windows session file identity changed".to_string());
    }
    validate_existing_session_dir(&root, &session.home)?;
    validate_existing_session_dir(&root, &session.chromium)?;
    verify_private_acl(&root)?;
    verify_private_acl(&session.home)?;
    verify_private_acl(&session.chromium)?;
    let owner = read_owner_manifest(&root.join(OWNER_NAME))?;
    if owner.session_id != session.session_id
        || owner.pid != session.owner_pid
        || owner.process_creation_time != session.owner_creation_time
        || owner.identity != session.identity
    {
        return Err("Windows session owner changed".to_string());
    }
    Ok(())
}

fn session_identity(path: &Path) -> Result<WindowsSessionIdentity, String> {
    let wide = wide_path(path)?;
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(format!(
            "cannot open Windows session identity {}: {}",
            path.display(),
            io::Error::last_os_error()
        ));
    }
    let handle = OwnedHandle(raw);
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(handle.0, &mut info) } == 0 {
        return Err(format!(
            "cannot read Windows session identity {}: {}",
            path.display(),
            io::Error::last_os_error()
        ));
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "Windows session identity is a reparse point: {}",
            path.display()
        ));
    }
    let volume_serial_number = info.dwVolumeSerialNumber;
    let file_index = ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64;
    Ok(WindowsSessionIdentity {
        volume_serial_number,
        file_index,
    })
}

fn apply_private_acl(path: &Path) -> Result<(), String> {
    let sid = current_user_sid()?;
    let acl_bytes =
        size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>() + sid.len();
    let mut storage = vec![0u32; acl_bytes.div_ceil(size_of::<u32>())];
    let acl = storage.as_mut_ptr().cast::<ACL>();
    if unsafe { InitializeAcl(acl, acl_bytes as u32, ACL_REVISION) } == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if unsafe {
        AddAccessAllowedAceEx(
            acl,
            ACL_REVISION,
            OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
            FILE_ALL_ACCESS,
            sid.as_ptr().cast_mut().cast(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error().to_string());
    }
    let wide = wide_path(path)?;
    let status = unsafe {
        SetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            acl,
            std::ptr::null(),
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(format!(
            "cannot protect ACL for {}: {}",
            path.display(),
            io::Error::from_raw_os_error(status as i32)
        ))
    }
}

fn current_user_sid() -> Result<Vec<u8>, String> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let token = OwnedHandle(token);
    let mut needed = 0;
    unsafe {
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut needed);
    }
    if needed == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let mut storage = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().to_string());
    }
    let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    let length = unsafe { GetLengthSid(user.User.Sid) } as usize;
    if length == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(unsafe { std::slice::from_raw_parts(user.User.Sid.cast::<u8>(), length) }.to_vec())
}

fn inspect_private_acl(
    descriptor: *mut c_void,
    acl: *mut ACL,
    expected_sid: PSID,
) -> Result<(), String> {
    if descriptor.is_null() || acl.is_null() {
        return Err("private DACL is missing".to_string());
    }
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    if control & SE_DACL_PROTECTED == 0 {
        return Err("private DACL is not protected".to_string());
    }
    let mut info = ACL_SIZE_INFORMATION::default();
    if unsafe {
        GetAclInformation(
            acl,
            (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().to_string());
    }
    if info.AceCount != 1 {
        return Err(format!(
            "private DACL has {} entries instead of one",
            info.AceCount
        ));
    }
    let mut raw_ace = std::ptr::null_mut();
    if unsafe { GetAce(acl, 0, &mut raw_ace) } == 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let ace = unsafe { &*raw_ace.cast::<ACCESS_ALLOWED_ACE>() };
    let ace_sid = (&ace.SidStart as *const u32).cast_mut().cast();
    if ace.Header.AceType != ACCESS_ALLOWED_ACE_TYPE
        || ace.Mask & FILE_ALL_ACCESS != FILE_ALL_ACCESS
        || unsafe { EqualSid(ace_sid, expected_sid) } == 0
    {
        return Err("private DACL does not grant only the current user full control".to_string());
    }
    Ok(())
}

fn wide_path(path: &Path) -> Result<Vec<u16>, String> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(format!("Windows path contains NUL: {}", path.display()));
    }
    wide.push(0);
    Ok(wide)
}

fn classify_unsafe_cleanup(path: &Path, reason: String) -> WindowsCleanupResult {
    match fs::symlink_metadata(path) {
        Ok(_) => WindowsCleanupResult::Retained { reason },
        Err(error) if error.kind() == io::ErrorKind::NotFound => WindowsCleanupResult::Removed,
        Err(error) => WindowsCleanupResult::Unknown {
            reason: format!("{reason}; cleanup state cannot be inspected: {error}"),
        },
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}
