/*
 * [INPUT]: 依赖已提交安装身份、权限 owner/CAS、BeforeLaunch guide 与目标签名复验。
 * [OUTPUT]: 编排默认 live install 的授权、终态落盘和一次正常启动。
 * [POS]: install 的共享收尾；生产与隔离安装调用同一顺序，OS 动作留给闭包。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallPermissionDispatch {
    NotSelected,
    Completed,
}

pub(crate) fn run_after_install<G, L>(
    parsed: &ParsedCli,
    root: &Path,
    app: &Path,
    install_id: Option<&str>,
    run_guide: G,
    launch: L,
) -> Result<InstallPermissionDispatch, String>
where
    G: FnOnce(
        bool,
        &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<crate::accessibility_guide_host::Outcome, String>,
    L: FnOnce(&mut dyn FnMut() -> Result<(), String>) -> Result<(), String>,
{
    if !parsed.live || parsed.app.is_some() {
        return Ok(InstallPermissionDispatch::NotSelected);
    }

    let install_id = install_id.ok_or(
        "App installation finished, but its install identity is missing; permission setup was not started.",
    )?;
    let permission_lock = crate::accessibility_setup::acquire_guide_owner(root, app)?;
    let request_id = crate::accessibility_setup::request_setup(root, app, install_id).map_err(
        |error| {
            format!(
                "App installation finished, but permission setup could not be prepared: {error}. Run incodex install to retry."
            )
        },
    )?;
    crate::accessibility_setup::mark_continuation(
        root,
        app,
        install_id,
        &request_id,
        crate::accessibility_setup::ResumeReason::PermissionSetup,
    )?;
    println!(
        "{}",
        format_kv(
            "Permissions",
            "Checking ChatGPT Accessibility and Screen Recording; the shared native guide opens only if needed.",
            None
        )
    );

    let before_launch = crate::accessibility_guide_host::supports_preflight(app);
    let mut verify_target = || {
        validate_committed_live_snapshot(root, install_id, app)
            .map_err(|error| error.to_string())?;
        crate::macos_signing::verify_patched(root, app, None).map(|_| ())
    };
    let permission = run_guide(before_launch, &mut verify_target);
    let state = match &permission {
        Ok(crate::accessibility_guide_host::Outcome::Granted) => "granted",
        Ok(crate::accessibility_guide_host::Outcome::Pending) => "deferred",
        Ok(crate::accessibility_guide_host::Outcome::AwaitingEvidence) => "pending",
        Err(_) => "error",
    };
    if let Err(error) = crate::accessibility_setup::finish_owned_setup(
        root,
        app,
        install_id,
        &request_id,
        permission_lock,
        state,
    ) {
        let launch_note = if before_launch {
            " The automatic post-setup launch was not requested."
        } else {
            ""
        };
        println!("{}", format_warn(&format!("Installation is complete, but the permission result could not be recorded: {error}.{launch_note} Run `incodex perm` to retry."), None));
        // 安装已提交；终态未落盘则保留续接，不启动或宣称旧读数成功。
        return Ok(InstallPermissionDispatch::Completed);
    }

    if before_launch {
        if let Err(error) = launch(&mut verify_target) {
            println!(
                "{}",
                format_warn(
                    &format!(
                        "Installed, but the automatic post-setup launch did not complete: {error}"
                    ),
                    None
                )
            );
        }
    }
    match permission {
        Ok(crate::accessibility_guide_host::Outcome::Granted) => println!("{}", format_ok("Installed. ChatGPT Accessibility and Screen Recording access verified.", None)),
        Ok(crate::accessibility_guide_host::Outcome::Pending | crate::accessibility_guide_host::Outcome::AwaitingEvidence) => println!("{}", format_warn("Installed. Permission setup is unfinished; run `incodex perm` when ready. `incodex doctor` checks Accessibility only; it does not verify Screen Recording.", None)),
        Err(error) => println!("{}", format_warn(&format!("Installed, but permission setup could not finish: {error}. Run `incodex perm` to retry."), None)),
    }
    Ok(InstallPermissionDispatch::Completed)
}
