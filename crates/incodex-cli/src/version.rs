use std::process::Command;

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
    VersionFacts {
        version: env!("CARGO_PKG_VERSION").to_string(),
        macos: probe("sw_vers", &["-productVersion"]),
        architecture: probe("uname", &["-m"]),
        kernel: probe("uname", &["-r"]),
        sip: sip_status(),
        disk_free: disk_free(),
        install: install_channel(),
        shell: std::env::var("SHELL").unwrap_or_else(|_| "Unknown".to_string()),
    }
}

#[cfg(target_os = "windows")]
pub fn collect_version_facts() -> VersionFacts {
    let version = probe("cmd", &["/C", "ver"]);
    VersionFacts {
        version: env!("CARGO_PKG_VERSION").to_string(),
        windows: version,
        architecture: std::env::consts::ARCH.to_string(),
        install: install_channel(),
        shell: std::env::var("COMSPEC").unwrap_or_else(|_| "Unknown".to_string()),
    }
}

fn probe(cmd: &str, args: &[&str]) -> String {
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

#[cfg(not(target_os = "windows"))]
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
