use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) fn scratch(label: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let seq = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let root = env::temp_dir().join(format!("incodex-tx-{label}-{now}-{seq}"));
    fs::create_dir_all(&root).expect("scratch");
    root
}

pub(crate) fn make_app(root: &Path, name: &str, marker: &str) -> PathBuf {
    let app = root.join(name);
    fs::create_dir_all(app.join("Contents/MacOS")).expect("app dirs");
    fs::create_dir_all(app.join("Contents/Resources/nested")).expect("resource dirs");
    fs::write(app.join("marker"), format!("{marker}\n")).expect("marker");
    fs::write(
        app.join("Contents/MacOS/ChatGPT"),
        format!("binary-{marker}\n"),
    )
    .expect("binary");
    fs::write(
        app.join("Contents/Resources/nested/config.json"),
        format!("{{\"marker\":\"{marker}\"}}\n"),
    )
    .expect("resource");
    symlink(
        "nested/config.json",
        app.join("Contents/Resources/current-config.json"),
    )
    .expect("resource symlink");
    app
}

pub(crate) fn seed_original(root: &Path, app: &Path, install_id: &str) {
    let destination = root
        .join("transactions")
        .join(install_id)
        .join("original/ChatGPT.app");
    copy_tree(app, &destination);
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("copy destination");
    for entry in fs::read_dir(from).expect("copy source") {
        let entry = entry.expect("copy entry");
        let source = entry.path();
        let destination = to.join(entry.file_name());
        let file_type = entry.file_type().expect("copy type");
        if file_type.is_symlink() {
            symlink(
                fs::read_link(&source).expect("copy link target"),
                &destination,
            )
            .expect("copy symlink");
        } else if file_type.is_dir() {
            copy_tree(&source, &destination);
        } else {
            fs::copy(source, destination).expect("copy file");
        }
    }
}

pub(crate) fn tree_digest(root: &Path) -> String {
    let mut entries = Vec::new();
    collect_files(root, root, &mut entries);
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = Sha256::new();
    for (relative, bytes) in entries {
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(bytes);
        digest.update([0]);
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn collect_files(root: &Path, current: &Path, entries: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(current).expect("digest directory") {
        let entry = entry.expect("digest entry");
        let path = entry.path();
        let file_type = entry.file_type().expect("digest type");
        if file_type.is_symlink() {
            let relative = path
                .strip_prefix(root)
                .expect("digest relative link")
                .to_string_lossy()
                .into_owned();
            let target = fs::read_link(&path)
                .expect("digest link target")
                .to_string_lossy()
                .into_owned()
                .into_bytes();
            entries.push((relative, target));
        } else if file_type.is_dir() {
            collect_files(root, &path, entries);
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("digest relative path")
                .to_string_lossy()
                .into_owned();
            entries.push((relative, fs::read(path).expect("digest file")));
        }
    }
}
