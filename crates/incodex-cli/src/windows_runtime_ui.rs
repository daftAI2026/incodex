//! File events and authorization for the existing installed UI bridge.
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use incodex_core::windows_path::reject_reparse_ancestors;
use incodex_core::windows_session::verify_private_acl;
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::Storage::FileSystem::{
    FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
};
use windows_sys::Win32::System::Threading::WaitForSingleObject;

use crate::runtime_ui_update::{UiGeneration, UiUpdate};
use crate::windows_install_state::{
    read_windows_install_state, WindowsInstallPhase, WindowsInstallState,
};

struct DirectoryChange(HANDLE);
impl DirectoryChange {
    fn new(directory: &Path) -> Result<Self, String> {
        reject_reparse_ancestors(directory)?;
        verify_private_acl(directory)?;
        let wide = directory
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        // Nonrecursive: session writes and staging assets must not trigger scans.
        let handle = unsafe {
            FindFirstChangeNotificationW(
                wide.as_ptr(),
                0,
                FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE,
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(format!(
                "cannot watch Runtime publication: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self(handle))
    }
    fn changed(&self) -> Result<bool, String> {
        match unsafe { WaitForSingleObject(self.0, 0) } {
            WAIT_TIMEOUT => Ok(false),
            WAIT_OBJECT_0 => {
                // Rearm before preparation so publications during activation survive.
                if unsafe { FindNextChangeNotification(self.0) } == 0 {
                    return Err(format!(
                        "cannot rearm Runtime notification: {}",
                        std::io::Error::last_os_error()
                    ));
                }
                Ok(true)
            }
            _ => Err(format!(
                "Runtime notification failed: {}",
                std::io::Error::last_os_error()
            )),
        }
    }
}
impl Drop for DirectoryChange {
    fn drop(&mut self) {
        unsafe {
            FindCloseChangeNotification(self.0);
        }
    }
}

pub(crate) struct InstalledUiUpdates {
    root: PathBuf,
    authorization: WindowsInstallState,
    controller: UiUpdate,
    watches: Option<[DirectoryChange; 2]>,
    initial: bool,
    process_identity: serde_json::Value,
}

fn authorized_state(
    root: &Path,
    expected: &WindowsInstallState,
) -> Result<WindowsInstallState, String> {
    let state =
        read_windows_install_state(root)?.ok_or("Windows Runtime integration was removed")?;
    if !state.desired_enabled()
        || !matches!(
            state.phase,
            WindowsInstallPhase::EnabledObserved | WindowsInstallPhase::EnabledUnobserved
        )
        || state.registration_id != expected.registration_id
        || state.package_full_name != expected.package_full_name
        || state.helper_path != expected.helper_path
        || state.helper_sha256 != expected.helper_sha256
    {
        return Err("Windows Runtime authorization changed".into());
    }
    let registration = crate::windows_registration::read_windows_debug_registration(root)?
        .ok_or("Windows Runtime registration was removed")?;
    if !crate::windows_registration::registration_matches_install_state(&registration, &state) {
        return Err("Windows Runtime registration changed".into());
    }
    Ok(state)
}

fn selected_generation(
    root: &Path,
    authorization: &WindowsInstallState,
) -> Result<UiGeneration, String> {
    let _gate = crate::windows_install_state::acquire_windows_install_state()?;
    let state = authorized_state(root, authorization)?;
    crate::windows_runtime::read_selected_windows_ui_generation(root, &state.runtime_release)
}

impl InstalledUiUpdates {
    pub fn new(
        root: &Path,
        release: &str,
        package: &str,
        source: &str,
        main_pid: u32,
    ) -> Result<Self, String> {
        let authorization =
            read_windows_install_state(root)?.ok_or("Windows Runtime integration unavailable")?;
        let helper = std::env::current_exe().map_err(|error| error.to_string())?;
        if authorization.package_full_name != package
            || crate::windows_file::canonical_regular_file(
                &authorization.helper_path,
                "installed helper",
            )? != crate::windows_file::canonical_regular_file(&helper, "running helper")?
        {
            return Err("Windows Runtime observer does not own this installation".into());
        }
        authorized_state(root, &authorization)?;
        let initial = crate::windows_runtime::read_verified_windows_ui_generation(root, release)?;
        if initial.source != source {
            return Err("Windows Runtime startup source changed".into());
        }
        let watches = [
            DirectoryChange::new(root)?,
            DirectoryChange::new(&root.join("runtime"))?,
        ];
        Ok(Self {
            root: root.to_path_buf(),
            authorization,
            controller: UiUpdate::new(initial),
            watches: Some(watches),
            initial: true,
            process_identity: process_identity(main_pid),
        })
    }
    pub fn source(&self) -> &str {
        &self.controller.active().source
    }
    pub fn renderer_invalidated(&mut self) {
        self.controller.renderer_invalidated();
        self.initial = true;
        self.report("renderer-unconfirmed");
    }
    pub fn native_open_executable(&self, requested: Option<&str>) -> Result<PathBuf, String> {
        crate::windows_update::native_open_executable_for_runtime(
            &self.root,
            &self.authorization.helper_path,
            self.controller.request_release(requested)?,
        )
    }
    pub fn validate_request_release(&self, requested: Option<&str>) -> Result<(), String> {
        self.controller.request_release(requested).map(|_| ())
    }
    pub fn refresh(
        &mut self,
        apply: impl FnMut(&UiGeneration) -> Result<bool, String>,
        commit: impl FnMut(&UiGeneration) -> Result<bool, String>,
    ) {
        let Some(watches) = self.watches.as_ref() else {
            return;
        };
        let changed = (|| {
            Ok::<_, String>(
                watches[0].changed()? | watches[1].changed()? | std::mem::take(&mut self.initial),
            )
        })();
        let changed = match changed {
            Ok(value) => value,
            Err(error) => {
                self.watches = None;
                self.controller.notification_failed();
                self.report("watch-unavailable");
                eprintln!("Windows Runtime notification unavailable: {error}");
                return;
            }
        };
        if !changed {
            return;
        }
        match selected_generation(&self.root, &self.authorization) {
            Ok(candidate) => {
                // Preserve activation and rollback results instead of relabeling every error as preparation.
                let _ = self.controller.activate_with_commit(
                    candidate,
                    |expected| {
                        selected_generation(&self.root, &self.authorization)
                            .map(|actual| actual == *expected)
                    },
                    apply,
                    commit,
                );
            }
            Err(_) => self.controller.preparation_failed(),
        }
        self.report(self.controller.phase());
    }
    fn report(&self, phase: &str) {
        let mut snapshot = self.controller.snapshot();
        snapshot["helper"] = self.process_identity["helper"].clone();
        snapshot["app"] = self.process_identity["app"].clone();
        if let Err(error) = crate::windows_update_observer_log::installed_ui_runtime_status(
            &self.root, phase, &snapshot,
        ) {
            eprintln!("Windows Runtime update diagnostics unavailable: {error}");
        }
    }
}

pub(crate) fn process_identity(main_pid: u32) -> serde_json::Value {
    serde_json::json!({
        "helper":{"pid":std::process::id(),"createdFileTime":crate::windows_update_repair::process_creation_time(std::process::id()).ok()},
        "app":{"pid":main_pid,"createdFileTime":crate::windows_update_repair::process_creation_time(main_pid).ok()}
    })
}

pub(crate) fn report_controller_unavailable(root: &Path, main_pid: u32) {
    let mut snapshot = process_identity(main_pid);
    snapshot.as_object_mut().unwrap().extend(
        serde_json::json!({
            "published":null, "controller":null, "activeUi":null, "main":null,
            "rendererAckId":null, "actionAckId":null,
            "failure":"controller-unavailable", "restartRequired":null, "installRequired":null
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    let _ = crate::windows_update_observer_log::installed_ui_runtime_status(
        root,
        "controller-unavailable",
        &snapshot,
    );
}

#[cfg(test)]
mod tests {
    use super::DirectoryChange;
    use incodex_core::windows_session::ensure_private_windows_dir;

    #[test]
    fn unavailable_controller_reports_both_acknowledgements_as_unknown() {
        let root = std::env::temp_dir().join(format!(
            "incodex-unavailable-log-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        ensure_private_windows_dir(&root).unwrap();
        super::report_controller_unavailable(&root, 0);
        let bytes = std::fs::read(root.join("windows/installed-ui.json")).unwrap();
        let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        for field in [
            "published",
            "controller",
            "activeUi",
            "main",
            "rendererAckId",
            "actionAckId",
            "restartRequired",
            "installRequired",
        ] {
            assert_eq!(
                record["runtime"].get(field),
                Some(&serde_json::Value::Null),
                "{field} must be explicitly unknown"
            );
        }
        assert_eq!(record["runtime"]["failure"], "controller-unavailable");
    }

    #[test]
    fn process_identity_captures_real_creation_times_and_keeps_absence_unknown() {
        let current = super::process_identity(std::process::id());
        assert_eq!(current["helper"]["pid"], std::process::id());
        assert_eq!(current["app"]["pid"], std::process::id());
        assert!(current["helper"]["createdFileTime"].as_u64().unwrap() > 0);
        assert_eq!(
            current["helper"]["createdFileTime"],
            current["app"]["createdFileTime"]
        );
        assert!(super::process_identity(0)["app"]["createdFileTime"].is_null());
    }

    #[test]
    fn native_notifications_cover_both_publication_roots_and_rearm_without_session_scans() {
        let root = std::env::temp_dir().join(format!(
            "incodex-ui-watch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        ensure_private_windows_dir(&root).unwrap();
        let runtime = ensure_private_windows_dir(&root.join("runtime")).unwrap();
        let sessions = ensure_private_windows_dir(&root.join("sessions")).unwrap();
        let owned_session = ensure_private_windows_dir(&sessions.join("test")).unwrap();
        let session = ensure_private_windows_dir(&owned_session.join("home")).unwrap();
        {
            let state_watch = DirectoryChange::new(&root).unwrap();
            let runtime_watch = DirectoryChange::new(&runtime).unwrap();
            std::fs::write(session.join("owned-test-data"), "unrelated session write").unwrap();
            assert!(!state_watch.changed().unwrap());
            assert!(!runtime_watch.changed().unwrap());
            for generation in ["a", "b", "a"] {
                crate::windows_runtime::replace_private_file(
                    &runtime,
                    &runtime.join("current.json"),
                    generation.as_bytes(),
                )
                .unwrap();
                assert!(runtime_watch.changed().unwrap());
                crate::windows_runtime::replace_private_file(
                    &root,
                    &root.join("windows-install.json"),
                    generation.as_bytes(),
                )
                .unwrap();
                assert!(state_watch.changed().unwrap());
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
