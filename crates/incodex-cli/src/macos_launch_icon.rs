//! [INPUT]: 依赖稳定 CLI、宿主当前系统图标、原生铭牌合成与精确旧代摘要。
//! [OUTPUT]: 验证并原子发布版本 4 的宿主铭牌 Finder 启动器，并清理精确自有入口。
//! [POS]: 以固定名称维护自有 Finder 入口；拒绝外部同名 bundle，不改宿主。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

const EXECUTABLE_NAME: &str = "IncodexLauncher";
const ICON_FILENAME: &str = "IncodexLauncher.icns";
const BUNDLE_NAME: &str = "ChatGPT - Inc.app";
const LEGACY_BUNDLE_NAME: &str = "Incodex Launcher.app";
#[cfg(test)]
const ICON_BYTES: &[u8] = include_bytes!("../assets/incodex-launcher.icns");
const OWNER_MARKER: &str = ".incodex-launcher-owner";
const LEGACY_ICON_SHA256: &str = "85f610d2455bc42a43fd0943773a330e6c7d518826af7c53d0c102d25c219150";
const OWNER_MARKER_CONTENT: &[u8] = b"incodex-launcher-owner-v1\n";
const VERSION_3_INFO_PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>IncodexLauncher</string><key>CFBundleIdentifier</key><string>com.daftai.incodex.launcher</string><key>CFBundleName</key><string>ChatGPT - Inc</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>3</string><key>CFBundleIconFile</key><string>IncodexLauncher.icns</string><key>LSUIElement</key><true/></dict></plist>\n";
const INFO_PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>IncodexLauncher</string><key>CFBundleIdentifier</key><string>com.daftai.incodex.launcher</string><key>CFBundleName</key><string>ChatGPT - Inc</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>4</string><key>CFBundleIconFile</key><string>IncodexLauncher.icns</string><key>LSUIElement</key><true/></dict></plist>\n";
const VERSION_2_INFO_PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>IncodexLauncher</string><key>CFBundleIdentifier</key><string>com.daftai.incodex.launcher</string><key>CFBundleName</key><string>Incodex Launcher</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>2</string><key>CFBundleIconFile</key><string>IncodexLauncher.icns</string><key>LSUIElement</key><true/></dict></plist>\n";
const LEGACY_INFO_PLIST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>IncodexLauncher</string><key>CFBundleIdentifier</key><string>com.daftai.incodex.launcher</string><key>CFBundleName</key><string>Incodex Launcher</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>LSUIElement</key><true/></dict></plist>\n";
const SHELL_HEADER: &str = "#!/bin/sh\n";
const DETACHED_MARKER: &str = "# incodex-launcher-detached-v1";

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn launcher_path(home: &Path) -> PathBuf {
    home.join("Applications").join(BUNDLE_NAME)
}

pub(crate) fn legacy_launcher_path(home: &Path) -> PathBuf {
    home.join("Applications").join(LEGACY_BUNDLE_NAME)
}

#[derive(Debug, Default)]
pub(crate) struct LauncherInventory {
    pub(crate) owned: Vec<PathBuf>,
    pub(crate) unknown: Vec<(PathBuf, String)>,
}

#[derive(Debug, Default)]
pub(crate) struct LauncherRemoval {
    pub(crate) removed: Vec<PathBuf>,
    pub(crate) warnings: Vec<(PathBuf, String)>,
}

pub(crate) fn inspect_launchers(home: &Path) -> LauncherInventory {
    let paths = [launcher_path(home), legacy_launcher_path(home)];
    let applications = home.join("Applications");
    let mut inventory = LauncherInventory::default();
    match fs::symlink_metadata(&applications) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return inventory,
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {}
        Ok(_) => {
            let reason = format!(
                "user Applications path is not a real directory: {}",
                applications.display()
            );
            inventory
                .unknown
                .extend(paths.into_iter().map(|path| (path, reason.clone())));
            return inventory;
        }
        Err(error) => {
            let reason = format!(
                "cannot inspect user Applications path {}: {error}",
                applications.display()
            );
            inventory
                .unknown
                .extend(paths.into_iter().map(|path| (path, reason.clone())));
            return inventory;
        }
    }

    for path in paths {
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => inventory
                .unknown
                .push((path.clone(), format!("cannot inspect launcher: {error}"))),
            Ok(_) => match verify_replaceable_bundle(&path) {
                Ok(()) => inventory.owned.push(path),
                Err(error) => inventory.unknown.push((path, error)),
            },
        }
    }
    inventory
}

pub(crate) fn remove_confirmed_launchers(
    home: &Path,
    confirmed: &[PathBuf],
    reject_unknown: bool,
) -> Result<LauncherRemoval, String> {
    let allowed = [launcher_path(home), legacy_launcher_path(home)];
    if confirmed.iter().any(|path| !allowed.contains(path)) {
        return Err("refusing to remove a launcher outside the fixed Incodex paths".into());
    }
    let root = incodex_core::paths::user_root_from(home);
    let _lock = incodex_transaction::acquire_target_lock(
        &root,
        &launcher_path(home),
        "launch-remove",
        None,
    )?;
    let current = inspect_launchers(home);
    if reject_unknown && !current.unknown.is_empty() {
        return Err(unknown_launcher_error(&current.unknown));
    }
    if let Some(path) = current.owned.iter().find(|path| !confirmed.contains(path)) {
        return Err(format!(
            "launcher inventory changed; rerun self-uninstall to confirm: {}",
            path.display()
        ));
    }

    let mut report = LauncherRemoval::default();
    report.warnings.extend(current.unknown.iter().cloned());
    for path in confirmed {
        if current.owned.contains(path) {
            remove_exact_owned_bundle(path)?;
            report.removed.push(path.clone());
        }
    }
    Ok(report)
}

pub(crate) fn unknown_launcher_error(unknown: &[(PathBuf, String)]) -> String {
    let details = unknown
        .iter()
        .map(|(path, reason)| format!("{} ({reason})", path.display()))
        .collect::<Vec<_>>()
        .join(", ");
    format!("refusing to remove an unrecognized launcher: {details}")
}

pub(crate) fn publish_launcher(home: &Path, cli: &Path, app: &Path) -> Result<PathBuf, String> {
    publish_launcher_with_renderer(home, cli, app, crate::macos_launcher_artwork::render)
}

fn publish_launcher_with_renderer(
    home: &Path,
    cli: &Path,
    app: &Path,
    render: impl FnOnce(&Path) -> Result<Vec<u8>, String>,
) -> Result<PathBuf, String> {
    let resolved_home = resolve_directory(home, "user HOME")?;
    let stable_cli = crate::macos_launch_cli::stable_entry(cli, &resolved_home)?;
    resolve_executable(&stable_cli)?;
    let resolved_app = resolve_directory(app, "official App")?;
    let script = launcher_script(&stable_cli, &resolved_app)?;
    let destination = launcher_path(&resolved_home);
    let _lock = incodex_transaction::acquire_target_lock(
        &incodex_core::paths::user_root_from(&resolved_home),
        &destination,
        "launch",
        None,
    )?;
    let applications = ensure_applications_directory(&resolved_home)?;
    let legacy = legacy_launcher_path(&resolved_home);
    let previous_exists = inspect_destination(&destination)?;
    let legacy_exists = inspect_destination(&legacy)?;
    let icon = render(&resolved_app)?;
    if !is_icns(&icon) {
        return Err("renderer did not produce a valid ICNS container".into());
    }
    let staging = create_staging_directory(&applications)?;

    if let Err(error) = populate_bundle(&staging, script.as_bytes(), &icon) {
        remove_created_path(&staging);
        return Err(error);
    }
    if let Err(error) = verify_owned_bundle(&staging) {
        remove_created_path(&staging);
        return Err(error);
    }

    let current_exists = match inspect_destination(&destination) {
        Ok(exists) => exists,
        Err(error) => {
            remove_created_path(&staging);
            return Err(error);
        }
    };
    if current_exists != previous_exists {
        remove_created_path(&staging);
        return Err("launcher destination changed during publication".into());
    }
    let current_legacy_exists = match inspect_destination(&legacy) {
        Ok(exists) => exists,
        Err(error) => {
            remove_created_path(&staging);
            return Err(error);
        }
    };
    if current_legacy_exists != legacy_exists {
        remove_created_path(&staging);
        return Err("legacy launcher destination changed during publication".into());
    }

    if previous_exists {
        let backup = match unique_sibling(&applications, "previous") {
            Ok(path) => path,
            Err(error) => {
                remove_created_path(&staging);
                return Err(error);
            }
        };
        if let Err(error) = fs::rename(&destination, &backup) {
            remove_created_path(&staging);
            return Err(format!(
                "cannot retain the existing Incodex Launcher before update: {error}"
            ));
        }
        if let Err(error) = fs::rename(&staging, &destination) {
            return Err(restore_previous_bundle(
                &destination,
                &backup,
                &staging,
                error,
            ));
        }
        remove_owned_bundle(&backup);
    } else if let Err(error) = fs::rename(&staging, &destination) {
        remove_created_path(&staging);
        return Err(format!("cannot publish the Incodex Launcher: {error}"));
    }

    if legacy_exists {
        remove_exact_owned_bundle(&legacy)?;
    }
    Ok(launcher_path(home))
}

fn resolve_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let resolved = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve {label} {}: {error}", path.display()))?;
    let metadata = fs::symlink_metadata(&resolved)
        .map_err(|error| format!("cannot inspect {label} {}: {error}", resolved.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "{label} is not a real directory: {}",
            resolved.display()
        ));
    }
    Ok(resolved)
}

fn resolve_executable(path: &Path) -> Result<PathBuf, String> {
    let resolved = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve Incodex CLI {}: {error}", path.display()))?;
    let metadata = fs::symlink_metadata(&resolved)
        .map_err(|error| format!("cannot inspect Incodex CLI {}: {error}", resolved.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "Incodex CLI is not a regular file: {}",
            resolved.display()
        ));
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!(
            "Incodex CLI is not executable: {}",
            resolved.display()
        ));
    }
    Ok(resolved)
}

fn launcher_script(cli: &Path, app: &Path) -> Result<String, String> {
    let cli = shell_quote(cli)?;
    let app = shell_quote(app)?;
    Ok(format!("{SHELL_HEADER}{DETACHED_MARKER}\n/usr/bin/nohup {cli} launch --app {app} </dev/null >/dev/null 2>&1 &\n"))
}

fn shell_quote(path: &Path) -> Result<String, String> {
    let value = path
        .to_str()
        .ok_or_else(|| format!("launcher path is not valid UTF-8: {}", path.display()))?;
    if value.contains(['\n', '\r']) {
        return Err(format!("launcher path contains a line break: {value:?}"));
    }
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

fn ensure_applications_directory(home: &Path) -> Result<PathBuf, String> {
    let applications = home.join("Applications");
    match fs::symlink_metadata(&applications) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {}
        Ok(_) => {
            return Err(format!(
                "user Applications path is not a real directory: {}",
                applications.display()
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&applications).map_err(|error| {
                format!(
                    "cannot create user Applications directory {}: {error}",
                    applications.display()
                )
            })?;
        }
        Err(error) => {
            return Err(format!(
                "cannot inspect user Applications directory {}: {error}",
                applications.display()
            ));
        }
    }
    let metadata = fs::symlink_metadata(&applications).map_err(|error| {
        format!(
            "cannot verify user Applications directory {}: {error}",
            applications.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "user Applications path changed during publication: {}",
            applications.display()
        ));
    }
    Ok(applications)
}

fn inspect_destination(destination: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "refusing symlink at launcher destination: {}",
            destination.display()
        )),
        Ok(metadata) if !metadata.is_dir() => Err(format!(
            "refusing non-bundle at launcher destination: {}",
            destination.display()
        )),
        Ok(_) => {
            verify_replaceable_bundle(destination)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "cannot inspect launcher destination {}: {error}",
            destination.display()
        )),
    }
}

fn create_staging_directory(applications: &Path) -> Result<PathBuf, String> {
    for _ in 0..32 {
        let path = unique_sibling(applications, "staging")?;
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "cannot create private launcher staging directory {}: {error}",
                    path.display()
                ));
            }
        }
    }
    Err("cannot allocate a unique launcher staging directory".into())
}

fn unique_sibling(applications: &Path, role: &str) -> Result<PathBuf, String> {
    for _ in 0..32 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = applications.join(format!(
            ".incodex-launcher-{role}-{}-{sequence}",
            process::id()
        ));
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(error) => {
                return Err(format!(
                    "cannot inspect launcher transaction path {}: {error}",
                    path.display()
                ));
            }
            Ok(_) => {}
        }
    }
    Err(format!("cannot allocate a unique launcher {role} path"))
}

fn populate_bundle(staging: &Path, script: &[u8], icon: &[u8]) -> Result<(), String> {
    let contents = staging.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir(&contents)
        .map_err(|error| format!("cannot create launcher Contents: {error}"))?;
    fs::create_dir(&macos)
        .map_err(|error| format!("cannot create launcher MacOS directory: {error}"))?;
    fs::create_dir(&resources)
        .map_err(|error| format!("cannot create launcher Resources directory: {error}"))?;
    write_new_file(&contents.join("Info.plist"), INFO_PLIST.as_bytes(), 0o644)?;
    write_new_file(&macos.join(EXECUTABLE_NAME), script, 0o755)?;
    write_new_file(&resources.join(OWNER_MARKER), &icon_marker(icon), 0o600)?;
    write_new_file(&resources.join(ICON_FILENAME), icon, 0o644)?;
    Ok(())
}

fn write_new_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("cannot create launcher file {}: {error}", path.display()))?;
    file.write_all(contents)
        .map_err(|error| format!("cannot write launcher file {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("cannot flush launcher file {}: {error}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("cannot set launcher file mode {}: {error}", path.display()))?;
    Ok(())
}

fn verify_owned_bundle(bundle: &Path) -> Result<(), String> {
    verify_bundle(bundle, INFO_PLIST.as_bytes(), true)
}

fn verify_version_2_bundle(bundle: &Path) -> Result<(), String> {
    verify_bundle(bundle, VERSION_2_INFO_PLIST.as_bytes(), true)
}

fn verify_legacy_owned_bundle(bundle: &Path) -> Result<(), String> {
    verify_bundle(bundle, LEGACY_INFO_PLIST.as_bytes(), false)
}

fn verify_replaceable_bundle(bundle: &Path) -> Result<(), String> {
    if verify_owned_bundle(bundle).is_ok()
        || verify_bundle(bundle, VERSION_3_INFO_PLIST.as_bytes(), true).is_ok()
        || verify_version_2_bundle(bundle).is_ok()
        || verify_legacy_owned_bundle(bundle).is_ok()
    {
        Ok(())
    } else {
        Err(format!(
            "launcher bundle is not an exact Incodex generation: {}",
            bundle.display()
        ))
    }
}

fn verify_bundle(bundle: &Path, expected_info: &[u8], require_icon: bool) -> Result<(), String> {
    require_directory(bundle, "launcher bundle")?;
    require_entries(bundle, &["Contents"])?;
    let contents = bundle.join("Contents");
    require_directory(&contents, "launcher Contents")?;
    require_entries(&contents, &["Info.plist", "MacOS", "Resources"])?;

    let info = contents.join("Info.plist");
    require_regular_file(&info, "launcher Info.plist")?;
    let info_bytes = fs::read(&info).map_err(|error| {
        format!(
            "cannot read launcher Info.plist {}: {error}",
            info.display()
        )
    })?;
    if info_bytes != expected_info {
        return Err(format!(
            "launcher Info.plist is not Incodex-owned: {}",
            info.display()
        ));
    }

    let macos = contents.join("MacOS");
    require_directory(&macos, "launcher executable directory")?;
    require_entries(&macos, &[EXECUTABLE_NAME])?;
    let executable = macos.join(EXECUTABLE_NAME);
    require_regular_file(&executable, "launcher executable")?;
    let script = fs::read_to_string(&executable).map_err(|error| {
        format!(
            "cannot read launcher executable {}: {error}",
            executable.display()
        )
    })?;
    if !is_generated_launcher_script(&script) {
        return Err(format!(
            "launcher executable is not Incodex-owned: {}",
            executable.display()
        ));
    }
    if fs::metadata(&executable)
        .map_err(|error| format!("cannot inspect launcher executable: {error}"))?
        .permissions()
        .mode()
        & 0o111
        == 0
    {
        return Err(format!(
            "launcher executable lost its execute bit: {}",
            executable.display()
        ));
    }

    let resources = contents.join("Resources");
    require_directory(&resources, "launcher Resources")?;
    if require_icon {
        require_entries(&resources, &[OWNER_MARKER, ICON_FILENAME])?;
    } else {
        require_entries(&resources, &[OWNER_MARKER])?;
    }
    let marker = resources.join(OWNER_MARKER);
    require_regular_file(&marker, "launcher ownership marker")?;
    let marker_bytes = fs::read(&marker)
        .map_err(|error| format!("cannot read launcher ownership marker: {error}"))?;
    if expected_info != INFO_PLIST.as_bytes() && marker_bytes != OWNER_MARKER_CONTENT {
        return Err(format!(
            "launcher ownership marker is invalid: {}",
            marker.display()
        ));
    }
    if require_icon {
        verify_launcher_icon(
            &resources.join(ICON_FILENAME),
            &marker_bytes,
            expected_info == INFO_PLIST.as_bytes(),
        )?;
    }
    Ok(())
}

fn icon_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn icon_marker(bytes: &[u8]) -> Vec<u8> {
    format!("incodex-launcher-owner-v4\nsha256:{}\n", icon_digest(bytes)).into_bytes()
}

fn verify_launcher_icon(path: &Path, marker: &[u8], dynamic: bool) -> Result<(), String> {
    require_regular_file(path, "launcher ICNS resource")?;
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read launcher ICNS resource: {error}"))?;
    let matches = if dynamic {
        marker == icon_marker(&bytes)
    } else {
        icon_digest(&bytes) == LEGACY_ICON_SHA256
    };
    if !is_icns(&bytes) || !matches {
        return Err(format!(
            "launcher ICNS resource does not match its recorded generation: {}",
            path.display()
        ));
    }
    Ok(())
}

fn is_icns(bytes: &[u8]) -> bool {
    let Some(header) = bytes.get(..8) else {
        return false;
    };
    &header[..4] == b"icns"
        && u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize == bytes.len()
}

fn require_directory(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {label} {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "{label} is not a real directory: {}",
            path.display()
        ));
    }
    Ok(())
}

fn require_regular_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {label} {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{label} is not a regular file: {}", path.display()));
    }
    Ok(())
}

fn require_entries(directory: &Path, expected: &[&str]) -> Result<(), String> {
    let mut actual = fs::read_dir(directory)
        .map_err(|error| {
            format!(
                "cannot list launcher directory {}: {error}",
                directory.display()
            )
        })?
        .map(|entry| {
            entry
                .map_err(|error| format!("cannot read launcher directory entry: {error}"))?
                .file_name()
                .into_string()
                .map_err(|_| {
                    format!(
                        "launcher directory has a non-UTF-8 entry: {}",
                        directory.display()
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut expected = expected
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    if actual != expected {
        return Err(format!(
            "launcher directory contents changed: {}",
            directory.display()
        ));
    }
    Ok(())
}

fn is_generated_launcher_script(script: &str) -> bool {
    let mut lines = script.lines();
    if lines.next() != Some(SHELL_HEADER.trim_end()) {
        return false;
    }
    let Some(mut command) = lines.next() else {
        return false;
    };
    if command == DETACHED_MARKER {
        let Some(detached) = lines.next() else {
            return false;
        };
        command = detached;
        return lines.next().is_none()
            && command.starts_with("/usr/bin/nohup '")
            && command.contains("' launch --app '")
            && command.ends_with("' </dev/null >/dev/null 2>&1 &");
    }
    lines.next().is_none()
        && command.starts_with("exec '")
        && command.contains("' launch --app '")
        && command.ends_with('\'')
}

fn restore_previous_bundle(
    destination: &Path,
    backup: &Path,
    staging: &Path,
    publish_error: io::Error,
) -> String {
    remove_created_path(staging);
    match fs::rename(backup, destination) {
        Ok(()) => format!(
            "cannot publish the Incodex Launcher; previous bundle restored: {publish_error}"
        ),
        Err(restore_error) => format!(
            "cannot publish the Incodex Launcher: {publish_error}; previous bundle remains at {}: {restore_error}",
            backup.display()
        ),
    }
}

fn remove_owned_bundle(bundle: &Path) {
    let _ = remove_exact_owned_bundle(bundle);
}

fn remove_exact_owned_bundle(bundle: &Path) -> Result<(), String> {
    verify_replaceable_bundle(bundle)?;
    let applications = bundle
        .parent()
        .ok_or_else(|| format!("launcher has no parent directory: {}", bundle.display()))?;
    let quarantine = unique_sibling(applications, "remove")?;
    fs::rename(bundle, &quarantine).map_err(|error| {
        format!(
            "cannot move owned launcher into private removal: {}: {error}",
            bundle.display()
        )
    })?;
    if let Err(error) = verify_replaceable_bundle(&quarantine) {
        return match fs::rename(&quarantine, bundle) {
            Ok(()) => Err(format!(
                "launcher changed during removal; original entry restored: {error}"
            )),
            Err(restore_error) => Err(format!(
                "launcher changed during removal and remains at {}: {error}; cannot restore original path: {restore_error}",
                quarantine.display()
            )),
        };
    }
    fs::remove_dir_all(&quarantine).map_err(|error| {
        format!(
            "cannot remove owned launcher {}: {error}",
            quarantine.display()
        )
    })
}

fn remove_created_path(path: &Path) {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            let _ = fs::remove_file(path);
        }
        Ok(_) => {
            let _ = fs::remove_dir_all(path);
        }
        Err(_) => {}
    }
}

#[cfg(test)]
#[path = "macos_launch_icon_tests.rs"]
mod tests;
