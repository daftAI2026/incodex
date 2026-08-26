use std::process::Command;

pub struct VersionFacts {
    pub version: String,
    pub system_name: String,
    pub system_version: String,
    pub architecture: String,
    pub kernel: Option<String>,
    pub security_name: Option<String>,
    pub security_status: Option<String>,
    pub disk_free: Option<String>,
    pub install: String,
    pub shell: String,
}

pub fn format_version_report(facts: &VersionFacts) -> String {
    let mut lines = vec![
        format!("Incodex version {}", facts.version),
        format!("{}: {}", facts.system_name, facts.system_version),
        format!("Architecture: {}", facts.architecture),
    ];
    if let Some(kernel) = &facts.kernel {
        lines.push(format!("Kernel: {kernel}"));
    }
    if let (Some(name), Some(status)) = (&facts.security_name, &facts.security_status) {
        lines.push(format!("{name}: {status}"));
    }
    if let Some(disk_free) = &facts.disk_free {
        lines.push(format!("Disk Free: {disk_free}"));
    }
    lines.push(format!("Install: {}", facts.install));
    lines.push(format!("Shell: {}", facts.shell));
    format!("{}\n\n", lines.join("\n"))
}

#[cfg(not(target_os = "windows"))]
pub fn collect_version_facts() -> VersionFacts {
    VersionFacts {
        version: env!("CARGO_PKG_VERSION").to_string(),
        system_name: "macOS".to_string(),
        system_version: probe("sw_vers", &["-productVersion"]),
        architecture: probe("uname", &["-m"]),
        kernel: Some(probe("uname", &["-r"])),
        security_name: Some("SIP".to_string()),
        security_status: Some(sip_status()),
        disk_free: Some(disk_free()),
        install: install_channel(),
        shell: std::env::var("SHELL").unwrap_or_else(|_| "Unknown".to_string()),
    }
}

#[cfg(target_os = "windows")]
pub fn collect_version_facts() -> VersionFacts {
    let version = probe("cmd", &["/C", "ver"]);
    VersionFacts {
        version: env!("CARGO_PKG_VERSION").to_string(),
        system_name: "Windows".to_string(),
        system_version: version.clone(),
        architecture: std::env::consts::ARCH.to_string(),
        kernel: None,
        security_name: None,
        security_status: None,
        disk_free: None,
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
    "Unsupported".to_string()
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
    fn format_prints_version_machine_install_and_shell() {
        let text = format_version_report(&VersionFacts {
            version: "0.2.0".into(),
            system_name: "macOS".into(),
            system_version: "15.4".into(),
            architecture: "arm64".into(),
            kernel: Some("24.4.0".into()),
            security_name: Some("SIP".into()),
            security_status: Some("Enabled".into()),
            disk_free: Some("177.88GB".into()),
            install: "Script".into(),
            shell: "/bin/zsh".into(),
        });
        assert!(text.starts_with("Incodex version 0.2.0\n"));
        assert!(text.contains("macOS: 15.4"));
        assert!(text.contains("Architecture: arm64"));
        assert!(text.contains("Install: Script"));
        assert!(text.ends_with("\n\n"));
    }
}
