use std::path::{Path, PathBuf};

pub const WINDOWS_X64_RELEASE_ASSET: &str = "incodex-windows-x64.exe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsStandaloneLayout {
    user_root: PathBuf,
}

impl WindowsStandaloneLayout {
    pub fn new(user_root: &Path) -> Self {
        Self {
            user_root: user_root.to_path_buf(),
        }
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.user_root.join("bin")
    }

    pub fn package_root(&self) -> PathBuf {
        self.user_root.join("packages").join("standalone")
    }

    pub fn release_executable(&self, version: &str) -> Result<PathBuf, String> {
        validate_stable_version(version)?;
        Ok(self
            .package_root()
            .join("releases")
            .join(version)
            .join("incodex.exe"))
    }

    pub fn primary_launcher(&self) -> PathBuf {
        self.bin_dir().join("incodex.cmd")
    }

    pub fn alias_launcher(&self) -> PathBuf {
        self.bin_dir().join("inc.cmd")
    }
}

pub fn windows_release_asset(architecture: &str) -> Result<&'static str, String> {
    match architecture.to_ascii_lowercase().as_str() {
        "x86_64" | "amd64" | "x64" => Ok(WINDOWS_X64_RELEASE_ASSET),
        other => Err(format!("unsupported Windows architecture: {other}")),
    }
}

pub fn expected_release_sha256(manifest: &str, asset: &str) -> Result<String, String> {
    let matches = manifest
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let digest = fields.next()?;
            let name = fields.next()?;
            (name == asset && fields.next().is_none()).then_some(digest)
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!(
            "SHA256SUMS must contain exactly one entry for {asset}"
        ));
    }
    let digest = matches[0];
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("SHA256SUMS contains an invalid digest for {asset}"));
    }
    Ok(digest.to_ascii_lowercase())
}

fn validate_stable_version(version: &str) -> Result<(), String> {
    let parts = version.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(format!("invalid stable Incodex version: {version}"));
    }
    Ok(())
}
