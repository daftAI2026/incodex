use std::path::Path;

use sha2::{Digest, Sha256};

use crate::canonical::{canonical_path, is_official_app};
use crate::paths::DEFAULT_APP;

const HASH_LEN: usize = 12;

pub fn target_id(app_path: impl AsRef<Path>) -> String {
    let app_path = app_path.as_ref();
    if is_official_app(app_path, None) {
        let digest = sha256_hex(DEFAULT_APP);
        return format!("official-{}", &digest[..HASH_LEN]);
    }
    let real = canonical_path(app_path);
    let digest = sha256_hex(&real.to_string_lossy());
    format!("app-{}", &digest[..HASH_LEN])
}

fn sha256_hex(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unofficial_missing_app_is_app_prefix() {
        let path = std::env::temp_dir().join("Missing.app");
        let id = target_id(&path);
        assert!(id.starts_with("app-"));
        assert_eq!(id.len(), 16);
    }
}
