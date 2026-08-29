//! Embed the current cross-platform Electron Runtime from one artifact catalog.

include!(concat!(env!("OUT_DIR"), "/runtime_assets.rs"));

pub fn loader_source() -> &'static str {
    LOADER_SOURCE
}

pub fn manifest_source() -> &'static str {
    MANIFEST_SOURCE
}

pub fn external_artifact_names() -> &'static [&'static str] {
    EXTERNAL_ARTIFACT_NAMES
}

pub fn external_files() -> &'static [(&'static str, &'static str)] {
    EXTERNAL_FILES
}
