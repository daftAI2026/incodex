/*
 * [INPUT]: 依赖运行CLI的绝对路径、HOME稳定入口和Homebrew Cellar布局。
 * [OUTPUT]: 返回指向当前CLI的稳定词法入口；无入口时返回CLI绝对路径。
 * [POS]: macOS launch CLI路径适配器；与薄启动器协作，不改变CLI安装布局。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

const CLI_NAME: &str = "incodex";

pub(crate) fn stable_entry(current: &Path, home: &Path) -> Result<PathBuf, String> {
    let resolved_current = fs::canonicalize(current).map_err(|error| {
        format!(
            "cannot resolve running Incodex CLI {}: {error}",
            current.display()
        )
    })?;
    let absolute_home = if home.is_absolute() {
        home.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("cannot resolve current directory: {error}"))?
            .join(home)
    };

    let mut candidates = vec![
        absolute_home.join(".local/bin").join(CLI_NAME),
        absolute_home.join(".cargo/bin").join(CLI_NAME),
    ];
    if let Some(homebrew_entry) = homebrew_entry(&resolved_current) {
        candidates.push(homebrew_entry);
    }

    for candidate in candidates {
        if fs::canonicalize(&candidate).is_ok_and(|resolved| resolved == resolved_current) {
            return Ok(candidate);
        }
    }

    Ok(resolved_current)
}

fn homebrew_entry(current: &Path) -> Option<PathBuf> {
    let components = current.components().collect::<Vec<_>>();
    let cellar_index = components.windows(5).position(|window| {
        matches!(window[0], Component::Normal(value) if value == OsStr::new("Cellar"))
            && matches!(window[1], Component::Normal(value) if value == OsStr::new(CLI_NAME))
            && matches!(window[2], Component::Normal(_))
            && matches!(window[3], Component::Normal(value) if value == OsStr::new("bin"))
            && matches!(window[4], Component::Normal(value) if value == OsStr::new(CLI_NAME))
    })?;

    let mut prefix = PathBuf::new();
    for component in &components[..cellar_index] {
        prefix.push(component.as_os_str());
    }
    Some(prefix.join("bin").join(CLI_NAME))
}

#[cfg(test)]
mod tests {
    use super::stable_entry;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "incodex-launch-cli-path-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn file(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"fake CLI").unwrap();
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn stable_user_cli_path_survives_symlink_generation_switch() {
        let fixture = Fixture::new();
        let home = fixture.0.join("home");
        fs::create_dir(&home).unwrap();
        let first = fixture.file("incodex-v1");
        let second = fixture.file("incodex-v2");
        let stable = home.join(".local/bin/incodex");
        fs::create_dir_all(stable.parent().unwrap()).unwrap();
        symlink(&first, &stable).unwrap();

        let first_entry = stable_entry(&first, &home).unwrap();
        fs::remove_file(&stable).unwrap();
        symlink(&second, &stable).unwrap();
        let second_entry = stable_entry(&second, &home).unwrap();

        assert_eq!(first_entry, stable);
        assert_eq!(second_entry, stable);
    }

    #[test]
    fn homebrew_cellar_path_uses_prefix_bin_entry() {
        let fixture = Fixture::new();
        let home = fixture.0.join("home");
        fs::create_dir(&home).unwrap();
        let prefix = fixture.0.join("homebrew");
        let cellar_cli = prefix.join("Cellar/incodex/1.5.1/bin/incodex");
        fs::create_dir_all(cellar_cli.parent().unwrap()).unwrap();
        fs::write(&cellar_cli, b"fake Cellar CLI").unwrap();
        let stable = prefix.join("bin/incodex");
        fs::create_dir_all(stable.parent().unwrap()).unwrap();
        symlink(&cellar_cli, &stable).unwrap();

        assert_eq!(
            stable_entry(&cellar_cli, &home).unwrap(),
            fs::canonicalize(prefix).unwrap().join("bin/incodex")
        );
    }

    #[test]
    fn source_cli_without_a_matching_stable_link_keeps_its_absolute_path() {
        let fixture = Fixture::new();
        let home = fixture.0.join("home");
        fs::create_dir(&home).unwrap();
        let source_cli = fixture.file("target/debug/incodex");

        assert_eq!(
            stable_entry(&source_cli, &home).unwrap(),
            fs::canonicalize(source_cli).unwrap()
        );
    }
}
