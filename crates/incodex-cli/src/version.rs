/*
 * [INPUT]: 依赖系统只读 API、SIP 查询、进程环境和当前 CLI 路径。
 * [OUTPUT]: 提供版本事实快照与固定标签的版本报告。
 * [POS]: CLI 诊断出口；macOS 原生读取机器事实，仅 SIP 保留子命令。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::process::Command;

#[cfg(target_os = "windows")]
use crate::windows_system::system_binary_path;

#[cfg(not(target_os = "windows"))]
pub struct VersionFacts {
    pub version: String,
    pub macos: String,
    pub architecture: String,
    pub kernel: String,
    pub sip: String,
    pub disk_free: String,
    pub install: String,
    pub shell: String,
}

#[cfg(target_os = "windows")]
pub struct VersionFacts {
    pub version: String,
    pub windows: String,
    pub architecture: String,
    pub install: String,
    pub shell: String,
}

pub fn format_version_report(facts: &VersionFacts) -> String {
    let mut lines = vec![format!("Incodex version {}", facts.version)];
    #[cfg(not(target_os = "windows"))]
    lines.extend([
        format!("macOS: {}", facts.macos),
        format!("Architecture: {}", facts.architecture),
        format!("Kernel: {}", facts.kernel),
        format!("SIP: {}", facts.sip),
        format!("Disk Free: {}", facts.disk_free),
    ]);
    #[cfg(target_os = "windows")]
    lines.extend([
        format!("Windows: {}", facts.windows),
        format!("Architecture: {}", facts.architecture),
    ]);
    lines.extend([
        format!("Install: {}", facts.install),
        format!("Shell: {}", facts.shell),
    ]);
    format!("{}\n\n", lines.join("\n"))
}

#[cfg(not(target_os = "windows"))]
pub fn collect_version_facts() -> VersionFacts {
    let (architecture, kernel) = machine_facts();
    VersionFacts {
        version: env!("CARGO_PKG_VERSION").to_string(),
        macos: macos_version(),
        architecture,
        kernel,
        sip: sip_status(),
        disk_free: disk_free(),
        install: install_channel(),
        shell: std::env::var("SHELL").unwrap_or_else(|_| "Unknown".to_string()),
    }
}

#[cfg(target_os = "macos")]
fn macos_version() -> String {
    use std::ffi::CStr;

    let mut buffer = [0u8; 256];
    let mut length = buffer.len();
    // 只读 sysctl；失败或缓冲区不足时保持 Unknown，不再启动回退进程。
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 || length > buffer.len() {
        return "Unknown".to_string();
    }
    CStr::from_bytes_with_nul(&buffer[..length])
        .ok()
        .and_then(|text| text.to_str().ok())
        .filter(|text| !text.is_empty())
        .unwrap_or("Unknown")
        .to_string()
}

#[cfg(target_os = "macos")]
fn machine_facts() -> (String, String) {
    // 全零初始化保证 C 字符数组终止；仅在 uname 成功后读取结果。
    let mut info: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut info) } != 0 {
        return ("Unknown".to_string(), "Unknown".to_string());
    }
    unsafe {
        (
            std::ffi::CStr::from_ptr(info.machine.as_ptr())
                .to_string_lossy()
                .into_owned(),
            std::ffi::CStr::from_ptr(info.release.as_ptr())
                .to_string_lossy()
                .into_owned(),
        )
    }
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn macos_version() -> String {
    probe("sw_vers", &["-productVersion"])
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn machine_facts() -> (String, String) {
    (probe("uname", &["-m"]), probe("uname", &["-r"]))
}

#[cfg(target_os = "windows")]
pub fn collect_version_facts() -> VersionFacts {
    let version = system_binary_path("cmd.exe")
        .map(|cmd| probe(cmd, &["/C", "ver"]))
        .unwrap_or_else(|_| "Unknown".to_string());
    VersionFacts {
        version: env!("CARGO_PKG_VERSION").to_string(),
        windows: version,
        architecture: std::env::consts::ARCH.to_string(),
        install: install_channel(),
        shell: std::env::var("COMSPEC").unwrap_or_else(|_| "Unknown".to_string()),
    }
}

fn probe(cmd: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> String {
    let output = Command::new(cmd).args(args).output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => "Unknown".to_string(),
    }
}

#[cfg(not(target_os = "windows"))]
fn sip_status() -> String {
    let raw = probe("csrutil", &["status"]).to_lowercase();
    if raw.contains("enabled") {
        "Enabled".to_string()
    } else if raw.contains("disabled") {
        "Disabled".to_string()
    } else {
        "Unknown".to_string()
    }
}

#[cfg(target_os = "macos")]
fn disk_free() -> String {
    let mut info = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // 与 df -k / 使用同一 statfs 来源；成功才读取完整输出结构。
    if unsafe { libc::statfs(c"/".as_ptr(), info.as_mut_ptr()) } != 0 {
        return "Unknown".to_string();
    }
    let info = unsafe { info.assume_init() };
    let Some(available_bytes) = info.f_bavail.checked_mul(u64::from(info.f_bsize)) else {
        return "Unknown".to_string();
    };
    if info.f_bsize == 0 {
        return "Unknown".to_string();
    }
    // 先换算整 KiB，再沿用现有 GB 格式，保留 df -k 的截断顺序。
    let available_kb = available_bytes / 1024;
    format!("{:.2}GB", available_kb as f64 / 1024.0 / 1024.0)
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn disk_free() -> String {
    let raw = probe("df", &["-k", "/"]);
    let data = match raw.lines().nth(1) {
        Some(line) => line,
        None => return "Unknown".to_string(),
    };
    let avail_kb = match data
        .split_whitespace()
        .nth(3)
        .and_then(|column| column.parse::<f64>().ok())
    {
        Some(n) if n >= 0.0 => n,
        _ => return "Unknown".to_string(),
    };
    format!("{:.2}GB", avail_kb / 1024.0 / 1024.0)
}

#[cfg(target_os = "windows")]
fn install_channel() -> String {
    let executable = std::env::current_exe()
        .ok()
        .map(|path| path.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if executable.contains("\\target\\debug\\") || executable.contains("\\target\\release\\") {
        "Source".to_string()
    } else {
        "Script".to_string()
    }
}

#[cfg(not(target_os = "windows"))]
fn install_channel() -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_string))
        .unwrap_or_default();
    if exe.contains("/Cellar/incodex/")
        || exe.contains("/opt/homebrew/opt/incodex/")
        || exe.contains("/usr/local/opt/incodex/")
        || exe.ends_with("/opt/homebrew/bin/incodex")
        || exe.ends_with("/opt/homebrew/bin/inc")
    {
        return "Homebrew".to_string();
    }
    if exe.ends_with(".ts") || exe.ends_with(".cts") {
        return "Source".to_string();
    }
    "Script".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn macos_public_version_facts_contract_is_unchanged() {
        let facts = VersionFacts {
            version: "0.5.0".into(),
            macos: "15.4".into(),
            architecture: "arm64".into(),
            kernel: "24.4.0".into(),
            sip: "Enabled".into(),
            disk_free: "177.88GB".into(),
            install: "Script".into(),
            shell: "/bin/zsh".into(),
        };

        assert!(format_version_report(&facts).contains("macOS: 15.4"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_version_probe_does_not_search_path_for_cmd() {
        let source = include_str!("version.rs");
        let path_search = ["probe(", "\"cmd\""].concat();
        let trusted_probe = ["system_binary_path(", "\"cmd.exe\""].concat();
        assert!(!source.contains(&path_search));
        assert!(source.contains(&trusted_probe));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn format_prints_version_machine_install_and_shell() {
        let text = format_version_report(&VersionFacts {
            version: "0.2.0".into(),
            windows: "Microsoft Windows [Version 10.0.26100.0]".into(),
            architecture: "x86_64".into(),
            install: "Source".into(),
            shell: "cmd.exe".into(),
        });
        assert!(text.starts_with("Incodex version 0.2.0\n"));
        assert!(text.contains("Windows: Microsoft Windows"));
        assert!(text.contains("Architecture: x86_64"));
        assert!(text.contains("Install: Source"));
        assert!(text.ends_with("\n\n"));
    }
}
