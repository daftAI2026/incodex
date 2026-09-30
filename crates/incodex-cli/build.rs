use std::env;
use std::path::PathBuf;

fn main() {
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
