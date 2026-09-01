use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn compile(clang: &Path, sdk: &Path, source: &str, output: &Path, arguments: &[&str]) {
    let status = Command::new(clang)
        .arg("-isysroot")
        .arg(sdk)
        .args(arguments)
        .arg(source)
        .arg("-o")
        .arg(output)
        .status()
        .unwrap_or_else(|error| panic!("cannot start clang for {source}: {error}"));
    assert!(status.success(), "clang failed for {source}");
}

fn main() {
    println!("cargo:rerun-if-changed=native/macos_update_coordinator.m");
    println!("cargo:rerun-if-changed=native/macos_sparkle_interpose.m");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let developer_dir = Command::new("xcrun")
        .args(["--find", "clang"])
        .output()
        .expect("cannot locate clang with xcrun");
    assert!(developer_dir.status.success(), "xcrun cannot locate clang");
    let clang = PathBuf::from(
        String::from_utf8(developer_dir.stdout)
            .expect("xcrun returned a non-UTF-8 clang path")
            .trim(),
    );
    let sdk_output = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .expect("cannot locate the macOS SDK with xcrun");
    assert!(
        sdk_output.status.success(),
        "xcrun cannot locate the macOS SDK"
    );
    let sdk = PathBuf::from(
        String::from_utf8(sdk_output.stdout)
            .expect("xcrun returned a non-UTF-8 SDK path")
            .trim(),
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is unavailable"));

    compile(
        &clang,
        &sdk,
        "native/macos_update_coordinator.m",
        &out.join("incodex-update-coordinator"),
        &[
            "-fobjc-arc",
            "-framework",
            "AppKit",
            "-framework",
            "Foundation",
        ],
    );
    compile(
        &clang,
        &sdk,
        "native/macos_sparkle_interpose.m",
        &out.join("libincodex-sparkle-interpose.dylib"),
        &[
            "-fobjc-arc",
            "-dynamiclib",
            "-framework",
            "Foundation",
            "-Wl,-install_name,@rpath/libincodex-sparkle-interpose.dylib",
        ],
    );
}
