use std::process::Command;

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

pub fn format_version_report(facts: &VersionFacts) -> String {
    format!(
        "Incodex version {}\nmacOS: {}\nArchitecture: {}\nKernel: {}\nSIP: {}\nDisk Free: {}\nInstall: {}\nShell: {}\n\n",
        facts.version,
        facts.macos,
        facts.architecture,
        facts.kernel,
        facts.sip,
        facts.disk_free,
        facts.install,
        facts.shell
    )
}

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

fn probe(cmd: &str, args: &[&str]) -> String {
    let output = Command::new(cmd).args(args).output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => "Unknown".to_string(),
    }
}

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

fn disk_free() -> String {
    let raw = probe("df", &["-k", "/"]);
    let data = match raw.lines().nth(1) {
        Some(line) => line,
        None => return "Unknown".to_string(),
    };
    let cols: Vec<&str> = data.split_whitespace().collect();
    let avail_kb = match cols.get(3).and_then(|col| col.parse::<f64>().ok()) {
        Some(n) if n >= 0.0 => n,
        _ => return "Unknown".to_string(),
    };
    format!("{:.2}GB", avail_kb / 1024.0 / 1024.0)
}

fn install_channel() -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_string))
        .unwrap_or_default();
    if exe.contains("/Cellar/incodex/")
        || exe.contains("/opt/homebrew/opt/incodex/")
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
    fn format_prints_version_machine_install_and_shell() {
        let text = format_version_report(&VersionFacts {
            version: "0.2.0".into(),
            macos: "15.4".into(),
            architecture: "arm64".into(),
            kernel: "24.4.0".into(),
            sip: "Enabled".into(),
            disk_free: "177.88GB".into(),
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
