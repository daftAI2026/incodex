use std::path::Path;

pub(crate) struct VerifiedTarget;

impl VerifiedTarget {
    pub(crate) fn prepare(app: &Path, verify: impl FnOnce() -> Result<(), String>) -> Result<Self, String> {
        let _ = app;
        verify()?;
        Ok(Self)
    }

    pub(crate) fn revalidate(&self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, fs, path::PathBuf, sync::atomic::{AtomicU64, Ordering}};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture { root: PathBuf, app: PathBuf }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("incodex-permission-target-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&root).unwrap();
            let app = root.join("Target.app");
            fs::create_dir(&app).unwrap();
            fs::write(app.join("payload"), b"original").unwrap();
            Self { root, app }
        }
        fn prepare(&self) -> VerifiedTarget { VerifiedTarget::prepare(&self.app, || Ok(())).unwrap() }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.root); } }

    #[test]
    fn unchanged_target_reuses_one_full_verification() {
        let f = Fixture::new();
        let calls = Cell::new(0);
        let guard = VerifiedTarget::prepare(&f.app, || { calls.set(calls.get()+1); Ok(()) }).unwrap();
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
        fs::File::options().write(true).open(path).unwrap()
            .set_times(fs::FileTimes::new().set_modified(old_time)).unwrap();
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
            fs::write(f.app.join("payload"), b"changed").unwrap(); Ok(())
        }).is_err());
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

    #[cfg(unix)]
    #[test]
    fn wrapper_falls_back_to_full_verification_for_unsupported_entries_each_time() {
        use std::os::unix::fs::symlink;

        for (label, setup) in [
            ("special", Box::new(|f: &Fixture| {
                let path = f.app.join("fifo");
                let path_bytes = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(path_bytes.as_ptr(), 0o600) }, 0);
            }) as Box<dyn Fn(&Fixture)>),
            ("external-symlink", Box::new(|f: &Fixture| {
                fs::write(f.root.join("outside"), b"outside").unwrap();
                symlink("../outside", f.app.join("link")).unwrap();
            })),
            ("dangling-symlink", Box::new(|f: &Fixture| {
                symlink("missing", f.app.join("link")).unwrap();
            })),
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
            assert_eq!(calls.get(), 2, "{label} must use full verification every time");
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
        assert_eq!(calls.get(), 1, "drift must not rerun or replace the full verifier");
    }
}
