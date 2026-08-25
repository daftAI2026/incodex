use std::path::{Path, PathBuf};

use incodex_core::{format_kv, format_ok, format_step, format_warn};
use incodex_runtime_bundle::ensure_current;

use crate::parse::ParsedCli;
use crate::profile_mask::resolve_profile_mask;
use crate::CliFailure;

use super::{
    default_source_home, describe_incognito_open, format_session_cleanup,
    prepare_incognito_open_with_profile_mask, user_root, wait_and_burn, OpenExitCode,
    OPENING_MESSAGE,
};

pub fn run_open(parsed: &ParsedCli) -> Result<(), CliFailure> {
    let app_path = parsed
        .app
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(incodex_core::DEFAULT_APP));
    let profile_mask = resolve_profile_mask(
        parsed.mask,
        parsed.name.as_deref(),
        parsed.avatar.as_deref().map(Path::new),
    )
    .map_err(CliFailure::from)?;

    if parsed.dry_run {
        let (bin, _) = describe_incognito_open(&app_path).map_err(CliFailure::from)?;
        println!(
            "{}",
            format_step("Open incognito without patching Codex", None)
        );
        println!(
            "{}",
            format_kv("App", &app_path.display().to_string(), None)
        );
        println!("{}", format_kv("Binary", &bin.display().to_string(), None));
        if let Some(mask) = &profile_mask {
            println!("{}", format_kv("Profile", &mask.name, None));
        }
        println!("{}", format_warn("Dry run. No window opened.", None));
        return Ok(());
    }

    // Validate the executable before touching Runtime or creating a session.
    describe_incognito_open(&app_path).map_err(CliFailure::from)?;
    let root = user_root();
    ensure_current(&root).map_err(CliFailure::from)?;
    let source = default_source_home();
    let plan = prepare_incognito_open_with_profile_mask(
        &app_path,
        &root,
        &source,
        std::process::id() as i32,
        profile_mask,
    )?;
    println!("{}", format_step(OPENING_MESSAGE, None));
    println!(
        "{}",
        format_kv("Binary", &plan.bin.display().to_string(), None)
    );
    println!(
        "{}",
        format_kv("Home", &plan.home.display().to_string(), None)
    );
    println!("{}", format_kv("Session", &plan.session_id, None));
    let (process, cleanup) = wait_and_burn(&plan, &root, 250)?;
    let (ok, message) = format_session_cleanup(&cleanup);
    if ok {
        println!("{}", format_ok(&message, None));
    } else {
        println!("{}", format_warn(&message, None));
    }
    println!();
    let code = process.exit_code(&cleanup);
    if code == OpenExitCode::Success {
        Ok(())
    } else {
        Err(CliFailure::with_code(
            code.as_i32(),
            process.failure_message(code),
        ))
    }
}
