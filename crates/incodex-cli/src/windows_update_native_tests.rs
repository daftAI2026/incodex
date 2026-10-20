use std::fs;
use std::path::{Path, PathBuf};

use incodex_core::windows_session::{apply_private_windows_acl, ensure_private_windows_dir};

struct ManagedFixture(PathBuf);

impl ManagedFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-native-publication-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let package = ensure_private_windows_dir(&root).unwrap();
        let releases = ensure_private_windows_dir(&package.join("releases")).unwrap();
        for version in ["1.0.0", "2.0.0"] {
            let directory = ensure_private_windows_dir(&releases.join(version)).unwrap();
            // 复现安装器 Copy-Item 的继承 ACL，不替代消费者的实际权限验证。
            fs::write(directory.join("incodex.exe"), version).unwrap();
        }
        fs::write(package.join("current"), "2.0.0\n").unwrap();
        Self(package)
    }

    fn executable(&self, version: &str) -> PathBuf {
        self.0.join("releases").join(version).join("incodex.exe")
    }
}

impl Drop for ManagedFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn version_probe(path: &Path, expected: &str) -> Result<(), String> {
    let observed = fs::read_to_string(path).map_err(|error| error.to_string())?;
    if observed == expected {
        Ok(())
    } else {
        Err("fixture version does not match the requested generation".into())
    }
}

#[test]
fn runtime_publication_rejects_the_acl_that_the_old_helper_would_reject() {
    let fixture = ManagedFixture::new();
    let error = super::managed_native_open_generation(&fixture.0, "2.0.0", |_, _| {
        panic!("unsafe executable must be rejected before its version probe")
    })
    .expect_err("publication must not select a CLI that the installed helper cannot execute");
    assert!(error.contains("private DACL is not protected"), "{error}");

    apply_private_windows_acl(&fixture.executable("2.0.0")).unwrap();
    let (path, version) =
        super::managed_native_open_generation(&fixture.0, "2.0.0", version_probe).unwrap();
    assert_eq!(path, fixture.executable("2.0.0"));
    assert_eq!(version, "2.0.0");
}

#[test]
fn native_publication_validation_is_bound_to_the_requested_retained_generation() {
    let fixture = ManagedFixture::new();
    for version in ["1.0.0", "2.0.0"] {
        apply_private_windows_acl(&fixture.executable(version)).unwrap();
    }
    let (path, version) =
        super::managed_native_open_generation(&fixture.0, "1.0.0", version_probe).unwrap();
    assert_eq!(path, fixture.executable("1.0.0"));
    assert_eq!(version, "1.0.0");
}
