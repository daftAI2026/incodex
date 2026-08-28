#![cfg(target_os = "windows")]

use incodex_cli::windows_update_repair::{
    classify_package_update, PackageUpdateObservation, PackageUpdateOutcome,
};

const FAMILY: &str = "OpenAI.Codex_2p2nqsd0c76g0";
const OLD_PACKAGE: &str = "OpenAI.Codex_26.820.9563.0_x64__2p2nqsd0c76g0";
const NEW_PACKAGE: &str = "OpenAI.Codex_26.825.3734.0_x64__2p2nqsd0c76g0";

fn observation(
    family: &str,
    target: &str,
    complete: bool,
    error_code: i32,
) -> PackageUpdateObservation {
    PackageUpdateObservation {
        source_package_full_name: OLD_PACKAGE.to_string(),
        target_package_full_name: target.to_string(),
        target_package_family_name: family.to_string(),
        complete,
        error_code,
    }
}

#[test]
fn only_a_successful_new_codex_generation_authorizes_repair() {
    assert_eq!(
        classify_package_update(
            FAMILY,
            OLD_PACKAGE,
            &observation("Other.App_publisher", NEW_PACKAGE, true, 0),
        ),
        PackageUpdateOutcome::Ignore,
    );
    assert_eq!(
        classify_package_update(
            FAMILY,
            OLD_PACKAGE,
            &observation(FAMILY, NEW_PACKAGE, false, 0),
        ),
        PackageUpdateOutcome::Updating,
    );
    assert_eq!(
        classify_package_update(
            FAMILY,
            OLD_PACKAGE,
            &observation(FAMILY, NEW_PACKAGE, true, -1),
        ),
        PackageUpdateOutcome::Failed,
    );
    assert_eq!(
        classify_package_update(
            FAMILY,
            OLD_PACKAGE,
            &observation(FAMILY, OLD_PACKAGE, true, 0),
        ),
        PackageUpdateOutcome::Ignore,
    );
    assert_eq!(
        classify_package_update(
            FAMILY,
            OLD_PACKAGE,
            &observation(FAMILY, NEW_PACKAGE, true, 0),
        ),
        PackageUpdateOutcome::Ready {
            target_package_full_name: NEW_PACKAGE.to_string(),
        },
    );
}
