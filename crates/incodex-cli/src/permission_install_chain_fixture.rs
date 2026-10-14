/*
 * [INPUT]: 依赖临时 HOME、合成 macOS App、真实事务引擎与 ad-hoc 签名器。
 * [OUTPUT]: 提供已 COMMITTED 且通过生产快照/签名验证的安装夹具。
 * [POS]: 权限安装链测试共用夹具；不改变进程 HOME，不触碰真实 App 或 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#![cfg(target_os = "macos")]

use incodex_asar::{pack_dir, patch_asar};
use incodex_macos::{ditto, sign_app, write_asar_integrity};
use incodex_transaction::{validate_committed_live_snapshot, Engine};
use std::fs::{self, DirBuilder};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) struct Fixture {
    pub(super) sandbox: PathBuf,
    pub(super) root: PathBuf,
    pub(super) app: PathBuf,
    pub(super) install_id: String,
}

impl Fixture {
    pub(super) fn new(label: &str) -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let label = safe_label(label);
        let sandbox = std::env::temp_dir()
            .canonicalize()
            .expect("canonical system temp directory")
            .join(format!(
                "incodex-permission-install-{}-{sequence}-{label}",
                std::process::id(),
            ));
        create_private_dir(&sandbox).expect("create private install fixture sandbox");

        let home = sandbox.join("home");
        let root = home.join(".incodex");
        let app = home.join("Applications/ChatGPT.app");
        let mut fixture = Self {
            sandbox,
            root,
            app,
            install_id: String::new(),
        };
        fixture
            .create_committed_install()
            .unwrap_or_else(|error| panic!("create committed install fixture: {error}"));
        fixture
    }

    fn create_committed_install(&mut self) -> Result<(), String> {
        let home = self
            .root
            .parent()
            .ok_or("fixture root has no HOME parent")?;
        create_private_dir(home)?;
        create_private_dir(&self.root)?;
        create_synthetic_app(&self.app, &self.sandbox)?;

        let mut transaction = Engine::begin(&self.root, &self.app, "permission-install-fixture")?;
        self.install_id = transaction.install_id().to_owned();
        let original = self
            .root
            .join("transactions")
            .join(&self.install_id)
            .join(&transaction.journal().paths.original);
        fs::create_dir_all(
            original
                .parent()
                .ok_or("fixture original backup has no parent")?,
        )
        .map_err(|error| error.to_string())?;
        ditto(&self.app, &original)?;
        transaction.mark_backup_committed()?;

        let staging_parent = self.sandbox.join("staging");
        fs::create_dir(&staging_parent).map_err(|error| error.to_string())?;
        let staging = staging_parent.join("ChatGPT.app");
        ditto(&self.app, &staging)?;
        let asar = staging.join("Contents/Resources/app.asar");
        let (asar_hash, _) = patch_asar(
            &asar,
            incodex_runtime_bundle::loader_source(),
            Some(&self.install_id),
        )?;
        write_asar_integrity(&staging, &asar_hash)?;
        sign_app(&staging)?;

        transaction.place_staging(&staging)?;
        transaction.swap()?;
        transaction.commit()?;

        let transactions = self.root.join("transactions");
        let transaction_dir = transactions.join(&self.install_id);
        for path in [&self.root, &transactions, &transaction_dir] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|error| error.to_string())?;
        }

        validate_committed_live_snapshot(&self.root, &self.install_id, &self.app)?;
        crate::macos_signing::verify_patched(&self.root, &self.app, None)
            .map_err(|error| format!("ad-hoc patched bundle verification failed: {error}"))?;
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.sandbox);
    }
}

fn create_synthetic_app(app: &Path, sandbox: &Path) -> Result<(), String> {
    let contents = app.join("Contents");
    fs::create_dir_all(contents.join("Resources")).map_err(|error| error.to_string())?;
    fs::create_dir_all(contents.join("MacOS")).map_err(|error| error.to_string())?;

    let sandbox_name = sandbox
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("fixture sandbox name is invalid")?;
    let bundle_id = format!("com.daftai.incodex.fixture.{sandbox_name}");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>{bundle_id}</string>
  <key>CFBundleName</key><string>ChatGPT</string>
  <key>CFBundleDisplayName</key><string>ChatGPT</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>
"#
    );
    fs::write(contents.join("Info.plist"), plist).map_err(|error| error.to_string())?;

    let executable = contents.join("MacOS/ChatGPT");
    fs::copy("/usr/bin/true", &executable).map_err(|error| {
        format!("copy macOS system executable into synthetic App: {error}")
    })?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;

    let source = sandbox.join("asar-source");
    fs::create_dir_all(&source).map_err(|error| error.to_string())?;
    fs::write(
        source.join("package.json"),
        b"{\"main\":\"main.js\"}\n",
    )
    .map_err(|error| error.to_string())?;
    fs::write(source.join("main.js"), b"module.exports = {};\n")
        .map_err(|error| error.to_string())?;
    pack_dir(&source, &contents.join("Resources/app.asar"))?;
    Ok(())
}

fn create_private_dir(path: &Path) -> Result<(), String> {
    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    builder.create(path).map_err(|error| error.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())
}

fn safe_label(label: &str) -> String {
    label
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect()
}
