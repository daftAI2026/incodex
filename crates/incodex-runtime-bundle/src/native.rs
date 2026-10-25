/*
 * [INPUT]: 依赖已提交原生资产、远程密钥有序源摘要与固定 ABI。
 * [OUTPUT]: 验证 Runtime 原生资产与独立 device-key helper、adapter 的源码和摘要。
 * [POS]: macOS 原生资产边界；CLI 只发布已验证字节。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};

pub(crate) const DYLIB_NAME: &str = "incodex-permission-ui.dylib";
pub(crate) const HOST_EXECUTABLE_NAME: &str = "incodex-permission-host";
pub(crate) const DEVICE_KEY_HELPER_NAME: &str = "incodex-device-key-helper";
pub(crate) const DEVICE_KEY_ADAPTER_NAME: &str = "incodex-device-key-adapter.node";
#[cfg(test)]
pub(crate) const DEVICE_KEY_ADAPTER_MANIFEST_NAME: &str =
    "incodex-device-key-adapter-manifest.json";
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
static DEVICE_KEY_HELPER_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/dist/incodex-device-key-helper"
));

#[cfg(target_os = "macos")]
static DEVICE_KEY_ADAPTER_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/dist/incodex-device-key-adapter.node"
));

#[cfg(target_os = "macos")]
static DEVICE_KEY_ADAPTER_MANIFEST: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/dist/incodex-device-key-adapter-manifest.json"
));

#[cfg(target_os = "macos")]
pub(crate) fn device_key_helper_bytes() -> &'static [u8] {
    DEVICE_KEY_HELPER_BYTES
}

#[cfg(target_os = "macos")]
pub(crate) fn device_key_adapter_bytes() -> &'static [u8] {
    DEVICE_KEY_ADAPTER_BYTES
}

#[cfg(target_os = "macos")]
pub(crate) fn device_key_adapter_manifest_bytes() -> &'static [u8] {
    DEVICE_KEY_ADAPTER_MANIFEST
}

#[cfg(target_os = "macos")]
const MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/dist/runtime-native-manifest.json"
));

#[cfg(target_os = "macos")]
const SOURCE_BYTES: &[&[u8]] = &[
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/permission-views.swift"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/permission-initial.swift"
    )),
];

#[cfg(target_os = "macos")]
const HOST_SOURCE_BYTES: &[&[u8]] = &[
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/permission-host.swift"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/permission-host-presenter-geometry.swift"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/permission-host-presenter-views.swift"
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
    ("incodex-remote-key-compat.node", REMOTE_KEY_BYTES),
    ("incodex-remote-key-manifest.json", REMOTE_KEY_MANIFEST),
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
        validate_remote_key()?;
        validate_device_key()?;
        validate_device_key_adapter()?;
        validate_with_source_bytes(SOURCE_BYTES)
    }
}

pub(crate) fn validate_with_source_bytes(source: &[&[u8]]) -> Result<(), String> {
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
        if expected_source != sha256_concat_hex(source) {
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

#[cfg(all(test, target_os = "macos"))]
mod remote_key_tests {
    #[test]
    fn runtime_catalog_remote_key_artifacts_match_the_public_legacy_generation() {
        assert_eq!(
            super::sha256_hex(super::REMOTE_KEY_SOURCES[0]),
            "2b9841ae501d9833d7a45e803d82a5b31d459c21930733c6b1f51ec65c2e2169"
        );
        assert_eq!(
            super::sha256_hex(super::REMOTE_KEY_BYTES),
            "4418cab06eda46fcaaf6480d5881846d979d4becca808f84d66b4daeeb2327fd"
        );
        assert_eq!(
            super::sha256_hex(super::REMOTE_KEY_MANIFEST),
            "56fddd68c3c744a50418641a08bf2b2c98f9403371fcd3cccc6eb1aa2f2ea081"
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(super::REMOTE_KEY_MANIFEST).unwrap();
        assert_eq!(
            manifest["sourceSha256"],
            "373361afc15c663af39c1c90721c826ac673efc3b925d056b439b54aaea810b1"
        );
    }

    #[test]
    fn publisher_includes_verified_remote_key_compatibility_artifacts() {
        let files = super::files();
        assert!(files
            .iter()
            .any(|(name, _)| *name == "incodex-remote-key-compat.node"));
        assert!(files
            .iter()
            .any(|(name, _)| *name == "incodex-remote-key-manifest.json"));
    }

    #[test]
    fn remote_key_manifest_contains_only_the_runtime_addon() {
        let manifest: serde_json::Value =
            serde_json::from_slice(super::REMOTE_KEY_MANIFEST).expect("remote key manifest");
        let files = manifest["files"]
            .as_object()
            .expect("remote key manifest files");

        assert_eq!(files.len(), 1);
        assert!(files.contains_key("incodex-remote-key-compat.node"));
        assert!(!files.contains_key(super::DEVICE_KEY_HELPER_NAME));
    }

    #[test]
    fn remote_key_source_binding_excludes_device_helper_sources() {
        assert_eq!(super::REMOTE_KEY_SOURCES.len(), 9);
    }

    #[test]
    fn runtime_catalog_does_not_include_device_helper_or_its_manifest() {
        let files = crate::required_runtime_files().collect::<Vec<_>>();

        assert_eq!(
            files.len() + 1,
            23,
            "include the fixed loader in this count"
        );
        assert!(!files.iter().any(|name| {
            *name == super::DEVICE_KEY_HELPER_NAME || *name == "incodex-device-key-manifest.json"
        }));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod device_key_tests {
    #[test]
    fn device_key_manifest_accepts_exact_sources() {
        let manifest: serde_json::Value =
            serde_json::from_slice(super::DEVICE_KEY_MANIFEST).expect("device key manifest");
        assert_eq!(manifest["schemaVersion"], 1);
        assert_eq!(manifest["platform"], "macos");
        assert_eq!(manifest["abiVersion"], 1);
        assert_eq!(manifest["minimumMacOS"], "12.0");
        assert_eq!(
            manifest["architectures"],
            serde_json::json!(["arm64", "x86_64"])
        );
        assert_eq!(
            manifest["files"].as_object().map(|files| files.len()),
            Some(1)
        );
        assert!(manifest["files"]
            .get(super::DEVICE_KEY_HELPER_NAME)
            .is_some());

        super::validate_device_key_with_source_bytes(super::DEVICE_KEY_SOURCE_BYTES)
            .expect("current helper source must match its independent manifest");
    }

    #[test]
    fn device_key_validation_rejects_changed_helper_source() {
        let mut changed = super::DEVICE_KEY_SOURCE_BYTES.to_vec();
        changed[0] = b"changed helper source";
        assert!(super::validate_device_key_with_source_bytes(&changed).is_err());
    }
}

#[cfg(target_os = "macos")]
const REMOTE_KEY_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/dist/incodex-remote-key-compat.node"
));
#[cfg(target_os = "macos")]
const REMOTE_KEY_MANIFEST: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/dist/incodex-remote-key-manifest.json"
));
#[cfg(target_os = "macos")]
const DEVICE_KEY_MANIFEST: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/macos/dist/incodex-device-key-manifest.json"
));
#[cfg(target_os = "macos")]
const DEVICE_KEY_SOURCE_BYTES: &[&[u8]] = &[
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/device-key-helper.m"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/device-key-consumer.h"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/host-identity.h"
    )),
];
#[cfg(target_os = "macos")]
const DEVICE_KEY_ADAPTER_SOURCE_BYTES: &[&[u8]] = &[
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/device-key-adapter.m"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/device-key-bridge.h"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/host-identity.h"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/vendor/node/node_api.h"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/vendor/node/node_api_types.h"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/vendor/node/js_native_api.h"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/vendor/node/js_native_api_types.h"
    )),
];
#[cfg(target_os = "macos")]
const REMOTE_KEY_SOURCES: &[&[u8]] = &[
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/remote-key-compat.m"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/remote-key-policy.h"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/host-identity.h"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/vendor/fishhook/fishhook.c"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/vendor/fishhook/fishhook.h"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/vendor/node/node_api.h"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/vendor/node/node_api_types.h"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/vendor/node/js_native_api.h"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../native/macos/vendor/node/js_native_api_types.h"
    )),
];
#[cfg(target_os = "macos")]
fn validate_remote_key() -> Result<(), String> {
    let manifest: serde_json::Value =
        serde_json::from_slice(REMOTE_KEY_MANIFEST).map_err(|e| e.to_string())?;
    if !is_macho(REMOTE_KEY_BYTES)
        || manifest["schemaVersion"] != 1
        || manifest["platform"] != "macos"
        || manifest["abiVersion"] != 1
        || manifest["minimumMacOS"] != "12.0"
        || manifest["architectures"] != serde_json::json!(["arm64", "x86_64"])
        || manifest["sourceSha256"] != sha256_concat_hex(REMOTE_KEY_SOURCES)
        || manifest["files"].as_object().map(|f| f.len()) != Some(1)
        || manifest["files"]["incodex-remote-key-compat.node"] != sha256_hex(REMOTE_KEY_BYTES)
    {
        return Err("embedded remote key native artifact/source mismatch".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn validate_device_key() -> Result<(), String> {
    validate_device_key_with_source_bytes(DEVICE_KEY_SOURCE_BYTES)
}

#[cfg(target_os = "macos")]
fn validate_device_key_with_source_bytes(source: &[&[u8]]) -> Result<(), String> {
    let manifest: serde_json::Value =
        serde_json::from_slice(DEVICE_KEY_MANIFEST).map_err(|e| e.to_string())?;
    let manifest_fields = [
        "schemaVersion",
        "platform",
        "abiVersion",
        "minimumMacOS",
        "architectures",
        "sourceSha256",
        "files",
    ];
    let Some(object) = manifest.as_object() else {
        return Err("embedded device key manifest is not an object".into());
    };
    if object.len() != manifest_fields.len()
        || manifest_fields
            .iter()
            .any(|field| !object.contains_key(*field))
        || !is_macho(device_key_helper_bytes())
        || manifest["schemaVersion"] != 1
        || manifest["platform"] != "macos"
        || manifest["abiVersion"] != 1
        || manifest["minimumMacOS"] != "12.0"
        || manifest["architectures"] != serde_json::json!(["arm64", "x86_64"])
        || source.len() != DEVICE_KEY_SOURCE_BYTES.len()
    {
        return Err("embedded device key manifest metadata is invalid".into());
    }

    let expected_source = manifest["sourceSha256"]
        .as_str()
        .filter(|value| is_sha256(value))
        .ok_or("embedded device key manifest sourceSha256 is invalid")?;
    if expected_source != sha256_concat_hex(source) {
        return Err("embedded device key manifest source hash mismatch".into());
    }

    let files = manifest["files"]
        .as_object()
        .ok_or("embedded device key manifest files are missing")?;
    if files.len() != 1 {
        return Err("embedded device key manifest files are invalid".into());
    }
    let expected_helper = files
        .get(DEVICE_KEY_HELPER_NAME)
        .and_then(serde_json::Value::as_str)
        .filter(|value| is_sha256(value))
        .ok_or("embedded device key manifest helper hash is invalid")?;
    if expected_helper != sha256_hex(device_key_helper_bytes()) {
        return Err("embedded device key manifest helper hash mismatch".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn validate_device_key_adapter() -> Result<(), String> {
    validate_device_key_adapter_with_source_bytes(DEVICE_KEY_ADAPTER_SOURCE_BYTES)
}

#[cfg(target_os = "macos")]
fn validate_device_key_adapter_with_source_bytes(source: &[&[u8]]) -> Result<(), String> {
    let manifest: serde_json::Value = serde_json::from_slice(DEVICE_KEY_ADAPTER_MANIFEST)
        .map_err(|error| format!("invalid device key adapter manifest: {error}"))?;
    let manifest_fields = [
        "schemaVersion",
        "platform",
        "abiVersion",
        "minimumMacOS",
        "architectures",
        "sourceSha256",
        "files",
    ];
    let Some(object) = manifest.as_object() else {
        return Err("device key adapter manifest is not an object".into());
    };
    if object.len() != manifest_fields.len()
        || manifest_fields
            .iter()
            .any(|field| !object.contains_key(*field))
        || DEVICE_KEY_ADAPTER_MANIFEST.last() != Some(&b'\n')
        || !is_macho(device_key_adapter_bytes())
        || manifest["schemaVersion"] != 1
        || manifest["platform"] != "macos"
        || manifest["abiVersion"] != 1
        || manifest["minimumMacOS"] != "12.0"
        || manifest["architectures"] != serde_json::json!(["arm64", "x86_64"])
        || source.len() != DEVICE_KEY_ADAPTER_SOURCE_BYTES.len()
    {
        return Err("device key adapter manifest metadata is invalid".into());
    }

    let expected_source = manifest["sourceSha256"]
        .as_str()
        .filter(|value| is_sha256(value))
        .ok_or("device key adapter manifest sourceSha256 is invalid")?;
    if expected_source != sha256_concat_hex(source) {
        return Err("device key adapter manifest source hash mismatch".into());
    }

    let files = manifest["files"]
        .as_object()
        .ok_or("device key adapter manifest files are missing")?;
    if files.len() != 1 {
        return Err("device key adapter manifest files are invalid".into());
    }
    let expected_adapter = files
        .get(DEVICE_KEY_ADAPTER_NAME)
        .and_then(serde_json::Value::as_str)
        .filter(|value| is_sha256(value))
        .ok_or("device key adapter manifest hash is invalid")?;
    if expected_adapter != sha256_hex(device_key_adapter_bytes()) {
        return Err("device key adapter manifest hash mismatch".into());
    }
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod device_key_adapter_tests {
    #[test]
    fn adapter_manifest_binds_only_the_standalone_native_adapter() {
        let manifest: serde_json::Value =
            serde_json::from_slice(super::DEVICE_KEY_ADAPTER_MANIFEST).expect("adapter manifest");
        assert_eq!(manifest["schemaVersion"], 1);
        assert_eq!(manifest["platform"], "macos");
        assert_eq!(manifest["abiVersion"], 1);
        assert_eq!(manifest["minimumMacOS"], "12.0");
        assert_eq!(
            manifest["architectures"],
            serde_json::json!(["arm64", "x86_64"])
        );
        assert_eq!(
            manifest["files"].as_object().map(|files| files.len()),
            Some(1)
        );
        assert!(manifest["files"]
            .get(super::DEVICE_KEY_ADAPTER_NAME)
            .is_some());
        assert!(super::validate_device_key_adapter_with_source_bytes(
            super::DEVICE_KEY_ADAPTER_SOURCE_BYTES
        )
        .is_ok());
        assert!(!crate::required_runtime_files().any(|name| name == super::DEVICE_KEY_ADAPTER_NAME));
    }

    #[test]
    fn adapter_validation_rejects_changed_or_reordered_sources() {
        let source = super::DEVICE_KEY_ADAPTER_SOURCE_BYTES;
        let mut changed = source.to_vec();
        changed[0] = b"changed adapter source";
        assert!(super::validate_device_key_adapter_with_source_bytes(&changed).is_err());

        let mut reordered = source.to_vec();
        reordered.swap(0, 1);
        assert!(super::validate_device_key_adapter_with_source_bytes(&reordered).is_err());
    }
}
