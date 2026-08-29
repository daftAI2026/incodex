use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

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
const HOMEBREW_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const HOMEBREW_UPDATE_TIMEOUT: Duration = Duration::from_secs(120);
const HOMEBREW_UPGRADE_TIMEOUT: Duration = Duration::from_secs(120);
const RUNTIME_UPDATE_TIMEOUT: Duration = Duration::from_secs(30);
const UPDATE_NOTICE_WORKER_ENV: &str = "INCODEX_INTERNAL_UPDATE_NOTICE_WORKER";
const RUNTIME_UPDATE_PENDING_NOTICE: &str = "Runtime synchronization incomplete, run inc update";
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

struct CapturedOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

enum CommandOutcome {
    Completed(CapturedOutput),
    TimedOut { stdout: Vec<u8>, stderr: Vec<u8> },
}

pub fn run_runtime(parsed: &ParsedCli) -> Result<(), String> {
    if parsed.dry_run {
        println!("would update ~/.incodex/runtime/ without modifying Codex");
        return Ok(());
    }
    let mut progress = Progress::new();
    progress.stage("Publishing Runtime");
    let published = incodex_runtime_bundle::publish(&user_root())?;
    if runtime_update_pending() {
        complete_update_notice();
    }
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
                "this copy is running from source\n  git pull && cargo install --locked --path crates/incodex-cli"
                    .into(),
            )
        }
        InstallChannel::Homebrew => return run_homebrew_update(parsed, &exe),
        InstallChannel::Script => {}
    }
    let prefix = install_prefix(&exe);
    println!("update channel: script");
    println!("  prefix: {}", prefix.display());
    if parsed.dry_run {
        println!("would re-run install.sh for this prefix");
        println!("would publish Runtime with the installed CLI");
        println!("no changes made.");
        return Ok(());
    }

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
    if runtime_update_pending() {
        repair_pending_runtime(&update_target)?;
    }

    let mut progress = Progress::new();
    progress.stage("Checking for updates");
    let latest = latest_stable_release()?;
    let current = parse_stable_version(env!("CARGO_PKG_VERSION"))
        .ok_or("update failed: current CLI version is not stable")?;
    match latest.version.cmp(&current) {
        std::cmp::Ordering::Less => {
            synchronize_runtime(&mut progress, &update_target, false)?;
            progress.stop();
            println!(
                "Current version {} is newer than latest release {}.",
                env!("CARGO_PKG_VERSION"),
                latest.tag
            );
            complete_update_notice();
            return Ok(());
        }
        std::cmp::Ordering::Equal => {
            synchronize_runtime(&mut progress, &update_target, false)?;
            progress.stop();
            println!("Already on latest version, {}", env!("CARGO_PKG_VERSION"));
            complete_update_notice();
            return Ok(());
        }
        std::cmp::Ordering::Greater => {}
    }

    progress.stage("Preparing update");
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
    progress.stage(&format!("Installing {}", latest.tag));
    let first_attempt = run_installer(&tagged_installer, &prefix, &download_base, expected)
        .and_then(|_| verify_installed_version(&prefix, expected));
    if let Err(first_error) = first_attempt {
        progress.stop();
        println!(
            "{}",
            format_warn(
                &format!("Stable installer did not complete: {first_error}"),
                None,
            )
        );
        progress.stage("Downloading compatibility installer");
        let compatibility = curl_download(MAIN_INSTALLER_URL, "compatibility installer")?;
        progress.stage(&format!("Repairing {}", latest.tag));
        run_installer(&compatibility, &prefix, &download_base, expected)?;
        verify_installed_version(&prefix, expected)?;
    }

    synchronize_runtime(&mut progress, &update_target, true)?;
    progress.stop();
    println!(
        "{}",
        format_ok(&format!("Verified Incodex {expected}"), None)
    );
    complete_update_notice();
    Ok(())
}

fn run_homebrew_update(parsed: &ParsedCli, current_cli: &Path) -> Result<(), String> {
    println!("update channel: homebrew");
    if parsed.dry_run {
        println!("would run brew update");
        println!("would run brew upgrade incodex");
        println!("would publish Runtime with the installed CLI");
        println!("no changes made.");
        return Ok(());
    }

    let lock_target = user_root().join("update-targets/homebrew-incodex");
    let _lock =
        incodex_transaction::acquire_target_lock(&user_root(), &lock_target, "update", None)
            .map_err(|err| {
                if err.contains("another incodex command") {
                    "update failed: another update is already running".to_string()
                } else {
                    format!("update failed: could not acquire update lock: {err}")
                }
            })?;
    if runtime_update_pending() {
        repair_pending_runtime(current_cli)?;
    }

    let mut progress = Progress::new();
    progress.stage("Updating Homebrew");
    let _ = run_brew(
        &["update"],
        timeout_from_env(
            "INCODEX_HOMEBREW_UPDATE_TIMEOUT_MS",
            HOMEBREW_UPDATE_TIMEOUT,
        ),
    );

    progress.stage("Upgrading Incodex");
    let upgrade = run_brew(
        &["upgrade", "incodex"],
        timeout_from_env(
            "INCODEX_HOMEBREW_UPGRADE_TIMEOUT_MS",
            HOMEBREW_UPGRADE_TIMEOUT,
        ),
    );
    progress.stop();

    let output = match upgrade {
        Ok(CommandOutcome::Completed(output)) if output.status.success() => output,
        Ok(CommandOutcome::Completed(output)) => {
            let detail = output_detail(&output);
            return Err(if detail.is_empty() {
                format!(
                    "Homebrew upgrade failed: brew upgrade incodex exited with {}",
                    output.status
                )
            } else {
                format!("Homebrew upgrade failed\n{detail}")
            });
        }
        Ok(CommandOutcome::TimedOut { stdout, stderr }) => {
            let detail = output_detail_bytes(&stderr, &stdout);
            return Err(if detail.is_empty() {
                "Homebrew upgrade timed out".into()
            } else {
                format!("Homebrew upgrade timed out\n{detail}")
            });
        }
        Err(err) => return Err(format!("Homebrew upgrade failed: {err}")),
    };

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let already_installed = combined.contains("already installed");
    let cli_updated = !already_installed;
    let installed = homebrew_installed_version().ok_or_else(|| {
        runtime_sync_failure(
            "could not verify the installed Homebrew CLI version".to_string(),
            cli_updated,
        )
    })?;
    let installed_cli =
        homebrew_installed_cli().map_err(|detail| runtime_sync_failure(detail, cli_updated))?;
    verify_cli_version(&installed_cli, &installed)
        .map_err(|detail| runtime_sync_failure(detail, cli_updated))?;
    synchronize_runtime(&mut progress, &installed_cli, cli_updated)?;
    progress.stop();
    println!(
        "{}",
        if already_installed {
            format!("Already on latest version, {installed}")
        } else {
            format!("Updated to latest version, {installed}")
        }
    );
    complete_update_notice();
    Ok(())
}

fn repair_pending_runtime(installed_cli: &Path) -> Result<(), String> {
    let mut progress = Progress::new();
    synchronize_runtime(&mut progress, installed_cli, false)?;
    progress.stop();
    complete_update_notice();
    println!("{}", format_ok("Runtime synchronization repaired", None));
    Ok(())
}

fn synchronize_runtime(
    progress: &mut Progress,
    installed_cli: &Path,
    cli_updated: bool,
) -> Result<(), String> {
    progress.stage("Publishing Runtime");
    if let Err(detail) = run_runtime_command(installed_cli) {
        progress.stop();
        return Err(runtime_sync_failure(detail, cli_updated));
    }
    Ok(())
}

fn runtime_sync_failure(detail: String, cli_updated: bool) -> String {
    mark_runtime_update_pending();
    if cli_updated {
        format!("CLI was updated, but Runtime synchronization failed: {detail}")
    } else {
        format!("Runtime synchronization failed: {detail}")
    }
}

fn run_runtime_command(installed_cli: &Path) -> Result<(), String> {
    let mut command = Command::new(installed_cli);
    command.arg("runtime").env_remove(UPDATE_NOTICE_WORKER_ENV);
    match run_command_with_timeout(
        &mut command,
        timeout_from_env("INCODEX_RUNTIME_UPDATE_TIMEOUT_MS", RUNTIME_UPDATE_TIMEOUT),
    ) {
        Ok(CommandOutcome::Completed(output)) if output.status.success() => Ok(()),
        Ok(CommandOutcome::Completed(output)) => {
            let detail = output_detail(&output);
            if detail.is_empty() {
                Err(format!(
                    "{} runtime exited with {}",
                    installed_cli.display(),
                    output.status
                ))
            } else {
                Err(detail)
            }
        }
        Ok(CommandOutcome::TimedOut { stdout, stderr }) => {
            let detail = output_detail_bytes(&stderr, &stdout);
            if detail.is_empty() {
                Err(format!("{} runtime timed out", installed_cli.display()))
            } else {
                Err(format!("Runtime timed out\n{detail}"))
            }
        }
        Err(error) => Err(format!(
            "could not run {} runtime: {error}",
            installed_cli.display()
        )),
    }
}

fn homebrew_installed_cli() -> Result<PathBuf, String> {
    let output = match run_brew(
        &["--prefix", "incodex"],
        timeout_from_env("INCODEX_HOMEBREW_QUERY_TIMEOUT_MS", HOMEBREW_QUERY_TIMEOUT),
    ) {
        Ok(CommandOutcome::Completed(output)) if output.status.success() => output,
        Ok(CommandOutcome::Completed(output)) => {
            let detail = output_detail(&output);
            return Err(if detail.is_empty() {
                format!("Homebrew prefix lookup failed: {}", output.status)
            } else {
                format!("Homebrew prefix lookup failed\n{detail}")
            });
        }
        Ok(CommandOutcome::TimedOut { stdout, stderr }) => {
            let detail = output_detail_bytes(&stderr, &stdout);
            return Err(if detail.is_empty() {
                "Homebrew prefix lookup timed out".to_string()
            } else {
                format!("Homebrew prefix lookup timed out\n{detail}")
            });
        }
        Err(error) => return Err(format!("Homebrew prefix lookup failed: {error}")),
    };
    let prefix = String::from_utf8_lossy(&output.stdout);
    let prefix = prefix.trim();
    let prefix = PathBuf::from(prefix);
    if prefix.as_os_str().is_empty() || !prefix.is_absolute() {
        return Err("Homebrew prefix lookup returned an invalid path".to_string());
    }
    let installed_cli = prefix.join("bin/incodex");
    if !installed_cli.is_file() {
        return Err(format!(
            "Homebrew prefix has no installed Incodex CLI: {}",
            installed_cli.display()
        ));
    }
    Ok(installed_cli)
}

fn run_brew(args: &[&str], timeout: Duration) -> Result<CommandOutcome, String> {
    let mut command = Command::new("brew");
    command
        .args(args)
        .env("HOMEBREW_NO_ENV_HINTS", "1")
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("NONINTERACTIVE", "1");
    run_command_with_timeout(&mut command, timeout)
}

fn run_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<CommandOutcome, String> {
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|err| {
        format!(
            "could not start {}: {err}",
            command.get_program().to_string_lossy()
        )
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "could not capture command stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "could not capture command stderr".to_string())?;
    let stdout_reader = thread::spawn(move || read_stream(stdout));
    let stderr_reader = thread::spawn(move || read_stream(stderr));
    let deadline = Instant::now() + timeout;
    let mut exit_status = None;

    loop {
        if exit_status.is_none() {
            match child.try_wait() {
                Ok(status) => exit_status = status,
                Err(err) => {
                    terminate_process_group(&mut child);
                    let _ = join_reader(stdout_reader);
                    let _ = join_reader(stderr_reader);
                    return Err(format!("could not wait for command: {err}"));
                }
            }
        }
        if stdout_reader.is_finished() && stderr_reader.is_finished() {
            if let Some(status) = exit_status.take() {
                return Ok(CommandOutcome::Completed(CapturedOutput {
                    status,
                    stdout: join_reader(stdout_reader),
                    stderr: join_reader(stderr_reader),
                }));
            }
        }
        if Instant::now() >= deadline {
            terminate_process_group(&mut child);
            return Ok(CommandOutcome::TimedOut {
                stdout: join_reader(stdout_reader),
                stderr: join_reader(stderr_reader),
            });
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn terminate_process_group(child: &mut Child) {
    let process_group = -(child.id() as i32);
    unsafe {
        libc::kill(process_group, libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn read_stream(mut stream: impl Read) -> Vec<u8> {
    let mut output = Vec::new();
    let _ = stream.read_to_end(&mut output);
    output
}

fn join_reader(reader: thread::JoinHandle<Vec<u8>>) -> Vec<u8> {
    reader.join().unwrap_or_default()
}

fn timeout_from_env(name: &str, default: Duration) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|milliseconds| *milliseconds > 0)
        .map(Duration::from_millis)
        .unwrap_or(default)
}

fn output_detail(output: &CapturedOutput) -> String {
    output_detail_bytes(&output.stderr, &output.stdout)
}

fn output_detail_bytes(stderr: &[u8], stdout: &[u8]) -> String {
    [stderr, stdout]
        .into_iter()
        .map(|bytes| String::from_utf8_lossy(bytes))
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn homebrew_installed_version() -> Option<String> {
    let output = match run_brew(
        &["list", "--versions", "incodex"],
        timeout_from_env("INCODEX_HOMEBREW_QUERY_TIMEOUT_MS", HOMEBREW_QUERY_TIMEOUT),
    ) {
        Ok(CommandOutcome::Completed(output)) if output.status.success() => output,
        _ => return public_cli_version(),
    };
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .nth(1)
        .map(str::to_string)
        .or_else(public_cli_version)
}

fn public_cli_version() -> Option<String> {
    let mut command = Command::new("inc");
    command.arg("--version");
    let output = match run_command_with_timeout(
        &mut command,
        timeout_from_env("INCODEX_HOMEBREW_QUERY_TIMEOUT_MS", HOMEBREW_QUERY_TIMEOUT),
    ) {
        Ok(CommandOutcome::Completed(output)) if output.status.success() => output,
        _ => return None,
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Incodex version "))
        .and_then(|version| version.split_whitespace().next())
        .map(str::to_string)
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
        .env("INCODEX_PREFIX", prefix)
        .env("INCODEX_DOWNLOAD_BASE", download_base)
        .env("INCODEX_EXPECTED_VERSION", expected_version)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
    let output = bash
        .wait_with_output()
        .map_err(|err| format!("update failed: could not wait for bash: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = [stderr.as_ref(), stdout.as_ref()]
        .into_iter()
        .find_map(|text| {
            let lines = text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            (!lines.is_empty()).then(|| lines.join(" | "))
        });
    Err(match detail {
        Some(detail) => format!(
            "update failed: installer exited with {}: {detail}",
            output.status
        ),
        None => format!("update failed: installer exited with {}", output.status),
    })
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
    verify_cli_version(&installed, expected)
}

fn verify_cli_version(installed: &Path, expected: &str) -> Result<(), String> {
    if cli_reported_version(installed).as_deref() != Ok(expected) {
        return Err(format!(
            "update failed: installed CLI did not report {expected}"
        ));
    }
    Ok(())
}

fn cli_reported_version(installed: &Path) -> Result<String, String> {
    let mut command = Command::new(installed);
    command
        .arg("--version")
        .env_remove(UPDATE_NOTICE_WORKER_ENV);
    let output = match run_command_with_timeout(
        &mut command,
        timeout_from_env("INCODEX_CLI_QUERY_TIMEOUT_MS", HOMEBREW_QUERY_TIMEOUT),
    )? {
        CommandOutcome::Completed(output) if output.status.success() => output,
        _ => return Err("installed CLI version probe failed".to_string()),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("Incodex version ")
                .and_then(|rest| rest.split_whitespace().next())
        })
        .map(str::to_string)
        .ok_or_else(|| "installed CLI version probe failed".to_string())
}

fn clear_update_notice() {
    write_update_notice("");
}

fn complete_update_notice() {
    clear_update_notice();
    clear_runtime_update_pending();
}

fn runtime_update_pending_path() -> PathBuf {
    user_root().join("cache/runtime_update_pending")
}

fn runtime_update_pending() -> bool {
    let path = runtime_update_pending_path();
    fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn mark_runtime_update_pending() {
    write_cache_file(&runtime_update_pending_path(), "pending\n");
    write_update_notice(&format!("{RUNTIME_UPDATE_PENDING_NOTICE}\n"));
}

fn clear_runtime_update_pending() {
    let _ = fs::remove_file(runtime_update_pending_path());
}

pub(crate) fn read_update_notice() -> Option<String> {
    if runtime_update_pending() {
        return Some(RUNTIME_UPDATE_PENDING_NOTICE.to_string());
    }
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
        install_channel(&exe),
        InstallChannel::Script | InstallChannel::Homebrew
    ) && action == "inc update"
}

pub(crate) fn spawn_update_notice_refresh() {
    let Ok(exe) = current_exe() else {
        return;
    };
    if install_channel(&exe) == InstallChannel::Source {
        return;
    }
    let child = Command::new(exe)
        .env(UPDATE_NOTICE_WORKER_ENV, "1")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return;
    };
    thread::spawn(move || {
        let _ = child.wait();
    });
}

pub(crate) fn run_update_notice_worker() -> bool {
    if std::env::var(UPDATE_NOTICE_WORKER_ENV).as_deref() != Ok("1") {
        return false;
    }
    refresh_update_notice();
    true
}

fn refresh_update_notice() {
    if runtime_update_pending() {
        return;
    }
    let Ok(exe) = current_exe() else {
        return;
    };
    let channel = install_channel(&exe);
    if channel == InstallChannel::Source {
        return;
    }
    let Ok(latest) = latest_stable_release() else {
        write_refreshed_update_notice("");
        return;
    };
    let Some(process_version) = parse_stable_version(env!("CARGO_PKG_VERSION")) else {
        write_refreshed_update_notice("");
        return;
    };
    let message = if latest.version <= process_version {
        String::new()
    } else {
        let Some(current) = active_installed_version(channel, &exe) else {
            write_refreshed_update_notice("");
            return;
        };
        if latest.version <= current {
            write_refreshed_update_notice("");
            return;
        }
        match channel {
            InstallChannel::Script => format!(
                "Update {} available, run inc update\n",
                latest.tag.trim_start_matches('v')
            ),
            InstallChannel::Homebrew => homebrew_update_notice(current),
            InstallChannel::Source => String::new(),
        }
    };
    write_refreshed_update_notice(&message);
}

fn active_installed_version(channel: InstallChannel, current_exe: &Path) -> Option<[u64; 3]> {
    let installed_cli = match channel {
        InstallChannel::Script => current_exe.to_path_buf(),
        InstallChannel::Homebrew => homebrew_installed_cli().ok()?,
        InstallChannel::Source => return None,
    };
    cli_reported_version(&installed_cli)
        .ok()
        .and_then(|version| parse_stable_version(&version))
}

fn write_refreshed_update_notice(message: &str) {
    if !runtime_update_pending() {
        write_update_notice(message);
    }
}

fn homebrew_update_notice(current: [u64; 3]) -> String {
    let Some((version_text, formula)) = homebrew_stable_version() else {
        return String::new();
    };
    if formula > current {
        format!("Update {version_text} available, run inc update\n")
    } else {
        String::new()
    }
}

fn homebrew_stable_version() -> Option<(String, [u64; 3])> {
    let timeout = timeout_from_env("INCODEX_HOMEBREW_QUERY_TIMEOUT_MS", HOMEBREW_QUERY_TIMEOUT);
    if let Ok(CommandOutcome::Completed(output)) =
        run_brew(&["outdated", "--formula", "--verbose", "incodex"], timeout)
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(candidate) = text
                .lines()
                .find_map(|line| line.split_once("< ").map(|(_, value)| value))
                .and_then(|value| value.split_whitespace().next())
            {
                if let Some(version) = parse_stable_version(candidate) {
                    return Some((candidate.to_string(), version));
                }
            }
        }
    }

    let output = match run_brew(&["info", "--json=v2", "incodex"], timeout) {
        Ok(CommandOutcome::Completed(output)) if output.status.success() => output,
        _ => return None,
    };
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
    write_cache_file(&cache, message);
}

fn write_cache_file(cache: &Path, message: &str) {
    let Some(parent) = cache.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let sequence = UPDATE_NOTICE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = cache
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("cache");
    let temporary = parent.join(format!(".{name}.{}.{sequence}.tmp", std::process::id(),));
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
            return Err("this copy is running from source\n  cargo uninstall incodex-cli".into())
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
    crate::confirm::require("self-uninstall", parsed.yes)?;
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
        || text.contains("/usr/local/opt/incodex/")
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
