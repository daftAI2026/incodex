use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch(label: &str) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "incodex-update-{label}-{}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

#[test]
fn update_fails_when_install_script_download_fails() {
    let home = scratch("download-failure");
    let prefix = home.join("prefix");
    let bin = prefix.join("bin");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&fake_bin).unwrap();

    let curl = fake_bin.join("curl");
    fs::write(
        &curl,
        "#!/bin/sh\nprintf '%s\\n' 'simulated download failure' >&2\nexit 22\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&curl).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    fs::set_permissions(&curl, permissions).unwrap();

    let bash_profile = home.join(".bash_profile");
    fs::write(
        &bash_profile,
        format!(
            "export PATH='{}:/usr/bin:/bin'\n",
            fake_bin.display()
        ),
    )
    .unwrap();

    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    let output = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "update unexpectedly succeeded:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("update failed"));
}
