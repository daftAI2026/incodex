#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};

pub(crate) const DYLIB_NAME: &str = "incodex-permission-ui.dylib";
pub(crate) const MANIFEST_NAME: &str = "runtime-native-manifest.json";

#[cfg(target_os = "macos")]
const DYLIB_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/dist/incodex-permission-ui.dylib"
));

#[cfg(target_os = "macos")]
const MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/dist/runtime-native-manifest.json"
));

pub(crate) fn files() -> &'static [(&'static str, &'static [u8])] {
    #[cfg(target_os = "macos")]
    {
        &[(DYLIB_NAME, DYLIB_BYTES), (MANIFEST_NAME, MANIFEST_BYTES)]
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
        if !is_macho(DYLIB_BYTES) {
            return Err("embedded macOS native helper is not a Mach-O binary".into());
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
        manifest["sourceSha256"]
            .as_str()
            .filter(|value| is_sha256(value))
            .ok_or("embedded native manifest sourceSha256 is invalid")?;
        let files = manifest["files"]
            .as_object()
            .ok_or("embedded native manifest files are missing")?;
        if files.len() != 1 {
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
