use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const FILE_MODE: u32 = 0o600;
const DIR_MODE: u32 = 0o700;

pub fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let mut perms = fs::metadata(dir).map_err(|err| err.to_string())?.permissions();
    perms.set_mode(DIR_MODE);
    fs::set_permissions(dir, perms).map_err(|err| err.to_string())?;
    Ok(())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("durable write needs a parent directory")?;
    ensure_private_dir(parent)?;
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(".{}.tmp", n));
    {
        let mut opts = OpenOptions::new();
        opts.write(true)
            .create_new(true)
            .mode(FILE_MODE)
            .custom_flags(libc::O_NOFOLLOW);
        let mut file = opts.open(&tmp).map_err(|err| err.to_string())?;
        file.write_all(bytes).map_err(|err| err.to_string())?;
        file.sync_data().map_err(|err| err.to_string())?;
        unsafe {
            libc::fchmod(file.as_raw_fd(), FILE_MODE as libc::mode_t);
        }
    }
    fs::rename(&tmp, path).map_err(|err| err.to_string())?;
    sync_dir(parent)?;
    let mut perms = fs::metadata(path).map_err(|err| err.to_string())?.permissions();
    perms.set_mode(FILE_MODE);
    fs::set_permissions(path, perms).map_err(|err| err.to_string())?;
    Ok(())
}

pub(crate) fn sync_dir(dir: &Path) -> Result<(), String> {
    let file = OpenOptions::new()
        .read(true)
        .open(dir)
        .map_err(|err| err.to_string())?;
    file.sync_all().map_err(|err| err.to_string())
}
