use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use incodex_core::paths::{user_root, DEFAULT_APP};
use incodex_core::{format_kv, format_ok, format_step, format_warn};
use serde::Deserialize;

use crate::parse::ParsedCli;
use crate::spinner::Progress;

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/daftAI2026/incodex/releases/latest";
const MAIN_INSTALLER_URL: &str =
    "https://raw.githubusercontent.com/daftAI2026/incodex/main/install.sh";
const DOWNLOAD_ATTEMPTS: usize = 3;
const RETRY_DELAY: Duration = Duration::from_millis(200);
static UPDATE_NOTICE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StableRelease {
    tag: String,
    version: [u64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallChannel {
    Source,
    Script,
    Homebrew,
}

pub fn run_runtime(parsed: &ParsedCli) -> Result<(), String> {
    if parsed.dry_run {
        println!("would update ~/.incodex/runtime/ without modifying Codex");
        return Ok(());
    }
    let mut progress = Progress::new();
    progress.stage("Publishing Runtime");
    let published = incodex_runtime_bundle::publish(&user_root())?;
    progress.stop();
    println!("{}", format_step("Runtime", None));
    println!(
        "{}",
        format_ok(
            "Runtime updated. Codex was not modified. Reopen it to load the new logic.",
            None,
        )
    );
    println!("{}", format_kv("Runtime", &published.version, None));
    Ok(())
}

pub fn run_update(parsed: &ParsedCli) -> Result<(), String> {
    let exe = current_exe()?;
    match install_channel(&exe) {
        InstallChannel::Source => {
            return Err(
                "this copy is running from source\n  git pull && bun install --frozen-lockfile && bun link"
                    .into(),
            )
        }
        InstallChannel::Homebrew => {
            return Err("this copy was installed with Homebrew\n  brew upgrade incodex".into())
        }
        InstallChannel::Script => {}
    }
    let prefix = install_prefix(&exe);
    println!("update channel: script");
    println!("  prefix: {}", prefix.display());
    if parsed.dry_run {
        println!("would re-run install.sh for this prefix");
        println!("no changes made.");
        return Ok(());
    }

    let mut progress = Progress::new();
    progress.stage("Checking for updates");
    let latest = latest_stable_release()?;
    let current = parse_stable_version(env!("CARGO_PKG_VERSION"))
        .ok_or("update failed: current CLI version is not stable")?;
    match latest.version.cmp(&current) {
        std::cmp::Ordering::Less => {
            progress.stop();
            println!(
                "Current version {} is newer than latest release {}.",
                env!("CARGO_PKG_VERSION"),
                latest.tag
            );
            return Ok(());
        }
        std::cmp::Ordering::Equal => {
            progress.stop();
            println!("Already on latest version, {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        std::cmp::Ordering::Greater => {}
    }

    progress.stage("Preparing update");
    let update_target = prefix.join("bin/incodex");
    let _lock =
        incodex_transaction::acquire_target_lock(&user_root(), &update_target, "update", None)
            .map_err(|err| {
                if err.contains("another incodex command") {
                    "update failed: another update is already running".to_string()
                } else {
                    format!("update failed: could not acquire update lock: {err}")
                }
            })?;
    progress.stop();
    println!("updating {} -> {}", env!("CARGO_PKG_VERSION"), latest.tag);
    let install_script_url = format!(
        "https://raw.githubusercontent.com/daftAI2026/incodex/{}/install.sh",
        latest.tag
    );
    let download_base = format!(
        "https://github.com/daftAI2026/incodex/releases/download/{}",
        latest.tag
    );
    let expected = latest.tag.trim_start_matches('v');

    progress.stage("Downloading stable installer");
    let tagged_installer = curl_download(&install_script_url, "stable installer")?;
    progress.stop();
    println!(
        "{}",
        format_step(&format!("Installing {}", latest.tag), None)
    );
    let first_attempt = run_installer(&tagged_installer, &prefix, &download_base, expected)
        .and_then(|_| verify_installed_version(&prefix, expected));
    if let Err(first_error) = first_attempt {
        println!(
            "{}",
            format_warn(
                &format!("Stable installer did not complete: {first_error}"),
                None,
            )
        );
        progress.stage("Downloading compatibility installer");
        let compatibility = curl_download(MAIN_INSTALLER_URL, "compatibility installer")?;
        progress.stop();
        println!(
            "{}",
            format_step(&format!("Repairing {}", latest.tag), None)
        );
        run_installer(&compatibility, &prefix, &download_base, expected)?;
        verify_installed_version(&prefix, expected)?;
    }

    progress.stop();
    println!(
        "{}",
        format_ok(&format!("Verified Incodex {expected}"), None)
    );
    clear_update_notice();
    Ok(())
}

fn run_installer(
    installer: &[u8],
    prefix: &Path,
    download_base: &str,
    expected_version: &str,
) -> Result<(), String> {
    let mut bash = Command::new("bash")
        .env_remove("INCODEX_ARCH")
        .env_remove("INCODEX_DOWNLOAD_DIR")
        .env("INCODEX_PREFIX", &prefix)
        .env("INCODEX_DOWNLOAD_BASE", download_base)
        .env("INCODEX_EXPECTED_VERSION", expected_version)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| format!("update failed: could not start bash: {err}"))?;
    let write_result = bash
        .stdin
        .take()
        .ok_or_else(|| "update failed: bash stdin unavailable".to_string())
        .and_then(|mut stdin| {
            stdin
                .write_all(installer)
                .map_err(|err| format!("update failed: could not send install script: {err}"))
        });
    if let Err(err) = write_result {
        let _ = bash.kill();
        let _ = bash.wait();
        return Err(err);
    }
    let status = bash
        .wait()
        .map_err(|err| format!("update failed: could not wait for bash: {err}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("update failed: installer exited with {status}"))
}

fn curl_download(url: &str, label: &str) -> Result<Vec<u8>, String> {
    let mut last_failure = None;
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        let downloaded = Command::new("curl")
            .args([
                "-fsSL",
                "--connect-timeout",
                "10",
                "--max-time",
                "60",
                "--write-out",
                "INCODEX_HTTP_STATUS:%{http_code}",
                url,
            ])
            .output()
            .map_err(|err| format!("update failed: could not start curl: {err}"))?;
        let (body, http_status) = split_curl_body_and_status(downloaded.stdout);
        if downloaded.status.success() {
            return Ok(body);
        }
        let detail = String::from_utf8_lossy(&downloaded.stderr);
        let detail = detail.trim();
        last_failure = Some(if detail.is_empty() {
            format!(
                "update failed: could not download {label}: curl exited with {}",
                downloaded.status
            )
        } else {
            format!("update failed: {detail}")
        });
        let code = downloaded.status.code();
        let transient = code.is_some_and(transient_curl_exit)
            || (code == Some(22) && http_status.is_some_and(transient_http_status));
        if attempt == DOWNLOAD_ATTEMPTS || !transient {
            break;
        }
        thread::sleep(RETRY_DELAY);
    }
    Err(last_failure.unwrap_or_else(|| format!("update failed: could not download {label}")))
}

fn transient_curl_exit(code: i32) -> bool {
    matches!(code, 6 | 7 | 18 | 28 | 35 | 52 | 55 | 56)
}

fn transient_http_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

fn split_curl_body_and_status(mut output: Vec<u8>) -> (Vec<u8>, Option<u16>) {
    let marker = b"INCODEX_HTTP_STATUS:";
    let Some(separator) = output
        .windows(marker.len())
        .rposition(|window| window == marker)
    else {
        return (output, None);
    };
    let status = &output[separator + marker.len()..];
    if status.len() != 3 || !status.iter().all(u8::is_ascii_digit) {
        return (output, None);
    }
    let status = std::str::from_utf8(status)
        .ok()
        .and_then(|value| value.parse().ok());
    output.truncate(separator);
    (output, status)
}

fn latest_stable_release() -> Result<StableRelease, String> {
    let body = curl_download(LATEST_RELEASE_URL, "latest release metadata")?;
    let release: LatestRelease = serde_json::from_slice(&body)
        .map_err(|_| "update failed: invalid latest release metadata".to_string())?;
    let raw = release.tag_name.strip_prefix('v').ok_or_else(|| {
        format!(
            "update failed: invalid latest release tag: {}",
            release.tag_name
        )
    })?;
    let version = parse_stable_version(raw).ok_or_else(|| {
        format!(
            "update failed: invalid latest release tag: {}",
            release.tag_name
        )
    })?;
    let canonical = format!("v{}.{}.{}", version[0], version[1], version[2]);
    if release.tag_name != canonical {
        return Err(format!(
            "update failed: invalid latest release tag: {}",
            release.tag_name
        ));
    }
    Ok(StableRelease {
        tag: release.tag_name,
        version,
    })
}

fn parse_stable_version(raw: &str) -> Option<[u64; 3]> {
    let mut values = [0_u64; 3];
    let mut parts = raw.split('.');
    for value in &mut values {
        let part = parts.next()?;
        if part.is_empty()
            || (part.len() > 1 && part.starts_with('0'))
            || !part.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        *value = part.parse().ok()?;
    }
    parts.next().is_none().then_some(values)
}

fn verify_installed_version(prefix: &Path, expected: &str) -> Result<(), String> {
    let installed = prefix.join("bin/incodex");
    let output = Command::new(&installed)
        .arg("--version")
        .output()
        .map_err(|err| {
            format!(
                "update failed: could not run {}: {err}",
                installed.display()
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "update failed: installed CLI did not report {expected}"
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let reported = stdout.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Incodex version ")
            .and_then(|rest| rest.split_whitespace().next())
    });
    if reported != Some(expected) {
        return Err(format!(
            "update failed: installed CLI did not report {expected}"
        ));
    }
    Ok(())
}

fn clear_update_notice() {
    write_update_notice("");
}

pub(crate) fn read_update_notice() -> Option<String> {
    let cache = user_root().join("cache/update_message");
    let message = fs::read_to_string(&cache).ok()?;
    let message = message.trim();
    if valid_update_notice(message) {
        return Some(message.to_string());
    }
    write_update_notice("");
    None
}

fn valid_update_notice(message: &str) -> bool {
    let Some(rest) = message.strip_prefix("Update ") else {
        return false;
    };
    let Some((version_text, action)) = rest.split_once(" available, run ") else {
        return false;
    };
    let Some(version) = parse_stable_version(version_text) else {
        return false;
    };
    let Some(current) = parse_stable_version(env!("CARGO_PKG_VERSION")) else {
        return false;
    };
    if version <= current {
        return false;
    }
    let Ok(exe) = current_exe() else {
        return false;
    };
    matches!(
        (install_channel(&exe), action),
        (InstallChannel::Script, "incodex update")
            | (InstallChannel::Homebrew, "brew upgrade incodex")
    )
}

pub(crate) fn spawn_update_notice_refresh() {
    let Ok(exe) = current_exe() else {
        return;
    };
    let channel = install_channel(&exe);
    if channel == InstallChannel::Source {
        return;
    }
    thread::spawn(move || {
        let Ok(latest) = latest_stable_release() else {
            return;
        };
        let Some(current) = parse_stable_version(env!("CARGO_PKG_VERSION")) else {
            return;
        };
        let message = match channel {
            InstallChannel::Script if latest.version > current => format!(
                "Update {} available, run incodex update\n",
                latest.tag.trim_start_matches('v')
            ),
            InstallChannel::Homebrew => homebrew_update_notice(current, latest.version),
            _ => String::new(),
        };
        write_update_notice(&message);
    });
}

fn homebrew_update_notice(current: [u64; 3], release: [u64; 3]) -> String {
    let Some((version_text, formula)) = homebrew_stable_version() else {
        return String::new();
    };
    if formula > current && formula <= release {
        format!("Update {version_text} available, run brew upgrade incodex\n")
    } else {
        String::new()
    }
}

fn homebrew_stable_version() -> Option<(String, [u64; 3])> {
    let output = Command::new("brew")
        .args(["info", "--json=v2", "incodex"])
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_ENV_HINTS", "1")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let text = body
        .get("formulae")?
        .as_array()?
        .first()?
        .get("versions")?
        .get("stable")?
        .as_str()?;
    let version = parse_stable_version(text)?;
    Some((text.to_string(), version))
}

fn write_update_notice(message: &str) {
    let cache = user_root().join("cache/update_message");
    let Some(parent) = cache.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let sequence = UPDATE_NOTICE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".update_message.{}.{sequence}.tmp",
        std::process::id()
    ));
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut file| file.write_all(message.as_bytes()));
    if written.is_ok() {
        let _ = fs::rename(&temporary, cache);
    }
    let _ = fs::remove_file(temporary);
}

pub fn run_self_uninstall(parsed: &ParsedCli) -> Result<(), String> {
    let exe = current_exe()?;
    match install_channel(&exe) {
        InstallChannel::Source => {
            return Err("this copy is running from source\n  bun unlink".into())
        }
        InstallChannel::Homebrew => {
            return Err("this copy was installed with Homebrew\n  brew uninstall incodex".into())
        }
        InstallChannel::Script => {}
    }
    let paths = self_uninstall_paths(&exe);
    println!("remove:");
    for path in &paths {
        println!("  {}", path.display());
    }
    if parsed.restore_app {
        println!("also restore: {DEFAULT_APP}");
    }
    if parsed.dry_run {
        println!("no changes made.");
        return Ok(());
    }
    ensure_confirmed(parsed, "self-uninstall")?;
    let mut progress = Progress::new();
    if parsed.restore_app {
        crate::install::restore_default_for_self_uninstall(&mut progress)?;
        progress.stop();
        println!("restored {DEFAULT_APP}");
    }
    progress.stage("Removing Incodex CLI");
    for path in paths {
        if path.exists() {
            std::fs::remove_file(path).map_err(|err| err.to_string())?;
        }
    }
    progress.stop();
    println!("done");
    Ok(())
}

fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|err| err.to_string())
}

fn install_channel(exe: &Path) -> InstallChannel {
    let text = exe.to_string_lossy();
    if text.contains("/Cellar/incodex/")
        || text.contains("/opt/homebrew/opt/incodex/")
        || text.ends_with("/opt/homebrew/bin/incodex")
        || text.ends_with("/opt/homebrew/bin/inc")
    {
        return InstallChannel::Homebrew;
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if exe.starts_with(repo) || text.contains("/target/debug/") || text.contains("/target/release/")
    {
        return InstallChannel::Source;
    }
    InstallChannel::Script
}

fn install_prefix(exe: &Path) -> PathBuf {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    if dir.file_name().is_some_and(|name| name == "bin") {
        dir.parent().unwrap_or(dir).to_path_buf()
    } else {
        dir.to_path_buf()
    }
}

fn self_uninstall_paths(exe: &Path) -> [PathBuf; 2] {
    let dir = exe.parent().unwrap_or_else(|| Path::new("."));
    [dir.join("incodex"), dir.join("inc")]
}

fn ensure_confirmed(parsed: &ParsedCli, command: &str) -> Result<(), String> {
    if parsed.yes {
        return Ok(());
    }
    if crate::terminal::is_tty() {
        return if crate::confirm::ask_to_continue()? {
            Ok(())
        } else {
            Err("aborted".into())
        };
    }
    Err(format!(
        "non-interactive {command} requires --yes\n  incodex {command} --yes"
    ))
}
