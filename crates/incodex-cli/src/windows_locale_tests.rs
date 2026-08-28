use std::fs;

use super::read_locale_override;

#[test]
fn locale_reader_refuses_an_oversized_config() {
    let root = std::env::temp_dir().join(format!(
        "incodex-windows-locale-limit-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create locale fixture");
    let file = fs::File::create(root.join("config.toml")).expect("create config");
    file.set_len(incodex_core::windows_session::MAX_WINDOWS_CONFIG_BYTES + 1)
        .expect("extend sparse config");

    assert_eq!(read_locale_override(&root), None);

    fs::remove_dir_all(root).expect("remove locale fixture");
}
