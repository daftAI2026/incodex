use std::io;
use std::path::PathBuf;

use super::diagnose_fs::collect_directory_entries;

#[test]
fn directory_entry_error_does_not_return_a_partial_checked_list() {
    let entries = vec![
        Ok(PathBuf::from("first")),
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "fixture")),
        Ok(PathBuf::from("last")),
    ];

    let result = collect_directory_entries(entries);

    assert!(result.is_err(), "an entry error must not be flattened away");
}
