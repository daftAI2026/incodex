use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("crate directory"));
    let root = crate_dir.join("../..");
    let catalog_path = root.join("runtime-artifacts.json");
    let catalog = read_catalog(&catalog_path);
    let output = render(&catalog.loader, &catalog.external);
    let output_path = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"))
        .join("runtime_assets.rs");
    fs::write(output_path, output).expect("write generated Runtime asset table");

    println!("cargo:rerun-if-changed={}", catalog_path.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("dist/runtime-manifest.json").display()
    );
    for name in
        std::iter::once(catalog.loader.as_str()).chain(catalog.external.iter().map(String::as_str))
    {
        println!(
            "cargo:rerun-if-changed={}",
            root.join("dist").join(name).display()
        );
    }
}

struct Catalog {
    loader: String,
    external: Vec<String>,
}

fn read_catalog(path: &Path) -> Catalog {
    let body = fs::read_to_string(path).expect("read Runtime artifact catalog");
    let value: serde_json::Value =
        serde_json::from_str(&body).expect("parse Runtime artifact catalog");
    let loader = value
        .get("loader")
        .and_then(serde_json::Value::as_str)
        .expect("Runtime artifact catalog loader")
        .to_string();
    let external = value
        .get("external")
        .and_then(serde_json::Value::as_array)
        .expect("Runtime artifact catalog external files")
        .iter()
        .map(|value| value.as_str().expect("Runtime artifact name").to_string())
        .collect::<Vec<_>>();
    validate(&loader, &external);
    Catalog { loader, external }
}

fn validate(loader: &str, external: &[String]) {
    assert!(safe_name(loader), "unsafe Runtime loader name: {loader}");
    assert!(
        !external.is_empty(),
        "Runtime external artifact catalog is empty"
    );
    let mut names = BTreeSet::from([loader]);
    for name in external {
        assert!(safe_name(name), "unsafe Runtime artifact name: {name}");
        assert!(
            names.insert(name),
            "duplicate Runtime artifact name: {name}"
        );
    }
}

fn safe_name(name: &str) -> bool {
    let Some(stem) = name.strip_prefix("incodex-").and_then(|name| {
        name.strip_suffix(".cjs")
            .or_else(|| name.strip_suffix(".js"))
    }) else {
        return false;
    };
    !stem.is_empty()
        && stem
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
}

fn render(loader: &str, external: &[String]) -> String {
    let loader_literal = format!("{loader:?}");
    let external_names = external
        .iter()
        .map(|name| format!("    {name:?},\n"))
        .collect::<String>();
    let external_files = external
        .iter()
        .map(|name| {
            format!(
                "    ({name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../dist/{name}\"))),\n"
            )
        })
        .collect::<String>();

    format!(
        "pub const LOADER_NAME: &str = {loader_literal};\n\
const LOADER_SOURCE: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../dist/{loader}\"));\n\
const MANIFEST_SOURCE: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../dist/runtime-manifest.json\"));\n\
const EXTERNAL_ARTIFACT_NAMES: &[&str] = &[\n{external_names}];\n\
const EXTERNAL_FILES: &[(&str, &str)] = &[\n{external_files}];\n"
    )
}
