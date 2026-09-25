#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};

pub(crate) const DYLIB_NAME: &str = "incodex-permission-ui.dylib";
pub(crate) const HOST_EXECUTABLE_NAME: &str = "incodex-permission-host";
pub(crate) const MANIFEST_NAME: &str = "runtime-native-manifest.json";

#[cfg(target_os = "macos")]
static DYLIB_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/dist/incodex-permission-ui.dylib"
));

#[cfg(target_os = "macos")]
static HOST_EXECUTABLE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/dist/incodex-permission-host"
));

#[cfg(target_os = "macos")]
const MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/dist/runtime-native-manifest.json"
));

#[cfg(target_os = "macos")]
const SOURCE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/permission-views.swift"
));

#[cfg(target_os = "macos")]
const HOST_SOURCE_BYTES: &[&[u8]] = &[
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/permission-host.swift"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/permission-host-presenter.swift"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/permission-host-settings.swift"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/permission-host-flight.swift"
    )),
];

#[cfg(target_os = "macos")]
static NATIVE_FILES: &[(&str, &[u8])] = &[
    (DYLIB_NAME, DYLIB_BYTES),
    (HOST_EXECUTABLE_NAME, HOST_EXECUTABLE_BYTES),
    (MANIFEST_NAME, MANIFEST_BYTES),
];

pub(crate) fn files() -> &'static [(&'static str, &'static [u8])] {
    #[cfg(target_os = "macos")]
    {
        NATIVE_FILES
    }
    #[cfg(not(target_os = "macos"))]
    {
        &[]
    }
}

pub(crate) fn validate() -> Result<(), String> {
    #[cfg(not(target_os = "macos"))]
    {
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        validate_with_source_bytes(SOURCE_BYTES)
    }
}

pub(crate) fn validate_with_source_bytes(source: &[u8]) -> Result<(), String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = source;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        if !is_macho(DYLIB_BYTES) {
            return Err("embedded macOS native helper is not a Mach-O binary".into());
        }
        if !is_macho(HOST_EXECUTABLE_BYTES) {
            return Err("embedded macOS native guide host is not a Mach-O binary".into());
        }
        let manifest: serde_json::Value = serde_json::from_slice(MANIFEST_BYTES)
            .map_err(|error| format!("invalid embedded native manifest: {error}"))?;
        if manifest["schemaVersion"] != 1
            || manifest["platform"] != "macos"
            || manifest["abiVersion"] != 1
            || manifest["minimumMacOS"] != "12.0"
            || manifest["architectures"] != serde_json::json!(["arm64", "x86_64"])
        {
            return Err("embedded native manifest metadata is invalid".into());
        }
        let expected_source = manifest["sourceSha256"]
            .as_str()
            .filter(|value| is_sha256(value))
            .ok_or("embedded native manifest sourceSha256 is invalid")?;
        if expected_source != sha256_hex(source) {
            return Err("embedded native manifest source hash mismatch".into());
        }
        let expected_host_source = manifest["hostSourceSha256"]
            .as_str()
            .filter(|value| is_sha256(value))
            .ok_or("embedded native manifest hostSourceSha256 is invalid")?;
        if expected_host_source != sha256_concat_hex(HOST_SOURCE_BYTES) {
            return Err("embedded native manifest host source hash mismatch".into());
        }
        let files = manifest["files"]
            .as_object()
            .ok_or("embedded native manifest files are missing")?;
        if files.len() != 2 {
            return Err("embedded native manifest files are invalid".into());
        }
        let expected = files
            .get(DYLIB_NAME)
            .and_then(serde_json::Value::as_str)
            .filter(|value| is_sha256(value))
            .ok_or("embedded native manifest dylib hash is invalid")?;
        let actual = sha256_hex(DYLIB_BYTES);
        if expected != actual {
            return Err("embedded native manifest dylib hash mismatch".into());
        }
        let expected_host = files
            .get(HOST_EXECUTABLE_NAME)
            .and_then(serde_json::Value::as_str)
            .filter(|value| is_sha256(value))
            .ok_or("embedded native manifest host hash is invalid")?;
        if expected_host != sha256_hex(HOST_EXECUTABLE_BYTES) {
            return Err("embedded native manifest host hash mismatch".into());
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn is_macho(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    matches!(
        u32::from_be_bytes(bytes[..4].try_into().expect("four bytes")),
        0xcafebabe | 0xcffaedfe | 0xfeedface | 0xfeedfacf
    )
}

#[cfg(target_os = "macos")]
fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(target_os = "macos")]
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(target_os = "macos")]
fn sha256_concat_hex(parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part);
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn shipped_native_host_matches_embedded_source_contract() {
        super::validate().expect("shipping native host and publisher must bind the same sources");
    }
}
