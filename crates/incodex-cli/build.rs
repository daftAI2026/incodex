/*
 * [INPUT]: 依赖受控native源及共享诊断/pending头、xcrun SDK 与系统 clang/codesign。
 * [OUTPUT]: 构建嵌入 CLI 的 macOS 更新/Keychain/私有签名 store 资产与 Windows 执行 manifest，不构建产品 TypeScript CLI。
 * [POS]: 原生资产编译边界；签名 store 是有界一次性工具，不是常驻权限 broker。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
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

fn sign_adhoc(path: &Path, identifier: &str) {
    let status = Command::new("codesign")
        .args(["--force", "--sign", "-", "--identifier", identifier, "--"])
        .arg(path)
        .status()
        .unwrap_or_else(|error| panic!("cannot start codesign for {}: {error}", path.display()));
    assert!(status.success(), "codesign failed for {}", path.display());
}

fn main() {
    embed_windows_manifest();
    println!("cargo:rerun-if-changed=native/macos_update_coordinator.m");
    println!("cargo:rerun-if-changed=native/macos_sparkle_interpose.m");
    println!("cargo:rerun-if-changed=native/macos_update_diagnostics.h");
    println!("cargo:rerun-if-changed=native/macos_update_pending.h");
    println!("cargo:rerun-if-changed=native/macos_keychain_helper.m");
    println!("cargo:rerun-if-changed=native/macos_launcher_icon.m");
    println!("cargo:rerun-if-changed=native/macos_signing_store.m");
    println!("cargo:rerun-if-changed=native/macos_keychain_provider.c");
    println!("cargo:rerun-if-changed=native/fishhook.c");
    println!("cargo:rerun-if-changed=native/fishhook.h");
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

    let target = env::var("TARGET").expect("Cargo did not provide TARGET");
    let arch = if target.starts_with("aarch64-") {
        "arm64"
    } else if target.starts_with("x86_64-") {
        "x86_64"
    } else {
        panic!("unsupported macOS launcher artwork target: {target}");
    };
    let launcher_icon_object = out.join("macos_launcher_icon.o");
    compile(
        &clang,
        &sdk,
        "native/macos_launcher_icon.m",
        &launcher_icon_object,
        &[
            "-fobjc-arc",
            "-O2",
            "-arch",
            arch,
            "-mmacosx-version-min=12.0",
            "-c",
        ],
    );
    let launcher_icon_archive = out.join("libincodex_launcher_icon.a");
    let archive_status = Command::new("/usr/bin/ar")
        .arg("crs")
        .arg(&launcher_icon_archive)
        .arg(&launcher_icon_object)
        .status()
        .expect("cannot run ar for launcher artwork bridge");
    assert!(
        archive_status.success(),
        "ar failed for launcher artwork bridge"
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=incodex_launcher_icon");
    for framework in ["AppKit", "CoreGraphics", "Foundation", "ImageIO"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }

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
    let keychain_helper = out.join("incodex-keychain-helper");
    compile(
        &clang,
        &sdk,
        "native/macos_keychain_helper.m",
        &keychain_helper,
        &[
            "-fobjc-arc",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-framework",
            "CoreFoundation",
            "-framework",
            "Foundation",
            "-framework",
            "LocalAuthentication",
            "-framework",
            "Security",
        ],
    );
    sign_adhoc(&keychain_helper, "com.daftai.incodex.keychain-helper");
    let signing_store = out.join("incodex-signing-store");
    compile(
        &clang,
        &sdk,
        "native/macos_signing_store.m",
        &signing_store,
        &[
            "-fobjc-arc",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-framework",
            "CoreFoundation",
            "-framework",
            "Foundation",
            "-framework",
            "Security",
        ],
    );
    sign_adhoc(&signing_store, "com.daftai.incodex.signing-store");
    let helper_hash_define = concat!(
        "-DINCODEX_KEYCHAIN_HELPER_SHA256=\"",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "\""
    );
    compile(
        &clang,
        &sdk,
        "native/macos_keychain_provider.c",
        &out.join("IncodexKeyProvider.dylib"),
        &[
            "-Wall",
            "-Wextra",
            "-Werror",
            "-dynamiclib",
            "native/fishhook.c",
            helper_hash_define,
            "-framework",
            "CoreFoundation",
            "-framework",
            "Security",
            "-Wl,-install_name,@loader_path/IncodexKeyProvider.dylib",
        ],
    );
}

fn embed_windows_manifest() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }

    let manifest = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo must provide CARGO_MANIFEST_DIR"),
    )
    .join("assets/incodex-windows.manifest")
    .canonicalize()
    .expect("canonicalize the Windows execution manifest");
    let manifest = manifest
        .to_str()
        .expect("Windows execution manifest path must be valid UTF-8");

    println!("cargo:rerun-if-changed={manifest}");
    println!("cargo:rustc-link-arg-bin=incodex=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bin=incodex=/MANIFESTINPUT:{manifest}");
}
