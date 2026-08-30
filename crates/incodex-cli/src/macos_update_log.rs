use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::macos_update_restore::{ensure_private_dir, set_file_mode};

const PRIVATE_FILE_MODE: u32 = 0o600;
const COORDINATOR_LOG_MAX_BYTES: usize = 64 * 1024;
const COORDINATOR_LOG_EVENT_MAX_BYTES: usize = 4 * 1024;

pub(super) fn log_coordinator_event(root: &Path, event: &str) {
    let event = event
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let _ = append_coordinator_log(root, &format!("{timestamp} {event}"));
}

fn append_coordinator_log(root: &Path, event: &str) -> Result<(), String> {
    if event.len() > COORDINATOR_LOG_EVENT_MAX_BYTES {
        return Err("macOS update coordinator log event is too large".into());
    }
    let directory = root.join("macos-update");
    ensure_private_dir(&directory)?;
    let path = directory.join("coordinator.log");
    let existing_size = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "refuse to write symlink macOS update coordinator log: {}",
                path.display()
            ));
        }
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(format!(
                "macOS update coordinator log is not a regular file: {}",
                path.display()
            ));
        }
        Ok(metadata) => metadata.len() as usize,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => {
            return Err(format!(
                "cannot inspect macOS update coordinator log: {error}"
            ))
        }
    };
    let truncate = existing_size.saturating_add(event.len() + 1) > COORDINATOR_LOG_MAX_BYTES;
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create(true)
        .append(!truncate)
        .truncate(truncate)
        .mode(PRIVATE_FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW);
    let mut file = options
        .open(&path)
        .map_err(|error| format!("cannot open macOS update coordinator log: {error}"))?;
    file.write_all(event.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_data())
        .map_err(|error| format!("cannot write macOS update coordinator log: {error}"))?;
    set_file_mode(&file, PRIVATE_FILE_MODE)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "incodex-macos-update-log-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn coordinator_log_is_private_bounded_and_never_follows_symlinks() {
        let root = scratch("contract");
        append_coordinator_log(&root, "worker started").unwrap();
        let path = root.join("macos-update/coordinator.log");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            PRIVATE_FILE_MODE
        );

        fs::write(&path, vec![b'x'; COORDINATOR_LOG_MAX_BYTES + 1]).unwrap();
        append_coordinator_log(&root, "worker bounded").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "worker bounded\n");

        fs::remove_file(&path).unwrap();
        let foreign = root.join("foreign.log");
        fs::write(&foreign, b"foreign\n").unwrap();
        std::os::unix::fs::symlink(&foreign, &path).unwrap();
        assert!(append_coordinator_log(&root, "must fail")
            .unwrap_err()
            .contains("symlink"));
        assert_eq!(fs::read(&foreign).unwrap(), b"foreign\n");
    }
}
