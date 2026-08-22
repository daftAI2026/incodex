use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_asar::{pack_dir, pack_dir_unpacked, patch_asar, Archive, LOADER_NAME, MARKER_KEY};
use sha2::{Digest, Sha256};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("incodex-asar-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_src(root: &std::path::Path, files: &[(&str, &str)]) -> PathBuf {
    let src = root.join("src");
    for (rel, body) in files {
        let dest = src.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(dest, body).unwrap();
    }
    src
}

fn pack(files: &[(&str, &str)]) -> PathBuf {
    let root = scratch();
    let src = write_src(&root, files);
    let archive = root.join("app.asar");
    pack_dir(&src, &archive).unwrap();
    archive
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn package_json_with_no_main_is_refused() {
    let archive = pack(&[("package.json", "{}\n"), ("index.js", "ok\n")]);
    assert_eq!(
        Archive::open(&archive)
            .unwrap()
            .read_package_main()
            .unwrap()
            .main,
        ""
    );
    let err = patch_asar(&archive, "/* loader */", None).unwrap_err();
    assert!(err.contains("no main"));
}

#[test]
fn missing_main_file_is_still_recorded() {
    let archive = pack(&[(
        "package.json",
        &format!("{}\n", serde_json::json!({"main":"missing.js"})),
    )]);
    assert_eq!(
        Archive::open(&archive)
            .unwrap()
            .read_package_main()
            .unwrap()
            .main,
        "missing.js"
    );
}

#[test]
fn paths_with_spaces_survive_rebuild() {
    let archive = pack(&[
        (
            "package.json",
            &format!("{}\n", serde_json::json!({"main":"index.js"})),
        ),
        ("index.js", "ok\n"),
        ("assets/my file.txt", "hello\n"),
    ]);
    patch_asar(&archive, "/* loader */", Some("space")).unwrap();
    let archive = Archive::open(&archive).unwrap();
    assert_eq!(
        String::from_utf8(archive.extract("assets/my file.txt").unwrap()).unwrap(),
        "hello\n"
    );
}

#[test]
fn already_patched_keeps_original_main() {
    let archive = pack(&[
        (
            "package.json",
            &format!("{}\n", serde_json::json!({"main":"index.js"})),
        ),
        ("index.js", "ok\n"),
    ]);
    patch_asar(&archive, "/* loader */", Some("one")).unwrap();
    let first = Archive::open(&archive)
        .unwrap()
        .read_package_main()
        .unwrap();
    assert!(first.already_patched);
    assert_eq!(first.main, "index.js");
    patch_asar(&archive, "/* loader */", Some("two")).unwrap();
    assert_eq!(
        Archive::open(&archive)
            .unwrap()
            .read_package_main()
            .unwrap()
            .main,
        "index.js"
    );
}

#[test]
fn packed_archive_keeps_file_bytes_and_sets_marker() {
    let archive = pack(&[
        (
            "package.json",
            &format!("{}\n", serde_json::json!({"main":"index.js"})),
        ),
        ("index.js", "module.exports = 1\n"),
        ("lib/util.js", "exports.ok = true\n"),
    ]);
    let before = Archive::open(&archive).unwrap();
    let index_before = before.extract("index.js").unwrap();
    let (hash, original) = patch_asar(&archive, "/* loader */", Some("pack")).unwrap();
    assert_eq!(original, "index.js");
    assert_eq!(hash.len(), 64);
    let after = Archive::open(&archive).unwrap();
    assert_eq!(after.header_hash(), hash);
    assert_eq!(after.extract("index.js").unwrap(), index_before);
    let listed = after.list();
    assert!(listed.iter().any(|p| p == "/index.js"));
    assert!(listed.iter().any(|p| p == "/lib/util.js"));
    assert!(listed.iter().any(|p| p == &format!("/{LOADER_NAME}")));
    assert!(!listed.iter().any(|p| p == "/incodex-main.cjs"));
    let pkg: serde_json::Value =
        serde_json::from_slice(&after.extract("package.json").unwrap()).unwrap();
    assert_eq!(pkg["main"], LOADER_NAME);
    assert_eq!(pkg[MARKER_KEY]["originalMain"], "index.js");
    assert_eq!(pkg[MARKER_KEY]["installId"], "pack");
}

#[test]
fn unpacked_directory_stays_unpacked() {
    let root = scratch();
    let src = write_src(
        &root,
        &[
            (
                "package.json",
                &format!("{}\n", serde_json::json!({"main":"index.js"})),
            ),
            ("index.js", "ok\n"),
            ("native/addon.node", "binary"),
            ("native/helper.bin", "help"),
        ],
    );
    let archive = root.join("app.asar");
    pack_dir_unpacked(&src, &archive, &["native"]).unwrap();
    assert_eq!(
        fs::read_to_string(format!("{}.unpacked/native/addon.node", archive.display())).unwrap(),
        "binary"
    );
    patch_asar(&archive, "/* loader */", Some("dir")).unwrap();
    assert_eq!(
        fs::read_to_string(format!("{}.unpacked/native/addon.node", archive.display())).unwrap(),
        "binary"
    );
    assert_eq!(
        fs::read_to_string(format!("{}.unpacked/native/helper.bin", archive.display())).unwrap(),
        "help"
    );
}

#[test]
fn internal_symlink_survives_rebuild() {
    let root = scratch();
    let src = write_src(
        &root,
        &[
            (
                "package.json",
                &format!("{}\n", serde_json::json!({"main":"index.js"})),
            ),
            ("index.js", "ok\n"),
            ("target.txt", "linked\n"),
        ],
    );
    symlink("target.txt", src.join("alias.txt")).unwrap();
    let archive = root.join("app.asar");
    pack_dir(&src, &archive).unwrap();
    patch_asar(&archive, "/* loader */", Some("link")).unwrap();
    let archive = Archive::open(&archive).unwrap();
    assert_eq!(
        String::from_utf8(archive.extract("target.txt").unwrap()).unwrap(),
        "linked\n"
    );
    assert_eq!(
        String::from_utf8(archive.extract("alias.txt").unwrap()).unwrap(),
        "linked\n"
    );
}

#[test]
fn injected_loader_overwrites_existing() {
    let archive = pack(&[
        (
            "package.json",
            &format!("{}\n", serde_json::json!({"main":"index.js"})),
        ),
        ("index.js", "ok\n"),
        (LOADER_NAME, "OLD LOADER\n"),
    ]);
    assert_eq!(
        String::from_utf8(
            Archive::open(&archive)
                .unwrap()
                .extract(LOADER_NAME)
                .unwrap()
        )
        .unwrap(),
        "OLD LOADER\n"
    );
    patch_asar(&archive, "/* loader */", Some("clash")).unwrap();
    let archive = Archive::open(&archive).unwrap();
    assert_eq!(
        String::from_utf8(archive.extract(LOADER_NAME).unwrap()).unwrap(),
        "/* loader */"
    );
    assert_eq!(
        String::from_utf8(archive.extract("index.js").unwrap()).unwrap(),
        "ok\n"
    );
}

#[test]
fn previous_full_runtime_is_stripped_to_loader() {
    let archive = pack(&[
        (
            "package.json",
            &format!("{}\n", serde_json::json!({"main":"index.js"})),
        ),
        ("index.js", "ok\n"),
        ("incodex-main.cjs", "OLD MAIN\n"),
        ("incodex-inject.js", "OLD INJECT\n"),
    ]);
    patch_asar(&archive, "/* loader */", Some("strip")).unwrap();
    let archive = Archive::open(&archive).unwrap();
    let listed = archive.list();
    assert!(listed.iter().any(|p| p.ends_with(LOADER_NAME)));
    assert!(!listed.iter().any(|p| p == "/incodex-main.cjs"));
    assert!(!listed.iter().any(|p| p == "/incodex-inject.js"));
    assert!(archive.has_only_loader());
}

#[test]
fn nonsense_file_offset_is_refused() {
    let archive = scratch().join("bad-offset.asar");
    let header = serde_json::json!({"files":{"index.js":{"size":2,"offset":"99999999"}}});
    let header_s = header.to_string();
    let mut bytes = vec![b'x'; 4];
    bytes.extend_from_slice(header_s.as_bytes());
    bytes.extend_from_slice(b"no");
    fs::write(&archive, bytes).unwrap();
    assert!(
        Archive::open(&archive).is_err()
            || Archive::open(&archive)
                .unwrap()
                .extract("index.js")
                .is_err()
    );
}

#[test]
fn multi_megabyte_blob_keeps_hash() {
    let big = "A".repeat(2 * 1024 * 1024);
    let archive = pack(&[
        (
            "package.json",
            &format!("{}\n", serde_json::json!({"main":"index.js"})),
        ),
        ("index.js", "ok\n"),
        ("blob.bin", &big),
    ]);
    let before = Archive::open(&archive)
        .unwrap()
        .extract("blob.bin")
        .unwrap();
    patch_asar(&archive, "/* loader */", Some("big")).unwrap();
    let after = Archive::open(&archive).unwrap();
    assert_eq!(after.extract("blob.bin").unwrap(), before);
    assert_eq!(
        String::from_utf8(after.extract("index.js").unwrap()).unwrap(),
        "ok\n"
    );
}

#[test]
fn packed_integrity_splits_files_into_electron_blocks() {
    let big = "A".repeat(4 * 1024 * 1024 + 1);
    let archive = pack(&[("package.json", "{}\n"), ("blob.bin", big.as_str())]);
    let archive = Archive::open(&archive).unwrap();
    let integrity = &archive.header["files"]["blob.bin"]["integrity"];
    let first_block = vec![b'A'; 4 * 1024 * 1024];
    let last_block = vec![b'A'];

    assert_eq!(integrity["algorithm"], "SHA256");
    assert_eq!(integrity["blockSize"], 4_194_304);
    assert_eq!(integrity["hash"], sha256_hex(big.as_bytes()));
    assert_eq!(
        integrity["blocks"],
        serde_json::json!([sha256_hex(&first_block), sha256_hex(&last_block),])
    );
}

#[test]
fn real_codex_asar_is_readable_when_present() {
    let path = std::path::Path::new("/Applications/ChatGPT.app/Contents/Resources/app.asar");
    if !path.exists() {
        return;
    }
    let archive = Archive::open(path).unwrap();
    let pkg = archive.read_package_main().unwrap();
    assert!(!pkg.main.is_empty());
    assert!(archive.list().iter().any(|p| p.ends_with("package.json")));
    let copy = scratch().join("app.asar");
    fs::copy(path, &copy).unwrap();
    if let Ok(unpacked) = fs::read_dir(format!("{}.unpacked", path.display())) {
        let _ = unpacked;
        let _ = fs::copy(
            format!("{}.unpacked", path.display()),
            format!("{}.unpacked", copy.display()),
        );
    }
    let listed_before = archive.list().len();
    patch_asar(&copy, "/* loader */", Some("live-diff")).unwrap();
    let patched = Archive::open(&copy).unwrap();
    assert!(patched.list().len() >= listed_before);
    assert_eq!(patched.read_package_main().unwrap().main, pkg.main);
}

fn bun_available() -> bool {
    Command::new("bun")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn rust_reads_electron_packed_archive() {
    if !bun_available() {
        return;
    }
    let root = scratch();
    let src = write_src(
        &root,
        &[
            (
                "package.json",
                &format!("{}\n", serde_json::json!({"main":"index.js"})),
            ),
            ("index.js", "hello-oracle\n"),
        ],
    );
    let archive = root.join("app.asar");
    let script = format!(
        r#"import {{ createPackageWithOptions }} from "@electron/asar"; await createPackageWithOptions({src}, {dest}, {{}});"#,
        src = serde_json::to_string(&src).unwrap(),
        dest = serde_json::to_string(&archive).unwrap()
    );
    let packed = Command::new("bun")
        .args(["-e", &script])
        .current_dir(repo_root())
        .status()
        .unwrap();
    assert!(packed.success());
    let archive = Archive::open(&archive).unwrap();
    assert_eq!(
        String::from_utf8(archive.extract("index.js").unwrap()).unwrap(),
        "hello-oracle\n"
    );
    assert_eq!(archive.read_package_main().unwrap().main, "index.js");
}

#[test]
fn electron_deduplicated_offsets_survive_rust_patch() {
    if !bun_available() {
        return;
    }
    let root = scratch();
    let src = write_src(
        &root,
        &[
            (
                "package.json",
                &format!("{}\n", serde_json::json!({"main":"index.js"})),
            ),
            ("index.js", "hello-oracle\n"),
            ("duplicate-a.txt", "shared-content\n"),
            ("duplicate-b.txt", "shared-content\n"),
        ],
    );
    let archive_path = root.join("app.asar");
    let pack_script = format!(
        r#"import {{ createPackageWithOptions }} from "@electron/asar"; await createPackageWithOptions({src}, {dest}, {{}});"#,
        src = serde_json::to_string(&src).unwrap(),
        dest = serde_json::to_string(&archive_path).unwrap()
    );
    let packed = Command::new("bun")
        .args(["-e", &pack_script])
        .current_dir(repo_root())
        .status()
        .unwrap();
    assert!(packed.success());

    let electron_archive = Archive::open(&archive_path).unwrap();
    assert_eq!(
        electron_archive.header["files"]["duplicate-a.txt"]["offset"],
        electron_archive.header["files"]["duplicate-b.txt"]["offset"]
    );
    assert_eq!(
        electron_archive.extract("duplicate-a.txt").unwrap(),
        electron_archive.extract("duplicate-b.txt").unwrap()
    );

    patch_asar(&archive_path, "/* loader */", Some("deduplicated")).unwrap();
    let patched = Archive::open(&archive_path).unwrap();
    assert_eq!(
        patched.extract("duplicate-a.txt").unwrap(),
        b"shared-content\n"
    );
    assert_eq!(
        patched.extract("duplicate-b.txt").unwrap(),
        b"shared-content\n"
    );
    assert_eq!(patched.read_package_main().unwrap().main, "index.js");

    let extract_script = format!(
        r#"import {{ extractFile }} from "@electron/asar"; const a = extractFile({archive}, "duplicate-a.txt"); const b = extractFile({archive}, "duplicate-b.txt"); if (!a.equals(b)) process.exit(1); process.stdout.write(a);"#,
        archive = serde_json::to_string(&archive_path).unwrap()
    );
    let extracted = Command::new("bun")
        .args(["-e", &extract_script])
        .current_dir(repo_root())
        .output()
        .unwrap();
    assert!(
        extracted.status.success(),
        "{}",
        String::from_utf8_lossy(&extracted.stderr)
    );
    assert_eq!(extracted.stdout, b"shared-content\n");
}

#[test]
fn electron_reads_rust_packed_archive() {
    if !bun_available() {
        return;
    }
    let archive = pack(&[
        (
            "package.json",
            &format!("{}\n", serde_json::json!({"main":"index.js"})),
        ),
        ("index.js", "from-rust\n"),
    ]);
    let script = format!(
        r#"import {{ extractFile }} from "@electron/asar"; process.stdout.write(extractFile({archive}, "index.js").toString("utf8"));"#,
        archive = serde_json::to_string(&archive).unwrap()
    );
    let out = Command::new("bun")
        .args(["-e", &script])
        .current_dir(repo_root())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "from-rust\n");
}

#[test]
fn crate_is_not_agpl() {
    let cargo = include_str!("../Cargo.toml");
    assert!(!cargo.to_lowercase().contains("agpl"));
}
