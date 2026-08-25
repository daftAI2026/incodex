use std::fs;
use std::path::{Path, PathBuf};

use incodex_asar::{pack_dir, patch_asar};
use incodex_macos::ditto;
use incodex_transaction::Engine;

pub struct CommittedInstall {
    pub app: PathBuf,
    pub transaction: PathBuf,
}

pub fn committed_install(home: &Path) -> CommittedInstall {
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    let source = home.join("asar-source");
    let candidate = home.join("candidate.app");
    let asar = app.join("Contents/Resources/app.asar");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(asar.parent().unwrap()).unwrap();
    fs::write(source.join("index.js"), b"official\n").unwrap();
    fs::write(source.join("package.json"), b"{\"main\":\"index.js\"}\n").unwrap();
    pack_dir(&source, &asar).unwrap();

    let mut transaction = Engine::begin(&root, &app, "test").unwrap();
    let install_id = transaction.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&install_id)
        .join("original/ChatGPT.app");
    fs::create_dir_all(original.parent().unwrap()).unwrap();
    ditto(&app, &original).unwrap();
    transaction.mark_backup_committed().unwrap();
    ditto(&app, &candidate).unwrap();
    patch_asar(
        &candidate.join("Contents/Resources/app.asar"),
        "module.exports = {};\n",
        Some(&install_id),
    )
    .unwrap();
    transaction.place_staging(&candidate).unwrap();
    transaction.swap().unwrap();
    transaction.commit().unwrap();

    CommittedInstall {
        app,
        transaction: root.join("transactions").join(install_id),
    }
}
