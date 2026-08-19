//! MIT ASAR subset used by Incodex. Do not depend on an AGPL asar crate.

mod archive;
mod pickle;

pub use archive::{
    electron_asar_integrity, pack_dir, pack_dir_unpacked, patch_asar, Archive, PackageMain, LOADER_NAME,
    MARKER_KEY,
};
