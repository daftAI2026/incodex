//! [INPUT]: 依赖macOS plist读取与目标App文件事实。
//! [OUTPUT]: 解析CFBundleExecutable并返回真实入口路径。
//! [POS]: CLI共用App入口解析；保留相对路径，不猜测执行文件名。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
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
