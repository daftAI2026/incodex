/*
 * [INPUT]: 依赖原生 Runtime 发布器与调用方隔离 HOME。
 * [OUTPUT]: 构造缺失当前代际字段的旧 Runtime 指针。
 * [POS]: 兼容与修复回归夹具；不修改实装 Runtime。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
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
