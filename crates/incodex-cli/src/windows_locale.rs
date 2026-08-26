use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::locale::parse_locale_override;

pub(crate) fn read_locale_override(source_home: &Path) -> Option<String> {
    let file = File::open(source_home.join("config.toml")).ok()?;
    let limit = incodex_core::windows_session::MAX_WINDOWS_CONFIG_BYTES;
    if file.metadata().ok()?.len() > limit {
        return None;
    }
    let mut content = String::new();
    let bytes = file.take(limit + 1).read_to_string(&mut content).ok()? as u64;
    if bytes > limit {
        return None;
    }
    parse_locale_override(&content, &['"', '\''])
}

#[cfg(test)]
#[path = "windows_locale_tests.rs"]
mod tests;
