use incodex_macos::write_asar_integrity;
use serde_json::json;
use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn updating_app_asar_preserves_the_other_integrity_entries() {
    let root = std::env::temp_dir().join(format!(
        "incodex-integrity-plist-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let contents = root.join("ChatGPT.app/Contents");
    fs::create_dir_all(&contents).unwrap();
    let plist = contents.join("Info.plist");
    let old = json!({"CFBundleExecutable":"ChatGPT", "ElectronAsarIntegrity": {
        "Resources/app.asar":{"algorithm":"SHA256", "hash":"a".repeat(64)},
        "Resources/other.asar":{"algorithm":"SHA256", "hash":"b".repeat(64)}
    }});
    fs::write(&plist, serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(Command::new("plutil")
        .args(["-convert", "xml1"])
        .arg(&plist)
        .status()
        .unwrap()
        .success());
    write_asar_integrity(&root.join("ChatGPT.app"), &"c".repeat(64)).unwrap();
    let output = Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(&plist)
        .output()
        .unwrap();
    assert!(output.status.success());
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(
        actual["ElectronAsarIntegrity"]["Resources/other.asar"],
        old["ElectronAsarIntegrity"]["Resources/other.asar"],
        "Electron hashes the entire dictionary, not only app.asar"
    );
    assert_eq!(
        actual["ElectronAsarIntegrity"]["Resources/app.asar"]["hash"],
        "c".repeat(64)
    );
}
