/*
 * [INPUT]: 依赖只读 Runtime 验证、固定原生资产与共用权限状态机。
 * [OUTPUT]: 提供不启动 App、不倒退 Runtime 的更新后引导。
 * [POS]: 短期授权流程保留已验证资产；新发布不重建进行中的卡片。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use sha2::{Digest, Sha256};

struct PinnedFactory {
    executable: PathBuf,
    digest: String,
}
impl PinnedFactory {
    fn prepare(root: &Path) -> Result<Self, String> {
        let identity = incodex_runtime_bundle::runtime_identity()?;
        let deployed = incodex_runtime_bundle::inspect_deployed(root)?
            .ok_or("permission update Runtime is missing")?;
        if !identity.matches(&deployed) {
            return Err("permission update Runtime selection changed".into());
        }
        let factory = Self {
            executable: root
                .join("runtime")
                .join(deployed.release)
                .join(HOST_EXECUTABLE_NAME),
            digest: identity
                .files
                .get(HOST_EXECUTABLE_NAME)
                .ok_or("permission host hash missing")?
                .clone(),
        };
        factory.verify()?;
        Ok(factory)
    }
    fn verify(&self) -> Result<(), String> {
        let metadata = fs::symlink_metadata(&self.executable).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > 16 * 1024 * 1024
            || metadata.permissions().mode() & 0o022 != 0
            || metadata.permissions().mode() & 0o111 == 0
            || fs::canonicalize(&self.executable).map_err(|e| e.to_string())? != self.executable
        {
            return Err("unsafe pinned permission host".into());
        }
        let digest: String = Sha256::digest(fs::read(&self.executable).map_err(|e| e.to_string())?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if digest != self.digest {
            return Err("pinned permission host hash changed".into());
        }
        verify_codesign(&self.executable)
    }
}
impl GuideHostFactory for PinnedFactory {
    fn start(&mut self, root: &Path, app: &Path) -> Result<Box<dyn GuideHost>, String> {
        self.start_with_permissions(
            root,
            app,
            &PermissionSnapshot {
                accessibility: AccessibilityStatus::Unknown,
                screen: Some(ScreenPermissionStatus::Unknown),
            },
        )
    }
    fn start_with_permissions(
        &mut self,
        _root: &Path,
        app: &Path,
        permissions: &PermissionSnapshot,
    ) -> Result<Box<dyn GuideHost>, String> {
        if app != Path::new(OFFICIAL_APP_PATH) {
            return Err("permission update needs the default App".into());
        }
        self.verify()?;
        Ok(Box::new(
            ProcessGuideHost::spawn_executable_with_permissions(
                &self.executable,
                GuideCopyContext::Installed,
                Some(permissions),
            )?,
        ))
    }
}
pub(crate) fn run<F>(root: &Path, app: &Path, mut verify_target: F) -> Result<Outcome, String>
where
    F: FnMut() -> Result<(), String>,
{
    let mut factory = PinnedFactory::prepare(root)?;
    let mut ops = SystemGuideOps {
        app,
        screen: crate::permission_screen_probe::InstalledScreenProbe::for_app(root, app),
    };
    flow::run_permission_guide_with_mode(
        &mut ops,
        root,
        app,
        &mut verify_target,
        &mut factory,
        flow::GuideTimeouts {
            ready: HOST_READY_TIMEOUT,
            choice: HOST_CHOICE_TIMEOUT,
            guide: HOST_GUIDE_TIMEOUT,
        },
        flow::GuideMode::AfterUpdate {
            check_timeout: Duration::from_secs(30),
        },
    )
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    struct Home(PathBuf);
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn permission_host_lookup_never_republishes_an_older_runtime() {
        let home = Home(
            std::env::temp_dir().canonicalize().unwrap().join(format!(
                "incodex-permission-pin-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )),
        );
        fs::create_dir(&home.0).unwrap();
        let root = home.0.join(".incodex");
        let a = incodex_runtime_bundle::publish(&root).unwrap();
        let pinned = PinnedFactory::prepare(&root).unwrap();
        let current_path = root.join("runtime/current.json");
        let mut pointer: Value = serde_json::from_slice(&fs::read(&current_path).unwrap()).unwrap();
        let a_dir = root.join("runtime").join(&a.release);
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(a_dir.join("runtime-manifest.json")).unwrap())
                .unwrap();
        let name = "incodex-permission-copy.json";
        let changed = [fs::read(a_dir.join(name)).unwrap(), b" \n".to_vec()].concat();
        let hex = |bytes: &[u8]| -> String {
            Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        };
        manifest["files"][name] = hex(&changed).into();
        pointer["files"][name] = manifest["files"][name].clone();
        let body = serde_json::to_vec(&manifest).unwrap();
        let hash = hex(&body);
        let release = format!("releases/{}-{hash}", pointer["version"].as_str().unwrap());
        let b_dir = root.join("runtime").join(&release);
        fs::create_dir(&b_dir).unwrap();
        for file in fs::read_dir(&a_dir).unwrap() {
            let file = file.unwrap();
            fs::copy(file.path(), b_dir.join(file.file_name())).unwrap();
        }
        fs::write(b_dir.join(name), changed).unwrap();
        fs::write(b_dir.join("runtime-manifest.json"), body).unwrap();
        pointer["release"] = release.into();
        pointer["manifestSha256"] = hash.into();
        let before = serde_json::to_vec(&pointer).unwrap();
        fs::write(&current_path, &before).unwrap();
        assert!(incodex_runtime_bundle::inspect_deployed(&root)
            .unwrap()
            .is_some());
        assert!(
            pinned.verify().is_ok(),
            "captured A assets remain valid across B publication"
        );
        assert!(
            PinnedFactory::prepare(&root).is_err(),
            "an old CLI must not select B's unbound assets"
        );
        let _ = verified_native_host_path(&root);
        assert_eq!(
            fs::read(&current_path).unwrap(),
            before,
            "native-host lookup rolled Runtime back to A"
        );
    }
}
