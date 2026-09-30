#![cfg(target_os = "windows")]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use std::slice;

use windows_sys::core::PCWSTR;
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    FindResourceW, LoadLibraryExW, LoadResource, LockResource, SizeofResource,
    LOAD_LIBRARY_AS_DATAFILE,
};

struct DataFile(HMODULE);

impl Drop for DataFile {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0);
        }
    }
}

fn read_embedded_manifest(executable: &Path) -> String {
    let path: Vec<u16> = OsStr::new(executable)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let module = LoadLibraryExW(path.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_AS_DATAFILE);
        assert!(
            !module.is_null(),
            "LoadLibraryExW could not map the CLI executable as a data file"
        );
        let module = DataFile(module);

        let resource = FindResourceW(module.0, resource_id(1), resource_id(24));
        assert!(
            !resource.is_null(),
            "incodex PE is missing RT_MANIFEST resource type 24, id 1"
        );

        let size = SizeofResource(module.0, resource) as usize;
        assert!(size > 0, "RT_MANIFEST resource is empty");
        let loaded = LoadResource(module.0, resource);
        assert!(!loaded.is_null(), "LoadResource could not read RT_MANIFEST");
        let data = LockResource(loaded).cast::<u8>();
        assert!(!data.is_null(), "LockResource returned no RT_MANIFEST data");
        let bytes = slice::from_raw_parts(data, size);

        decode_manifest(bytes)
    }
}

fn resource_id(id: usize) -> PCWSTR {
    id as PCWSTR
}

fn decode_manifest(bytes: &[u8]) -> String {
    if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    String::from_utf8_lossy(bytes).into_owned()
}

fn requested_execution_level_tags(manifest: &str) -> Vec<String> {
    let manifest = manifest.to_ascii_lowercase();
    let mut remaining = manifest.as_str();
    let mut tags = Vec::new();

    while let Some(start) = remaining.find("<requestedexecutionlevel") {
        let tag = &remaining[start..];
        let end = tag
            .find('>')
            .expect("requestedExecutionLevel element must end with '>'");
        tags.push(
            tag[..=end]
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect(),
        );
        remaining = &tag[end + 1..];
    }

    tags
}

fn has_xml_attribute(tag: &str, name: &str, value: &str) -> bool {
    ['"', '\'']
        .into_iter()
        .any(|quote| tag.contains(&format!("{name}={quote}{value}{quote}")))
}

#[test]
fn packaged_cli_embeds_as_invoker_manifest() {
    let executable = Path::new(env!("CARGO_BIN_EXE_incodex"));
    let manifest = read_embedded_manifest(executable);
    let normalized = manifest.to_ascii_lowercase();
    let execution_levels = requested_execution_level_tags(&manifest);

    assert_eq!(
        execution_levels.len(),
        1,
        "PE manifest must declare exactly one requestedExecutionLevel"
    );
    assert!(
        has_xml_attribute(&execution_levels[0], "level", "asinvoker"),
        "PE manifest requestedExecutionLevel must use level=\"asInvoker\""
    );
    assert!(
        has_xml_attribute(&execution_levels[0], "uiaccess", "false"),
        "PE manifest requestedExecutionLevel must use uiAccess=\"false\""
    );
    assert!(
        !normalized.contains("requireadministrator"),
        "PE manifest must not request requireAdministrator"
    );
    assert!(
        !normalized.contains("highestavailable"),
        "PE manifest must not request highestAvailable"
    );
}
