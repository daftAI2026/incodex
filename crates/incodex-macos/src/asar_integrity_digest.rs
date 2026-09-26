use serde_json::Value;

/// Plans a package-time Electron ASAR integrity digest update without changing
/// the input buffer. `None` means the framework has no active digest slot or
/// already contains the requested digest.
pub(crate) fn plan_integrity_digest_update(
    _macho: &[u8],
    _old_map: &Value,
    _new_map: &Value,
) -> Result<Option<Vec<u8>>, String> {
    Ok(None)
}
