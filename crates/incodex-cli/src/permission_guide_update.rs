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
