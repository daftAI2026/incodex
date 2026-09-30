//! In-memory continuity proof for one permission guide, not a signing cache.
//! Full validation is bracketed by equal filesystem snapshots. Later checks
//! detect drift without reading every file's contents or repeating inventory.
//! This is scoped to ordinary local APFS mutations, not privileged metadata
//! forgery or an atomic filesystem/TCC transaction.
use std::os::unix::fs::MetadataExt;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, PartialEq, Eq)]
struct Node {
    device: u64,
    inode: u64,
    mode: u32,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    link: Option<PathBuf>,
}

const UNSUPPORTED: &str = "continuity unavailable: ";
type AncestorIdentity = (PathBuf, u64, u64, u32, Option<PathBuf>);

impl Node {
    fn from_metadata(metadata: &fs::Metadata, link: Option<PathBuf>) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            size: metadata.size(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
            link,
        }
    }

    fn matches_directory(&self, path: &Path) -> Result<bool, String> {
        let current = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        Ok(current.is_dir() && Self::from_metadata(&current, None) == *self)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    canonical: PathBuf,
    ancestors: Vec<AncestorIdentity>,
    nodes: BTreeMap<PathBuf, Node>,
}

pub(crate) struct VerifiedTarget {
    app: PathBuf,
    verified: Snapshot,
}

impl VerifiedTarget {
    #[cfg(test)]
    pub(crate) fn prepare(
        app: &Path,
        verify: impl FnOnce() -> Result<(), String>,
    ) -> Result<Self, String> {
        let before = Snapshot::read(app)?;
        Self::prepare_from_snapshot(app, before, verify)
    }

    fn prepare_from_snapshot(
        app: &Path,
        before: Snapshot,
        verify: impl FnOnce() -> Result<(), String>,
    ) -> Result<Self, String> {
        verify()?;
        let after = Snapshot::read(app)?;
        if before != after {
            return Err(
                "permission target changed during full verification; no reset was performed".into(),
            );
        }
        Ok(Self {
            app: app.to_path_buf(),
            verified: after,
        })
    }

    pub(crate) fn revalidate(&self) -> Result<(), String> {
        if Snapshot::read(&self.app)? != self.verified {
            return Err(
                "permission target changed while the guide was open; no reset was performed".into(),
            );
        }
        Ok(())
    }
}

impl Snapshot {
    fn read(app: &Path) -> Result<Self, String> {
        Self::read_with_hook(app, |_| {})
    }

    fn read_with_hook(app: &Path, mut before_directory: impl FnMut(&Path)) -> Result<Self, String> {
        if !app.is_absolute() {
            return Err("permission target must be absolute".into());
        }
        let root = fs::symlink_metadata(app).map_err(|e| e.to_string())?;
        if !root.is_dir() || root.file_type().is_symlink() {
            return Err("permission target must be a real directory".into());
        }
        let canonical = fs::canonicalize(app).map_err(|e| e.to_string())?;
        let ancestors = ancestor_identities(app)?;
        let mut pending = vec![canonical.clone()];
        let mut nodes = BTreeMap::new();
        while let Some(path) = pending.pop() {
            if nodes.len() >= 200_000 {
                return Err(format!("{UNSUPPORTED}too many entries"));
            }
            let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if metadata.dev() != root.dev() {
                return Err(format!("{UNSUPPORTED}nested filesystem"));
            }
            let kind = metadata.file_type();
            let link = if kind.is_symlink() {
                let destination = fs::canonicalize(&path)
                    .map_err(|e| format!("{UNSUPPORTED}unresolved symbolic link: {e}"))?;
                if !destination.starts_with(&canonical) {
                    return Err(format!("{UNSUPPORTED}external symbolic link"));
                }
                Some(fs::read_link(&path).map_err(|e| e.to_string())?)
            } else if kind.is_dir() || kind.is_file() {
                None
            } else {
                return Err(format!("{UNSUPPORTED}special filesystem entry"));
            };
            let relative = path
                .strip_prefix(&canonical)
                .map_err(|e| e.to_string())?
                .to_path_buf();
            let node = Node::from_metadata(&metadata, link);
            if kind.is_dir() {
                before_directory(&path);
                if !node.matches_directory(&path)? {
                    return Err("permission target directory changed before enumeration".into());
                }
                for entry in fs::read_dir(&path).map_err(|e| e.to_string())? {
                    pending.push(entry.map_err(|e| e.to_string())?.path());
                    if nodes.len() + pending.len() > 200_000 {
                        return Err(format!("{UNSUPPORTED}too many entries"));
                    }
                }
                if !node.matches_directory(&path)? {
                    return Err("permission target directory changed during enumeration".into());
                }
            }
            nodes.insert(relative, node);
        }
        // A queued child must not hide a replacement of an already-enumerated
        // ancestor. Recheck every directory after all descendants were visited.
        for (relative, node) in &nodes {
            if node.mode & libc::S_IFMT as u32 == libc::S_IFDIR as u32
                && !node.matches_directory(&canonical.join(relative))?
            {
                return Err("permission target directory changed during inspection".into());
            }
        }
        // Resolve again to reject ancestor-link replacement during traversal.
        if fs::canonicalize(app).map_err(|e| e.to_string())? != canonical
            || ancestor_identities(app)? != ancestors
        {
            return Err("permission target path changed during inspection".into());
        }
        Ok(Self {
            canonical,
            ancestors,
            nodes,
        })
    }
}

fn ancestor_identities(app: &Path) -> Result<Vec<AncestorIdentity>, String> {
    app.ancestors()
        .skip(1)
        .map(|path| {
            let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
            let link = if metadata.file_type().is_symlink() {
                Some(fs::read_link(path).map_err(|e| e.to_string())?)
            } else {
                None
            };
            // Other children changing in a parent directory do not change which
            // target this path selects. Pin identity, not ancestor timestamps.
            Ok((
                path.to_path_buf(),
                metadata.dev(),
                metadata.ino(),
                metadata.mode(),
                link,
            ))
        })
        .collect()
}

pub(crate) fn verifier<'a, F>(
    app: &'a Path,
    use_continuity: bool,
    mut full: F,
) -> impl FnMut() -> Result<(), String> + 'a
where
    F: FnMut() -> Result<(), String> + 'a,
{
    let mut guard: Option<VerifiedTarget> = None;
    let mut fallback = !use_continuity;
    move || {
        if fallback {
            return full();
        }
        if let Some(guard) = &guard {
            return guard.revalidate();
        }
        let before = match Snapshot::read(app) {
            Ok(snapshot) => snapshot,
            Err(error) if error.starts_with(UNSUPPORTED) => {
                // Eligibility is decided before full validation. Never swallow
                // a verifier failure or fall back after a baseline was frozen.
                fallback = true;
                return full();
            }
            Err(error) => return Err(error),
        };
        guard = Some(VerifiedTarget::prepare_from_snapshot(
            app, before, &mut full,
        )?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        app: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "incodex-permission-target-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let app = root.join("Target.app");
            fs::create_dir(&app).unwrap();
            fs::write(app.join("payload"), b"original").unwrap();
            Self { root, app }
        }
        fn prepare(&self) -> VerifiedTarget {
            VerifiedTarget::prepare(&self.app, || Ok(())).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn unchanged_target_reuses_one_full_verification() {
        let f = Fixture::new();
        let calls = Cell::new(0);
        let guard = VerifiedTarget::prepare(&f.app, || {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .unwrap();
        guard.revalidate().unwrap();
        guard.revalidate().unwrap();
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn content_change_with_restored_mtime_is_rejected() {
        let f = Fixture::new();
        let path = f.app.join("payload");
        let old_time = fs::metadata(&path).unwrap().modified().unwrap();
        let guard = f.prepare();
        std::thread::sleep(std::time::Duration::from_millis(2));
        fs::write(&path, b"tampered").unwrap();
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old_time))
            .unwrap();
        assert!(guard.revalidate().is_err());
    }

    #[test]
    fn added_or_removed_entries_are_rejected() {
        let f = Fixture::new();
        let guard = f.prepare();
        fs::write(f.app.join("new"), b"new").unwrap();
        assert!(guard.revalidate().is_err());
        let guard = f.prepare();
        fs::remove_file(f.app.join("payload")).unwrap();
        assert!(guard.revalidate().is_err());
    }

    #[test]
    fn replacement_at_same_app_path_is_rejected() {
        let f = Fixture::new();
        let guard = f.prepare();
        fs::rename(&f.app, f.root.join("old.app")).unwrap();
        fs::create_dir(&f.app).unwrap();
        fs::write(f.app.join("payload"), b"original").unwrap();
        assert!(guard.revalidate().is_err());
    }

    #[test]
    fn full_verification_failure_never_creates_a_guard() {
        let f = Fixture::new();
        assert!(VerifiedTarget::prepare(&f.app, || Err("invalid signature".into())).is_err());
    }

    #[test]
    fn mutation_during_full_verification_is_rejected() {
        let f = Fixture::new();
        assert!(VerifiedTarget::prepare(&f.app, || {
            fs::write(f.app.join("payload"), b"changed").unwrap();
            Ok(())
        })
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_and_symlink_retarget_are_rejected() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        fs::write(f.root.join("outside"), b"external").unwrap();
        symlink("../outside", f.app.join("link")).unwrap();
        assert!(VerifiedTarget::prepare(&f.app, || Ok(())).is_err());
        fs::remove_file(f.app.join("link")).unwrap();
        symlink("payload", f.app.join("link")).unwrap();
        let guard = f.prepare();
        fs::write(f.app.join("second"), b"original").unwrap();
        fs::remove_file(f.app.join("link")).unwrap();
        symlink("second", f.app.join("link")).unwrap();
        assert!(guard.revalidate().is_err());
    }

    #[test]
    fn wrapper_runs_full_verification_once_for_a_stable_target() {
        let f = Fixture::new();
        let calls = Cell::new(0);
        let mut verify = verifier(&f.app, true, || {
            calls.set(calls.get() + 1);
            Ok(())
        });

        verify().unwrap();
        verify().unwrap();
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn nested_directory_replaced_during_enumeration_is_rejected() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let nested = f.app.join("nested");
        fs::create_dir(&nested).unwrap();
        let outside = f.root.join("outside-directory");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("external"), b"not part of the bundle").unwrap();
        let result = Snapshot::read_with_hook(&f.app, |path| {
            if path.ends_with("nested") {
                fs::rename(&nested, f.root.join("saved-directory")).unwrap();
                symlink(&outside, &nested).unwrap();
            }
        });
        assert!(
            result.is_err(),
            "directory replacement must invalidate the traversal"
        );
    }

    #[cfg(unix)]
    #[test]
    fn wrapper_falls_back_to_full_verification_for_unsupported_entries_each_time() {
        use std::os::unix::fs::symlink;

        for (label, setup) in [
            (
                "special",
                Box::new(|f: &Fixture| {
                    let path = f.app.join("fifo");
                    let path_bytes =
                        std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(path_bytes.as_ptr(), 0o600) }, 0);
                }) as Box<dyn Fn(&Fixture)>,
            ),
            (
                "external-symlink",
                Box::new(|f: &Fixture| {
                    fs::write(f.root.join("outside"), b"outside").unwrap();
                    symlink("../outside", f.app.join("link")).unwrap();
                }),
            ),
            (
                "dangling-symlink",
                Box::new(|f: &Fixture| {
                    symlink("missing", f.app.join("link")).unwrap();
                }),
            ),
        ] {
            let f = Fixture::new();
            setup(&f);
            let calls = Cell::new(0);
            let mut verify = verifier(&f.app, true, || {
                calls.set(calls.get() + 1);
                Ok(())
            });

            verify().unwrap_or_else(|error| panic!("{label} fallback failed: {error}"));
            verify().unwrap_or_else(|error| panic!("{label} fallback did not repeat: {error}"));
            assert_eq!(
                calls.get(),
                2,
                "{label} must use full verification every time"
            );
        }
    }

    #[test]
    fn wrapper_drift_fails_closed_without_replacing_its_baseline() {
        let f = Fixture::new();
        let calls = Cell::new(0);
        let mut verify = verifier(&f.app, true, || {
            calls.set(calls.get() + 1);
            Ok(())
        });

        verify().unwrap();
        fs::write(f.app.join("payload"), b"changed").unwrap();
        assert!(verify().is_err());
        fs::write(f.app.join("payload"), b"original").unwrap();
        assert!(verify().is_err());
        assert_eq!(
            calls.get(),
            1,
            "drift must not rerun or replace the full verifier"
        );
    }
}
