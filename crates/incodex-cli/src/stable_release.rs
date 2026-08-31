use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StableRelease {
    pub(crate) tag: String,
    pub(crate) version: [u64; 3],
}

#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
}

pub(crate) fn parse_latest_stable_release(metadata: &[u8]) -> Result<StableRelease, String> {
    let release: LatestRelease = serde_json::from_slice(metadata)
        .map_err(|_| "update failed: invalid latest release metadata".to_string())?;
    let raw = release.tag_name.strip_prefix('v').ok_or_else(|| {
        format!(
            "update failed: invalid latest release tag: {}",
            release.tag_name
        )
    })?;
    let version = parse_stable_version(raw).ok_or_else(|| {
        format!(
            "update failed: invalid latest release tag: {}",
            release.tag_name
        )
    })?;
    let canonical = format!("v{}.{}.{}", version[0], version[1], version[2]);
    if release.tag_name != canonical {
        return Err(format!(
            "update failed: invalid latest release tag: {}",
            release.tag_name
        ));
    }
    Ok(StableRelease {
        tag: release.tag_name,
        version,
    })
}

pub(crate) fn parse_stable_version(raw: &str) -> Option<[u64; 3]> {
    let mut values = [0_u64; 3];
    let mut parts = raw.split('.');
    for value in &mut values {
        let part = parts.next()?;
        if part.is_empty()
            || (part.len() > 1 && part.starts_with('0'))
            || !part.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        *value = part.parse().ok()?;
    }
    parts.next().is_none().then_some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_release_parser_is_platform_neutral() {
        let release =
            parse_latest_stable_release(br#"{"tag_name":"v9.9.9"}"#).expect("stable release");
        assert_eq!(release.tag, "v9.9.9");
        assert_eq!(release.version, [9, 9, 9]);

        for invalid in ["9.9.9", "v09.9.9", "v9.9.9-beta.1", "v9.9"] {
            let metadata = format!(r#"{{"tag_name":"{invalid}"}}"#);
            assert!(parse_latest_stable_release(metadata.as_bytes()).is_err());
        }
    }
}
