use std::collections::BTreeMap;
use std::ffi::c_void;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use windows_sys::core::{GUID, HRESULT};
use windows_sys::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};

use crate::windows_process::{
    assign_debugged_process_to_job, snapshot_process_ids, terminate_process_id, WindowsPendingJob,
    WindowsProcessTree,
};

const PACKAGE_DEBUGGER_MODE: &str = "__incodex_windows_package_debugger";

const CLSID_APPLICATION_ACTIVATION_MANAGER: GUID =
    GUID::from_u128(0x45ba127d_10a8_46ea_8ab7_56ea9078943c);
const IID_APPLICATION_ACTIVATION_MANAGER: GUID =
    GUID::from_u128(0x2e941141_7f97_4756_ba1d_9decde894a3d);
const CLSID_PACKAGE_DEBUG_SETTINGS: GUID = GUID::from_u128(0xb1aec16f_2383_4852_b0e9_8f0b1dc66b4d);
const IID_PACKAGE_DEBUG_SETTINGS: GUID = GUID::from_u128(0xf27c3930_8029_4ad1_94e3_3dba417810c1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsActivationRequest {
    package_full_name: String,
    app_user_model_id: String,
    arguments: String,
    environment: Vec<u16>,
}

impl WindowsActivationRequest {
    pub fn new<I>(
        package_full_name: &str,
        app_user_model_id: &str,
        arguments: I,
        environment: BTreeMap<String, OsString>,
    ) -> Result<Self, String>
    where
        I: IntoIterator<Item = OsString>,
    {
        validate_text(package_full_name, "package full name")?;
        validate_text(app_user_model_id, "app user model id")?;
        let arguments = arguments
            .into_iter()
            .map(|argument| quote_windows_argument(&argument))
            .collect::<Result<Vec<_>, _>>()?
            .join(" ");
        let environment = environment_block(environment)?;
        Ok(Self {
            package_full_name: package_full_name.to_string(),
            app_user_model_id: app_user_model_id.to_string(),
            arguments,
            environment,
        })
    }

    pub fn package_full_name(&self) -> &str {
        &self.package_full_name
    }

    pub fn app_user_model_id(&self) -> &str {
        &self.app_user_model_id
    }

    pub fn arguments(&self) -> &str {
        &self.arguments
    }

    pub fn environment(&self) -> &[u16] {
        &self.environment
    }
}

pub fn activate_packaged_kill_on_drop(
    request: &WindowsActivationRequest,
) -> Result<WindowsProcessTree, String> {
    let _apartment = ComApartment::initialize()?;
    let existing_processes = snapshot_process_ids()
        .map_err(|error| format!("cannot snapshot Windows processes before activation: {error}"))?;
    let pending_job = WindowsPendingJob::create()
        .map_err(|error| format!("cannot create Windows activation Job Object: {error}"))?;
    let package = wide_nul(request.package_full_name());
    let app_user_model_id = wide_nul(request.app_user_model_id());
    let arguments = wide_nul(request.arguments());
    let debugger_command = debugger_command_line(pending_job.name())?;
    let debugger_command = wide_nul(&debugger_command);
    let mut debugging =
        PackageDebugGuard::enable(&package, &debugger_command, request.environment())?;
    let manager = ComPtr::create(
        &CLSID_APPLICATION_ACTIVATION_MANAGER,
        &IID_APPLICATION_ACTIVATION_MANAGER,
        "Windows application activation manager",
    )?;
    let mut process_id = 0;
    let result = unsafe {
        let vtable = *(manager.raw() as *mut *const ApplicationActivationManagerVtable);
        ((*vtable).activate_application)(
            manager.raw(),
            app_user_model_id.as_ptr(),
            arguments.as_ptr(),
            0,
            &mut process_id,
        )
    };
    if failed(result) {
        let disable = debugging.disable();
        return Err(join_cleanup_error(
            hresult_message("cannot activate the Windows Codex package", result),
            disable,
        ));
    }
    if process_id == 0 || existing_processes.contains(&process_id) {
        let disable = debugging.disable();
        return Err(join_cleanup_error(
            "Windows package activation did not create a new isolated Codex process".to_string(),
            disable,
        ));
    }

    let mut process_tree = match pending_job.attach(process_id) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            let terminated = terminate_process_id(process_id);
            let disable = debugging.disable();
            let mut message = format!(
                "cannot contain activated Windows Codex process {process_id} in a Job Object: {error}"
            );
            if let Err(error) = terminated {
                message.push_str(&format!("; cannot terminate new process: {error}"));
            }
            return Err(join_cleanup_error(message, disable));
        }
    };
    if let Err(error) = debugging.disable() {
        let _ = process_tree.terminate();
        return Err(error);
    }
    Ok(process_tree)
}

pub(crate) fn try_run_package_debugger(arguments: &[String]) -> Option<Result<(), String>> {
    if arguments.first().map(String::as_str) != Some(PACKAGE_DEBUGGER_MODE) {
        return None;
    }
    let parsed = (|| {
        let job_name = flag_value(arguments, "--job")?;
        let process_id = flag_value(arguments, "-p")?
            .parse::<u32>()
            .map_err(|_| "Windows package debugger received an invalid process id".to_string())?;
        let thread_id = flag_value(arguments, "-tid")?
            .parse::<u32>()
            .map_err(|_| "Windows package debugger received an invalid thread id".to_string())?;
        assign_debugged_process_to_job(job_name, process_id, thread_id)
            .map_err(|error| format!("Windows package debugger failed: {error}"))
    })();
    Some(parsed)
}

fn flag_value<'a>(arguments: &'a [String], flag: &str) -> Result<&'a str, String> {
    arguments
        .windows(2)
        .find_map(|pair| (pair[0] == flag).then_some(pair[1].as_str()))
        .ok_or_else(|| format!("Windows package debugger is missing {flag}"))
}

fn debugger_command_line(job_name: &str) -> Result<String, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot locate the Incodex package debugger: {error}"))?;
    [
        executable.into_os_string(),
        OsString::from(PACKAGE_DEBUGGER_MODE),
        OsString::from("--job"),
        OsString::from(job_name),
    ]
    .iter()
    .map(|argument| quote_windows_argument(argument))
    .collect::<Result<Vec<_>, _>>()
    .map(|arguments| arguments.join(" "))
}

fn join_cleanup_error(primary: String, cleanup: Result<(), String>) -> String {
    match cleanup {
        Ok(()) => primary,
        Err(error) => format!("{primary}; {error}"),
    }
}

fn wide_nul(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn failed(result: HRESULT) -> bool {
    result < 0
}

fn hresult_message(action: &str, result: HRESULT) -> String {
    format!("{action}: HRESULT 0x{:08X}", result as u32)
}

struct ComApartment {
    uninitialize: bool,
}

impl ComApartment {
    fn initialize() -> Result<Self, String> {
        let result = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        if !failed(result) {
            Ok(Self { uninitialize: true })
        } else if result == RPC_E_CHANGED_MODE {
            Ok(Self {
                uninitialize: false,
            })
        } else {
            Err(hresult_message(
                "cannot initialize COM for Windows package activation",
                result,
            ))
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

struct PackageDebugGuard {
    interface: ComPtr,
    package: Vec<u16>,
    active: bool,
}

impl PackageDebugGuard {
    fn enable(
        package: &[u16],
        debugger_command: &[u16],
        environment: &[u16],
    ) -> Result<Self, String> {
        let interface = ComPtr::create(
            &CLSID_PACKAGE_DEBUG_SETTINGS,
            &IID_PACKAGE_DEBUG_SETTINGS,
            "Windows package debug settings",
        )?;
        let vtable = unsafe { *(interface.raw() as *mut *const PackageDebugSettingsVtable) };
        let result = unsafe {
            ((*vtable).enable_debugging)(
                interface.raw(),
                package.as_ptr(),
                debugger_command.as_ptr(),
                environment.as_ptr(),
            )
        };
        if failed(result) {
            return Err(hresult_message(
                "cannot set the isolated Windows Codex environment",
                result,
            ));
        }
        Ok(Self {
            interface,
            package: package.to_vec(),
            active: true,
        })
    }

    fn disable(&mut self) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        let vtable = unsafe { *(self.interface.raw() as *mut *const PackageDebugSettingsVtable) };
        let result =
            unsafe { ((*vtable).disable_debugging)(self.interface.raw(), self.package.as_ptr()) };
        if failed(result) {
            Err(hresult_message(
                "cannot restore Windows Codex package debug settings",
                result,
            ))
        } else {
            self.active = false;
            Ok(())
        }
    }
}

impl Drop for PackageDebugGuard {
    fn drop(&mut self) {
        let _ = self.disable();
    }
}

struct ComPtr(*mut c_void);

impl ComPtr {
    fn create(clsid: &GUID, iid: &GUID, label: &str) -> Result<Self, String> {
        let mut raw = ptr::null_mut();
        let result = unsafe {
            CoCreateInstance(clsid, ptr::null_mut(), CLSCTX_INPROC_SERVER, iid, &mut raw)
        };
        if failed(result) || raw.is_null() {
            Err(hresult_message(&format!("cannot create {label}"), result))
        } else {
            Ok(Self(raw))
        }
    }

    fn raw(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for ComPtr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let vtable = *(self.0 as *mut *const UnknownVtable);
                ((*vtable).release)(self.0);
            }
        }
    }
}

#[repr(C)]
struct UnknownVtable {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct ApplicationActivationManagerVtable {
    base: UnknownVtable,
    activate_application:
        unsafe extern "system" fn(*mut c_void, *const u16, *const u16, u32, *mut u32) -> HRESULT,
}

#[repr(C)]
struct PackageDebugSettingsVtable {
    base: UnknownVtable,
    enable_debugging:
        unsafe extern "system" fn(*mut c_void, *const u16, *const u16, *const u16) -> HRESULT,
    disable_debugging: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
}

fn validate_text(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.contains('\0') {
        return Err(format!("Windows {label} is empty or contains NUL"));
    }
    Ok(())
}

fn environment_block(environment: BTreeMap<String, OsString>) -> Result<Vec<u16>, String> {
    let mut block = Vec::new();
    for (name, value) in environment {
        if name.is_empty() || name.contains(['=', '\0']) {
            return Err("Windows activation environment name is invalid".to_string());
        }
        let value: Vec<u16> = value.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(format!(
                "Windows activation environment value for {name} contains NUL"
            ));
        }
        block.extend(name.encode_utf16());
        block.push('=' as u16);
        block.extend(value);
        block.push(0);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    Ok(block)
}

fn quote_windows_argument(argument: &OsStr) -> Result<String, String> {
    let wide: Vec<u16> = argument.encode_wide().collect();
    if wide.contains(&0) {
        return Err("Windows activation argument contains NUL".to_string());
    }
    let needs_quotes =
        wide.is_empty() || wide.iter().any(|unit| matches!(*unit, 0x20 | 0x09 | 0x22));
    if !needs_quotes {
        return String::from_utf16(&wide)
            .map_err(|_| "Windows activation argument is not valid Unicode".to_string());
    }

    let mut quoted = Vec::with_capacity(wide.len() + 2);
    quoted.push(b'"' as u16);
    let mut backslashes = 0;
    for unit in wide {
        if unit == b'\\' as u16 {
            backslashes += 1;
            continue;
        }
        if unit == b'"' as u16 {
            quoted.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2 + 1));
        } else {
            quoted.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
        }
        backslashes = 0;
        quoted.push(unit);
    }
    quoted.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
    quoted.push(b'"' as u16);
    String::from_utf16(&quoted)
        .map_err(|_| "Windows activation argument is not valid Unicode".to_string())
}
