use std::env;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_asar::pack_dir;
use serde_json::Value;
use sha2::{Digest, Sha256};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "incodex-release-{label}-{}-{now}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ReleaseRunner {
    binary: PathBuf,
    arch: String,
}

impl ReleaseRunner {
    fn from_environment() -> Self {
        let binary = PathBuf::from(
            env::var("INCODEX_RELEASE_BINARY").expect("INCODEX_RELEASE_BINARY is required"),
        );
        assert!(
            binary.is_absolute(),
            "release binary must be an absolute path"
        );
        assert!(
            binary.is_file(),
            "release binary does not exist: {}",
            binary.display()
        );
        let arch = env::var("INCODEX_RELEASE_ARCH").expect("INCODEX_RELEASE_ARCH is required");
        assert!(
            matches!(arch.as_str(), "arm64" | "x86_64"),
            "unsupported release arch: {arch}"
        );
        Self { binary, arch }
    }

    fn run(&self, args: &[&str], home: &Path, codex_home: &Path) -> Output {
        let mut command = if self.arch == "x86_64" {
            let mut command = Command::new("/usr/bin/arch");
            command.args(["-x86_64"]).arg(&self.binary);
            command
        } else {
            Command::new(&self.binary)
        };
        command
            .args(args)
            .env("HOME", home)
            .env("CODEX_HOME", codex_home)
            .env("TERM", "dumb")
            .env("NO_COLOR", "1")
            .env("SHELL", "/bin/zsh")
            .output()
            .expect("run release asset")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SnapshotKind {
    Directory,
    File { size: u64, sha256: [u8; 32] },
    Symlink { target: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotEntry {
    relative: PathBuf,
    mode: u32,
    kind: SnapshotKind,
}

type Snapshot = Vec<SnapshotEntry>;

fn snapshot(root: &Path) -> Snapshot {
    let metadata = fs::symlink_metadata(root).expect("snapshot root metadata");
    let mut entries = vec![SnapshotEntry {
        relative: PathBuf::new(),
        mode: metadata.mode() & 0o7777,
        kind: SnapshotKind::Directory,
    }];
    collect_snapshot(root, root, &mut entries);
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    entries
}

fn collect_snapshot(root: &Path, current: &Path, entries: &mut Snapshot) {
    let mut children = fs::read_dir(current)
        .unwrap_or_else(|error| panic!("read {}: {error}", current.display()))
        .collect::<Result<Vec<_>, _>>()
        .expect("read directory entries");
    children.sort_by_key(|entry| entry.file_name());
    for entry in children {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .expect("snapshot path")
            .to_path_buf();
        let metadata = fs::symlink_metadata(&path).expect("snapshot metadata");
        let mode = metadata.mode() & 0o7777;
        let kind = if metadata.file_type().is_symlink() {
            SnapshotKind::Symlink {
                target: fs::read_link(&path).expect("snapshot symlink target"),
            }
        } else if metadata.is_dir() {
            collect_snapshot(root, &path, entries);
            SnapshotKind::Directory
        } else if metadata.is_file() {
            let bytes = fs::read(&path).expect("snapshot file");
            SnapshotKind::File {
                size: bytes.len() as u64,
                sha256: sha256(&bytes),
            }
        } else {
            panic!("unsupported snapshot entry: {}", path.display());
        };
        entries.push(SnapshotEntry {
            relative,
            mode,
            kind,
        });
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn write_executable(path: &Path, source: &str) {
    let source_path = path.with_extension("c");
    fs::write(&source_path, source).expect("write C source");
    let status = Command::new("cc")
        .args(["-x", "c"])
        .arg(&source_path)
        .args(["-o"])
        .arg(path)
        .status()
        .expect("compile Mach-O fixture");
    assert!(status.success(), "cc failed for {}", path.display());
    fs::remove_file(source_path).expect("remove C source");
    let mut permissions = fs::metadata(path).expect("Mach-O metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("Mach-O permissions");
}

fn ad_hoc_sign(path: &Path) {
    let status = Command::new("codesign")
        .args(["--force", "--sign", "-", "--timestamp=none", "--"])
        .arg(path)
        .status()
        .expect("codesign fixture");
    assert!(status.success(), "codesign failed for {}", path.display());
}

fn fixture_app(root: &Path) -> PathBuf {
    let app = root.join("ChatGPT.app");
    let contents = app.join("Contents");
    let resources = contents.join("Resources");
    let macos = contents.join("MacOS");
    fs::create_dir_all(&resources).expect("fixture resources");
    fs::create_dir_all(&macos).expect("fixture MacOS");
    fs::write(
        contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.example.incodex-release-smoke</string>
  <key>CFBundleExecutable</key><string>ChatGPT</string>
  <key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#,
    )
    .expect("fixture Info.plist");
    write_executable(&macos.join("ChatGPT"), "int main(void) { return 0; }\n");

    let cua = contents.join("Frameworks/Codex Computer Use.app");
    let cua_contents = cua.join("Contents");
    fs::create_dir_all(cua_contents.join("MacOS")).expect("fixture CUA");
    fs::write(
        cua_contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.example.incodex-cua</string>
  <key>CFBundleExecutable</key><string>CodexComputerUse</string>
  <key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#,
    )
    .expect("fixture CUA Info.plist");
    write_executable(
        &cua_contents.join("MacOS/CodexComputerUse"),
        "int main(void) { return 0; }\n",
    );
    ad_hoc_sign(&cua);

    let asar_source = root.join("asar-source");
    fs::create_dir_all(&asar_source).expect("fixture ASAR source");
    fs::write(
        asar_source.join("package.json"),
        r#"{"main":"index.js"}
"#,
    )
    .expect("fixture package.json");
    fs::write(
        asar_source.join("index.js"),
        "module.exports = 'release-smoke';\n",
    )
    .expect("fixture index.js");
    pack_dir(&asar_source, &resources.join("app.asar")).expect("pack fixture ASAR");

    // Sign the outer bundle after its nested vendor sidecar and ASAR exist.
    ad_hoc_sign(&app);
    app
}

fn assert_success(output: Output, command: &str) {
    assert!(
        output.status.success(),
        "{command} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_json_success(output: Output, command: &str) {
    assert!(
        output.status.success(),
        "{command} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice::<Value>(&output.stdout)
        .unwrap_or_else(|error| panic!("{command} did not emit JSON: {error}"));
}

#[ignore]
#[test]
fn release_asset_behavior_smoke() {
    if !cfg!(target_os = "macos") {
        eprintln!("release asset smoke requires macOS");
        return;
    }
    let runner = ReleaseRunner::from_environment();
    let temp = TempDir::new(&runner.arch);
    let home = temp.path().join("home");
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&home).expect("smoke HOME");
    fs::create_dir_all(&codex_home).expect("smoke CODEX_HOME");
    fs::write(codex_home.join("auth.json"), "{}\n").expect("smoke auth");
    fs::write(codex_home.join("config.toml"), "model = \"smoke\"\n").expect("smoke config");
    let app = fixture_app(temp.path());

    assert_success(runner.run(&["--version"], &home, &codex_home), "--version");
    assert_success(runner.run(&["--help"], &home, &codex_home), "--help");

    // The release contract exercises Command::new(binary).arg("--version") and
    // arg("--help"),
    // args(["status", "--json"]),
    // args(["open", "--dry-run"]), args(["install", "--yes"]),
    // and args(["uninstall", "--yes"]) against this custom fixture.
    let status_before = runner.run(
        &["status", "--json", "--app", app.to_str().unwrap()],
        &home,
        &codex_home,
    );
    assert_json_success(status_before, "status --json before install");
    let dry_run_before = snapshot(&app);
    assert_success(
        runner.run(
            &["open", "--dry-run", "--app", app.to_str().unwrap()],
            &home,
            &codex_home,
        ),
        "open --dry-run",
    );
    assert_eq!(
        snapshot(&app),
        dry_run_before,
        "open --dry-run mutated fixture"
    );

    let original_snapshot = snapshot(&app);
    assert_success(
        runner.run(
            &["install", "--yes", "--app", app.to_str().unwrap()],
            &home,
            &codex_home,
        ),
        "install --yes",
    );
    let installed_snapshot = snapshot(&app);
    assert_ne!(
        installed_snapshot, original_snapshot,
        "install did not mutate fixture"
    );

    let status_after = runner.run(
        &["status", "--json", "--app", app.to_str().unwrap()],
        &home,
        &codex_home,
    );
    assert_json_success(status_after, "status --json after install");
    assert_success(
        runner.run(
            &["uninstall", "--yes", "--app", app.to_str().unwrap()],
            &home,
            &codex_home,
        ),
        "uninstall --yes",
    );
    let restored_snapshot = snapshot(&app);
    assert_eq!(restored_snapshot, original_snapshot);
}
