//! MIT ASAR subset used by Incodex. Do not depend on an AGPL asar crate.
//!
//! This crate is still a stub. The tests in `tests/fixtures.rs` are the
//! contract; they must fail until pack/extract/patch are implemented.

use std::path::{Path, PathBuf};

pub const LOADER_NAME: &str = "incodex-loader.cjs";
pub const MARKER_KEY: &str = "__incodex";

#[derive(Debug, Clone)]
pub struct Archive {
    pub path: PathBuf,
    pub header_string: String,
}

#[derive(Debug, Clone)]
pub struct PackageMain {
    pub main: String,
    pub already_patched: bool,
    pub install_id: Option<String>,
}

impl Archive {
    pub fn open(_path: impl AsRef<Path>) -> Result<Self, String> {
        Err("asar open not implemented".into())
    }

    pub fn header_hash(&self) -> String {
        String::new()
    }

    pub fn file_hash(&self) -> String {
        String::new()
    }

    pub fn list(&self) -> Vec<String> {
        Vec::new()
    }

    pub fn extract(&self, _rel: &str) -> Result<Vec<u8>, String> {
        Err("asar extract not implemented".into())
    }

    pub fn read_package_main(&self) -> Result<PackageMain, String> {
        Err("asar package.json not implemented".into())
    }

    pub fn has_only_loader(&self) -> bool {
        false
    }
}

pub fn pack_dir(_src: &Path, _dest: &Path) -> Result<(), String> {
    Err("pack_dir not implemented".into())
}

pub fn pack_dir_unpacked(
    _src: &Path,
    _dest: &Path,
    _unpacked_prefixes: &[&str],
) -> Result<(), String> {
    Err("pack_dir_unpacked not implemented".into())
}

pub fn patch_asar(
    _asar_path: &Path,
    _loader_source: &str,
    _install_id: Option<&str>,
) -> Result<(String, String), String> {
    Err("patch_asar not implemented".into())
}

pub fn electron_asar_integrity(_asar_path: &Path) -> Result<String, String> {
    Err("electron_asar_integrity not implemented".into())
}
