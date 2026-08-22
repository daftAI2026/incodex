use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::pickle::{read_string_pickle, read_u32_pickle, write_string_pickle, write_u32_pickle};

pub const LOADER_NAME: &str = "incodex-loader.cjs";
pub const MARKER_KEY: &str = "__incodex";

const LEFTOVERS: &[&str] = &[
    "incodex-inject.js",
    "incodex-main.cjs",
    "incodex-preload.cjs",
    "incodex-safe-home.cjs",
    "incodex-ipc-guard.cjs",
    "incodex-owner-core.cjs",
    "incodex-owner-recovery.cjs",
    "incodex-instance.cjs",
    "incodex-runtime-load.cjs",
    "incodex-window-kind.cjs",
];
const ELECTRON_ASAR_BLOCK_SIZE: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Archive {
    pub path: PathBuf,
    pub header_string: String,
    pub header: Value,
    pub data_offset: u64,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PackageMain {
    pub main: String,
    pub already_patched: bool,
    pub install_id: Option<String>,
}

impl Archive {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let bytes = fs::read(&path).map_err(|err| err.to_string())?;
        if bytes.len() < 16 {
            return Err("asar too small".into());
        }
        let (header_pickle_size, size_used) = read_u32_pickle(&bytes)?;
        let header_start = size_used;
        let header_end = header_start + header_pickle_size as usize;
        if bytes.len() < header_end {
            return Err("asar header truncated".into());
        }
        let (header_string, _) = read_string_pickle(&bytes[header_start..header_end])?;
        let header: Value = serde_json::from_str(&header_string).map_err(|err| err.to_string())?;
        Ok(Self {
            path,
            header_string,
            header,
            data_offset: header_end as u64,
            bytes,
        })
    }

    pub fn header_hash(&self) -> String {
        sha256_hex(self.header_string.as_bytes())
    }

    pub fn file_hash(&self) -> String {
        sha256_hex(&self.bytes)
    }

    pub fn list(&self) -> Vec<String> {
        let mut out = Vec::new();
        list_files(self.files(), "", &mut out);
        out
    }

    pub fn extract(&self, rel: &str) -> Result<Vec<u8>, String> {
        extract_node(self, self.files(), rel.trim_start_matches('/'))
    }

    pub fn read_package_main(&self) -> Result<PackageMain, String> {
        let raw: Value = serde_json::from_slice(&self.extract("package.json")?)
            .map_err(|err| err.to_string())?;
        let marker = raw.get(MARKER_KEY);
        let original = marker
            .and_then(|m| m.get("originalMain"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let main = original
            .clone()
            .or_else(|| raw.get("main").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();
        Ok(PackageMain {
            already_patched: original.is_some(),
            install_id: marker
                .and_then(|m| m.get("installId"))
                .and_then(Value::as_str)
                .map(str::to_string),
            main,
        })
    }

    pub fn has_only_loader(&self) -> bool {
        self.extract(LOADER_NAME).is_ok() && self.extract("incodex-main.cjs").is_err()
    }

    fn files(&self) -> &Map<String, Value> {
        self.header
            .get("files")
            .and_then(Value::as_object)
            .expect("asar header missing files")
    }
}

pub fn pack_dir(src: &Path, dest: &Path) -> Result<(), String> {
    pack_dir_unpacked(src, dest, &[])
}

pub fn pack_dir_unpacked(
    src: &Path,
    dest: &Path,
    unpacked_prefixes: &[&str],
) -> Result<(), String> {
    let mut files = Map::new();
    let mut blobs = Vec::new();
    collect_pack(src, src, &mut files, &mut blobs, dest, unpacked_prefixes)?;
    write_archive(dest, &files, &blobs)
}

pub fn patch_asar(
    asar_path: &Path,
    loader_source: &str,
    install_id: Option<&str>,
) -> Result<(String, String), String> {
    let archive = Archive::open(asar_path)?;
    let pkg_main = archive.read_package_main()?;
    if pkg_main.main.is_empty() {
        return Err("package.json has no main".into());
    }
    let keep_main = pkg_main.main.clone();
    let mut pkg: Value =
        serde_json::from_slice(&archive.extract("package.json")?).map_err(|err| err.to_string())?;
    pkg["main"] = json!(LOADER_NAME);
    let mut marker = Map::new();
    marker.insert("originalMain".into(), json!(keep_main));
    if let Some(id) = install_id {
        marker.insert("installId".into(), json!(id));
    }
    pkg[MARKER_KEY] = Value::Object(marker);

    let mut files = Map::new();
    let mut blobs = Vec::new();
    copy_tree(&archive, archive.files(), "", &mut files, &mut blobs)?;
    files.remove("incodex-loader.cjs");
    for leftover in LEFTOVERS {
        files.remove(*leftover);
    }
    let mut pkg_bytes = serde_json::to_vec_pretty(&pkg).map_err(|err| err.to_string())?;
    pkg_bytes.push(b'\n');
    insert_packed_file(&mut files, &mut blobs, "package.json", pkg_bytes);
    insert_packed_file(
        &mut files,
        &mut blobs,
        LOADER_NAME,
        loader_source.as_bytes().to_vec(),
    );
    write_archive(asar_path, &files, &blobs)?;
    let patched = Archive::open(asar_path)?;
    Ok((patched.header_hash(), keep_main))
}

fn files_of(node: &Value) -> Option<&Map<String, Value>> {
    node.get("files").and_then(Value::as_object)
}

fn list_files(files: &Map<String, Value>, prefix: &str, out: &mut Vec<String>) {
    for (name, node) in files {
        let path = if prefix.is_empty() {
            format!("/{name}")
        } else {
            format!("{prefix}/{name}")
        };
        if files_of(node).is_some() {
            list_files(files_of(node).unwrap(), &path, out);
        } else {
            out.push(path);
        }
    }
}

fn extract_node(
    archive: &Archive,
    files: &Map<String, Value>,
    rel: &str,
) -> Result<Vec<u8>, String> {
    let mut current = files;
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    for (i, part) in parts.iter().enumerate() {
        let node = current
            .get(*part)
            .ok_or_else(|| format!("missing asar entry: {rel}"))?;
        if i + 1 == parts.len() {
            if let Some(link) = node.get("link").and_then(Value::as_str) {
                let parent = Path::new(rel).parent().unwrap_or(Path::new(""));
                let target = parent.join(link);
                return extract_node(archive, files, &target.to_string_lossy());
            }
            if node
                .get("unpacked")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let sibling = PathBuf::from(format!("{}.unpacked", archive.path.display()));
                return fs::read(sibling.join(rel)).map_err(|err| err.to_string());
            }
            let size = node
                .get("size")
                .and_then(Value::as_u64)
                .ok_or("file missing size")? as usize;
            let offset = node
                .get("offset")
                .and_then(Value::as_str)
                .ok_or("file missing offset")?
                .parse::<u64>()
                .map_err(|err| err.to_string())?;
            let start = archive.data_offset + offset;
            let end = start as usize + size;
            if end > archive.bytes.len() {
                return Err(format!("asar file offset out of range: {rel}"));
            }
            return Ok(archive.bytes[start as usize..end].to_vec());
        }
        current = files_of(node).ok_or_else(|| format!("not a directory: {part}"))?;
    }
    Err(format!("missing asar entry: {rel}"))
}

fn collect_pack(
    root: &Path,
    dir: &Path,
    files: &mut Map<String, Value>,
    blobs: &mut Vec<u8>,
    dest: &Path,
    unpacked_prefixes: &[&str],
) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|err| err.to_string())?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|err| err.to_string())?;
        if meta.file_type().is_symlink() {
            let link = fs::read_link(&path).map_err(|err| err.to_string())?;
            files.insert(name, json!({ "link": link.to_string_lossy() }));
        } else if meta.is_dir() {
            let mut child = Map::new();
            collect_pack(root, &path, &mut child, blobs, dest, unpacked_prefixes)?;
            files.insert(name, json!({ "files": child }));
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let unpacked = unpacked_prefixes
                .iter()
                .any(|prefix| rel == *prefix || rel.starts_with(&format!("{prefix}/")));
            if unpacked {
                let unpacked_path =
                    PathBuf::from(format!("{}.unpacked", dest.display())).join(&rel);
                if let Some(parent) = unpacked_path.parent() {
                    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
                }
                fs::copy(&path, &unpacked_path).map_err(|err| err.to_string())?;
                files.insert(name, json!({ "size": meta.len(), "unpacked": true }));
            } else {
                let data = fs::read(&path).map_err(|err| err.to_string())?;
                insert_packed_file(files, blobs, &name, data);
            }
        }
    }
    Ok(())
}

fn copy_tree(
    archive: &Archive,
    files: &Map<String, Value>,
    prefix: &str,
    dest: &mut Map<String, Value>,
    blobs: &mut Vec<u8>,
) -> Result<(), String> {
    for (name, node) in files {
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if let Some(children) = files_of(node) {
            let mut child = Map::new();
            copy_tree(archive, children, &rel, &mut child, blobs)?;
            dest.insert(name.clone(), json!({ "files": child }));
            continue;
        }
        if let Some(link) = node.get("link").and_then(Value::as_str) {
            dest.insert(name.clone(), json!({ "link": link }));
            continue;
        }
        if node
            .get("unpacked")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            dest.insert(name.clone(), node.clone());
            continue;
        }
        let data = extract_node(archive, archive.files(), &rel)?;
        insert_packed_file(dest, blobs, name, data);
    }
    Ok(())
}

fn insert_packed_file(
    files: &mut Map<String, Value>,
    blobs: &mut Vec<u8>,
    name: &str,
    data: Vec<u8>,
) {
    let offset = blobs.len() as u64;
    let size = data.len() as u64;
    let integrity = file_integrity(&data);
    blobs.extend_from_slice(&data);
    files.insert(
        name.to_string(),
        json!({
            "size": size,
            "offset": offset.to_string(),
            "integrity": integrity
        }),
    );
}

fn file_integrity(data: &[u8]) -> Value {
    let mut blocks = data
        .chunks(ELECTRON_ASAR_BLOCK_SIZE)
        .map(sha256_hex)
        .collect::<Vec<_>>();
    if blocks.is_empty() {
        blocks.push(sha256_hex(data));
    }
    json!({
        "algorithm": "SHA256",
        "hash": sha256_hex(data),
        "blockSize": ELECTRON_ASAR_BLOCK_SIZE,
        "blocks": blocks,
    })
}

fn write_archive(dest: &Path, files: &Map<String, Value>, blobs: &[u8]) -> Result<(), String> {
    let header = json!({ "files": files });
    let header_string = serde_json::to_string(&header).map_err(|err| err.to_string())?;
    let header_pickle = write_string_pickle(&header_string);
    let size_pickle = write_u32_pickle(header_pickle.len() as u32);
    let mut out = Vec::with_capacity(size_pickle.len() + header_pickle.len() + blobs.len());
    out.extend_from_slice(&size_pickle);
    out.extend_from_slice(&header_pickle);
    out.extend_from_slice(blobs);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(dest, out).map_err(|err| err.to_string())?;
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn electron_asar_integrity(asar_path: &Path) -> Result<String, String> {
    Ok(Archive::open(asar_path)?.header_hash())
}
