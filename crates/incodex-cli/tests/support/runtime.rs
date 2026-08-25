#![allow(dead_code)]

use std::fs;
use std::path::Path;

pub fn make_legacy_version_runtime_stale(home: &Path) {
    let root = home.join(".incodex");
    incodex_runtime_bundle::publish(&root).expect("publish fixture Runtime");
    let current_path = root.join("runtime/current.json");
    let mut current: serde_json::Value =
        serde_json::from_slice(&fs::read(&current_path).unwrap()).unwrap();
    current["version"] = serde_json::Value::String("0.3.1".into());
    current.as_object_mut().unwrap().remove("manifestSha256");
    current.as_object_mut().unwrap().remove("sourceCommit");
    fs::write(
        current_path,
        format!("{}\n", serde_json::to_string_pretty(&current).unwrap()),
    )
    .unwrap();
}
