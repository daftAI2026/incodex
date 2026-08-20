use std::fs;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use incodex_core::canonical::{recheck_target, CanonicalTarget};
use sha2::{Digest, Sha256};

use crate::journal::{load_v2, tx_paths, write_journal, JournalV2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FsIdentity {
    pub(crate) device: u64,
    pub(crate) inode: u64,
}

pub(crate) fn directory_identity(path: &Path) -> Result<FsIdentity, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!("path is not a real directory: {}", path.display()));
    }
    Ok(FsIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

pub(crate) fn validate_tree_digest(path: &Path, expected: &str, label: &str) -> Result<(), String> {
    if expected.is_empty() {
        return Err(format!("{label} tree proof is missing from the journal"));
    }
    let actual = tree_digest(path)?;
    if actual != expected {
        return Err(format!("{label} tree proof does not match the journal"));
    }
    Ok(())
}

pub(crate) fn validate_pre_swap_live(
    target: &CanonicalTarget,
    journal: &JournalV2,
) -> Result<(), String> {
    recheck_target(target)?;
    validate_tree_digest(&target.real_path, &journal.pre_swap_digest, "pre-swap live")
}

pub(crate) fn validate_staged_snapshot(
    path: &Path,
    journal: &JournalV2,
) -> Result<FsIdentity, String> {
    let actual = directory_identity(path)?;
    let expected = parse_identity(
        &journal.staged_device,
        &journal.staged_inode,
        "staged target",
    )?;
    require_identity(Some(actual), expected, "staged target")?;
    validate_tree_digest(path, &journal.staged_digest, "staged target")?;
    Ok(actual)
}

enum TreePayload {
    Empty,
    Symlink(Vec<u8>),
    File(PathBuf),
}

struct TreeEntry {
    relative: Vec<u8>,
    kind: u8,
    mode: u32,
    size: u64,
    payload: TreePayload,
}

pub(crate) fn tree_digest(root: &Path) -> Result<String, String> {
    let root_metadata = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if root_metadata.file_type().is_symlink() || !root_metadata.file_type().is_dir() {
        return Err(format!(
            "tree root is not a real directory: {}",
            root.display()
        ));
    }
    let mut entries = vec![TreeEntry {
        relative: Vec::new(),
        kind: b'D',
        mode: root_metadata.mode() & 0o7777,
        size: 0,
        payload: TreePayload::Empty,
    }];
    collect_tree(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    let mut digest = Sha256::new();
    for entry in entries {
        digest.update((entry.relative.len() as u64).to_be_bytes());
        digest.update(entry.relative);
        digest.update([0]);
        digest.update([entry.kind]);
        digest.update([0]);
        digest.update(entry.mode.to_be_bytes());
        digest.update(entry.size.to_be_bytes());
        match entry.payload {
            TreePayload::Empty => {}
            TreePayload::Symlink(target) => {
                digest.update(target);
            }
            TreePayload::File(path) => {
                let mut file = fs::File::open(&path).map_err(|error| error.to_string())?;
                let mut buffer = [0u8; 64 * 1024];
                let mut read = 0u64;
                loop {
                    let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
                    if count == 0 {
                        break;
                    }
                    read += count as u64;
                    if read > entry.size {
                        return Err(format!("file changed while hashing: {}", path.display()));
                    }
                    digest.update(&buffer[..count]);
                }
                if read != entry.size {
                    return Err(format!("file changed while hashing: {}", path.display()));
                }
            }
        }
        digest.update([0]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn collect_tree(root: &Path, current: &Path, entries: &mut Vec<TreeEntry>) -> Result<(), String> {
    let mut children = fs::read_dir(current)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    children.sort_by_key(|entry| entry.file_name());
    for entry in children {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .as_os_str()
            .as_bytes()
            .to_vec();
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        let mode = fs::symlink_metadata(&path)
            .map_err(|error| error.to_string())?
            .mode()
            & 0o7777;
        if file_type.is_symlink() {
            let target = fs::read_link(&path).map_err(|error| error.to_string())?;
            entries.push(TreeEntry {
                relative,
                kind: b'L',
                mode,
                size: target.as_os_str().as_bytes().len() as u64,
                payload: TreePayload::Symlink(target.as_os_str().as_bytes().to_vec()),
            });
        } else if file_type.is_dir() {
            entries.push(TreeEntry {
                relative,
                kind: b'D',
                mode,
                size: 0,
                payload: TreePayload::Empty,
            });
            collect_tree(root, &path, entries)?;
        } else if file_type.is_file() {
            let size = fs::symlink_metadata(&path)
                .map_err(|error| error.to_string())?
                .len();
            entries.push(TreeEntry {
                relative,
                kind: b'F',
                mode,
                size,
                payload: TreePayload::File(path),
            });
        } else {
            return Err(format!("unsupported backup entry type: {}", path.display()));
        }
    }
    Ok(())
}

pub(crate) fn validate_backup_digest(path: &Path, journal: &JournalV2) -> Result<(), String> {
    if journal.backup_digest.is_empty() {
        return Err("durable backup manifest is missing".into());
    }
    validate_tree_digest(path, &journal.backup_digest, "backup snapshot")
}

pub(crate) fn restore_source(root: &Path, journal: &JournalV2) -> Result<PathBuf, String> {
    let paths = tx_paths(root, &journal.install_id);
    match fs::symlink_metadata(&paths.original) {
        Ok(_) => {
            directory_identity(&paths.original)
                .map_err(|error| format!("invalid original restore source: {error}"))?;
            validate_backup_digest(&paths.original, journal)?;
            return Ok(paths.original);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect original restore source: {error}")),
    }
    match fs::symlink_metadata(&paths.outgoing) {
        Ok(_) => {
            directory_identity(&paths.outgoing)
                .map_err(|error| format!("invalid outgoing restore source: {error}"))?;
            validate_backup_digest(&paths.outgoing, journal)?;
            Ok(paths.outgoing)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err("no safe restore source exists".into())
        }
        Err(error) => Err(format!("cannot inspect outgoing restore source: {error}")),
    }
}

pub(crate) fn parse_identity(device: &str, inode: &str, label: &str) -> Result<FsIdentity, String> {
    if device.is_empty() || inode.is_empty() {
        return Err(format!("{label} identity is missing from the journal"));
    }
    Ok(FsIdentity {
        device: device
            .parse()
            .map_err(|_| format!("{label} device identity is invalid"))?,
        inode: inode
            .parse()
            .map_err(|_| format!("{label} inode identity is invalid"))?,
    })
}

pub(crate) fn optional_directory_identity(
    path: &Path,
    label: &str,
) -> Result<Option<FsIdentity>, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => directory_identity(path)
            .map(Some)
            .map_err(|error| format!("{label} is not a real directory: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot inspect {label}: {error}")),
    }
}

pub(crate) fn require_identity(
    actual: Option<FsIdentity>,
    expected: FsIdentity,
    label: &str,
) -> Result<(), String> {
    match actual {
        Some(actual) if actual == expected => Ok(()),
        Some(actual) => Err(format!(
            "{label} identity changed from {}:{} to {}:{}",
            expected.device, expected.inode, actual.device, actual.inode
        )),
        None => Err(format!("{label} is missing")),
    }
}

pub(crate) fn matches_recorded_restore(live: &Path, journal: &JournalV2) -> Result<bool, String> {
    matches_recorded_restore_path(live, journal, "live target")
}

pub(crate) fn matches_recorded_restore_path(
    path: &Path,
    journal: &JournalV2,
    label: &str,
) -> Result<bool, String> {
    if journal.restored_device.is_empty()
        || journal.restored_inode.is_empty()
        || journal.restored_digest.is_empty()
    {
        return Ok(false);
    }
    let expected = parse_identity(
        &journal.restored_device,
        &journal.restored_inode,
        "restored target",
    )?;
    let actual = optional_directory_identity(path, label)?;
    if actual != Some(expected) {
        return Ok(false);
    }
    Ok(tree_digest(path)? == journal.restored_digest)
}

pub(crate) fn record_restore_intent(
    root: &Path,
    candidate: &Path,
    journal: &JournalV2,
) -> Result<JournalV2, String> {
    let identity = directory_identity(candidate)?;
    let digest = tree_digest(candidate)?;
    let mut next = journal.clone();
    next.restored_device = identity.device.to_string();
    next.restored_inode = identity.inode.to_string();
    next.restored_digest = digest;
    next.sequence += 1;
    write_journal(root, &next)?;
    Ok(load_v2(root, &journal.install_id)?)
}
