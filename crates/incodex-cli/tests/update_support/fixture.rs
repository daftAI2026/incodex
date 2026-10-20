/*
 * [INPUT]: 依赖原生 CLI 产物和当前测试进程的临时目录。
 * [OUTPUT]: 提供更新套件共用的独立安装代际与替身工具。
 * [POS]: 更新测试共享夹具；不读真实账号或执行真实包更新。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

pub(super) fn scratch(label: &str) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "incodex-update-{label}-{}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

pub(super) fn write_executable(path: &std::path::Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

pub(super) fn write_runtime_cli(path: &std::path::Path, version: &str) {
    write_executable(
        path,
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s\\n' 'Incodex version {version}' ;;\n  runtime) exit 0 ;;\n  *) exit 88 ;;\nesac\n"
        ),
    );
}

pub(super) fn runtime_pending_marker(version: &str, manifest_sha256: &str) -> String {
    format!(
        "{}\n",
        serde_json::json!({
            "schemaVersion": 1,
            "version": version,
            "manifestSha256": manifest_sha256,
        })
    )
}

pub(super) fn embedded_runtime_pending_marker() -> String {
    let identity = incodex_runtime_bundle::runtime_identity().unwrap();
    runtime_pending_marker(&identity.version, &identity.manifest_sha256)
}

pub(super) fn installed_cli(home: &std::path::Path) -> (PathBuf, PathBuf) {
    let prefix = home.join("prefix");
    let bin = prefix.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    (prefix, installed)
}

pub(super) fn homebrew_cli(home: &std::path::Path) -> PathBuf {
    homebrew_cli_generation(home, env!("CARGO_PKG_VERSION"))
}

pub(super) fn homebrew_cli_generation(home: &std::path::Path, generation: &str) -> PathBuf {
    let bin = home.join(format!("Cellar/incodex/{generation}/bin"));
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    installed
}

pub(super) fn intel_homebrew_cli(home: &std::path::Path) -> PathBuf {
    let bin = home.join("usr/local/opt/incodex/bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    installed
}

pub(super) fn usr_local_script_cli(home: &std::path::Path) -> PathBuf {
    let bin = home.join("usr/local/bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    installed
}
