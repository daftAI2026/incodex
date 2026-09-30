use std::path::{Path, PathBuf};

fn prepare_cache(_source: &Path, _cache_root: &Path) -> Result<PathBuf, String> {
    Err("official runtime cache has not been prepared before startup".into())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::prepare_cache;

    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        cache: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "incodex-official-cache-{}",
                crate::windows_install_state::random_registration_id().unwrap()
            ));
            let source = root.join("source");
            let cache = root.join("cache");
            fs::create_dir_all(source.join("bin/node_modules/fixture")).unwrap();
            fs::create_dir(&cache).unwrap();
            fs::write(
                source.join("manifest.json"),
                br#"{"platform":"windows","arch":"x64","node_path":"bin/node.exe","node_repl_path":"bin/node_repl.exe","node_modules":"bin/node_modules"}"#,
            ).unwrap();
            fs::write(source.join("bin/node.exe"), b"node fixture").unwrap();
            fs::write(source.join("bin/node_repl.exe"), b"repl fixture").unwrap();
            fs::write(
                source.join("bin/node_modules/fixture/index.js"),
                b"module fixture",
            )
            .unwrap();
            Self {
                root,
                source,
                cache,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn cold_official_runtime_is_ready_before_the_policy_clock_starts() {
        let fixture = Fixture::new();
        let prepared = prepare_cache(&fixture.source, &fixture.cache)
            .expect("cold runtime preparation must precede official bootstrap");
        assert_eq!(prepared.parent(), Some(fixture.cache.as_path()));
        assert_eq!(prepared.file_name().unwrap().to_string_lossy().len(), 16);
        for name in [
            "manifest.json",
            "bin/node.exe",
            "bin/node_repl.exe",
            "bin/node_modules/fixture/index.js",
        ] {
            assert_eq!(
                fs::read(prepared.join(name)).unwrap(),
                fs::read(fixture.source.join(name)).unwrap()
            );
        }
        assert_eq!(
            fs::read_dir(&fixture.cache).unwrap().count(),
            1,
            "only the committed cache may remain"
        );
    }

    #[test]
    fn warm_official_cache_is_reused_without_overwriting_its_contents() {
        let fixture = Fixture::new();
        let first = prepare_cache(&fixture.source, &fixture.cache).unwrap();
        fs::write(first.join("official-owned-marker"), b"preserve").unwrap();
        let second = prepare_cache(&fixture.source, &fixture.cache).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            fs::read(second.join("official-owned-marker")).unwrap(),
            b"preserve"
        );
    }

    #[test]
    fn content_change_uses_a_new_cache_key_without_removing_the_old_generation() {
        let fixture = Fixture::new();
        let first = prepare_cache(&fixture.source, &fixture.cache).unwrap();
        fs::write(fixture.source.join("bin/node.exe"), b"new node fixture").unwrap();
        let second = prepare_cache(&fixture.source, &fixture.cache).unwrap();
        assert_ne!(first, second);
        assert!(first.exists());
        assert_eq!(
            fs::read(second.join("bin/node.exe")).unwrap(),
            b"new node fixture"
        );
    }
}
