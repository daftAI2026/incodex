use std::fs::File;
use std::io::Read;
use std::path::Path;

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
    content.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        if name.trim() != "localeOverride" {
            return None;
        }
        let value = value.trim();
        value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

#[cfg(test)]
#[path = "windows_locale_tests.rs"]
mod tests;
