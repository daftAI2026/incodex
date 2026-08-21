use std::path::{Component, Path, PathBuf};

/// 从官方 App 的 Info.plist 解析实际可执行文件，不对缺失字段猜测默认值。
pub fn resolve_executable(app: &Path) -> Result<PathBuf, String> {
    let executable = incodex_macos::read_plist_executable(app)?;
    let name = Path::new(&executable);
    let mut components = name.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(format!(
            "CFBundleExecutable is not a plain executable name: {executable}"
        ));
    }
    let binary = app.join("Contents/MacOS").join(name);
    if !binary.is_file() {
        return Err(format!(
            "Codex executable from CFBundleExecutable not found: {}",
            binary.display()
        ));
    }
    Ok(binary)
}
