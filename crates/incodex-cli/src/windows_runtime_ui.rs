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
    main_pid: u32,
    helper_created: Option<u64>,
    main_created: Option<u64>,
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
            main_pid,
            helper_created: crate::windows_update_repair::process_creation_time(std::process::id())
                .ok(),
            main_created: crate::windows_update_repair::process_creation_time(main_pid).ok(),
        })
    }
    pub fn source(&self) -> &str {
        &self.controller.active().source
    }
    pub fn native_open_executable(&self) -> Result<PathBuf, String> {
        crate::windows_update::native_open_executable_for_runtime(
            &self.root,
            &self.authorization.helper_path,
            &self.controller.active().release,
        )
    }
    pub fn refresh(&mut self, apply: impl FnMut(&UiGeneration) -> Result<bool, String>) {
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
                self.controller.preparation_failed();
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
                let _ = self.controller.activate(
                    candidate,
                    |expected| {
                        selected_generation(&self.root, &self.authorization)
                            .map(|actual| actual == *expected)
                    },
                    apply,
                );
            }
            Err(_) => self.controller.preparation_failed(),
        }
        self.report(self.controller.phase());
    }
    fn report(&self, phase: &str) {
        let mut snapshot = self.controller.snapshot();
        snapshot["helper"] =
            serde_json::json!({"pid":std::process::id(),"createdFileTime":self.helper_created});
        snapshot["app"] =
            serde_json::json!({"pid":self.main_pid,"createdFileTime":self.main_created});
        if let Err(error) = crate::windows_update_observer_log::installed_ui_runtime_status(
            &self.root, phase, &snapshot,
        ) {
            eprintln!("Windows Runtime update diagnostics unavailable: {error}");
        }
    }
}

pub(crate) fn report_controller_unavailable(root: &Path, main_pid: u32) {
    let snapshot = serde_json::json!({
        "published":null, "controller":null, "activeUi":null, "rendererAckId":null,
        "failure":"controller-unavailable", "restartRequired":null, "installRequired":null,
        "helper":{"pid":std::process::id(),"createdFileTime":crate::windows_update_repair::process_creation_time(std::process::id()).ok()},
        "app":{"pid":main_pid,"createdFileTime":crate::windows_update_repair::process_creation_time(main_pid).ok()}
    });
    let _ = crate::windows_update_observer_log::installed_ui_runtime_status(
        root,
        "controller-unavailable",
        &snapshot,
    );
}
