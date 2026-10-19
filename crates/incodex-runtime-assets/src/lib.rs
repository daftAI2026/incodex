/*
 * [INPUT]: 依赖 build.rs 从唯一资产目录生成的静态表。
 * [OUTPUT]: 提供 loader、manifest 与外置 Runtime 资产的只读访问。
 * [POS]: 跨平台嵌入边界；供原生 CLI 与 macOS 发布器复用。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
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
